//! Ties the whole pipeline together for normal (executing) use:
//!
//! ```text
//! source -> parse -> expand -> lower -> verify -> IR-evaluate
//! ```
//!
//! Every top-level form goes through all five stages before the next
//! form's expansion begins, so a `def` earlier in the source is visible
//! both as a runtime global and as an expansion-time `for` constant to
//! forms later in the same call (and in later calls on the same
//! `Interpreter`, since `FrontendSession` and `Runtime` both outlive a
//! single `eval_source` call).

use std::io::Write;

use crate::ast::Expr;
use crate::error::LispError;
use crate::expand::{self, ExpansionContext};
use crate::globals::{GlobalRegistry, RuntimeConstants};
use crate::ir::IrModule;
use crate::ir_eval::{self, Runtime};
use crate::lower::{self, LowerContext};
use crate::parser;
use crate::value::Value;
use crate::verify;

/// Front-end (compile-time) state: the global name registry and the
/// accumulated IR function table. Both must persist across `eval_source`
/// calls so `FunctionId`s stay valid and later forms can call functions
/// `def`ined earlier.
pub struct FrontendSession {
    pub globals: GlobalRegistry,
    pub module: IrModule,
    /// Next `LoopId` to allocate. `FunctionId` allocation continues from
    /// `module.functions.len()`, but `LoopId`s are not stored anywhere on
    /// `IrModule` itself, so the running counter is tracked here.
    next_loop_id: u32,
}

impl FrontendSession {
    pub fn new() -> Self {
        let mut globals = GlobalRegistry::new();
        for (name, _) in crate::builtin::all() {
            globals.intern(name);
        }
        FrontendSession {
            globals,
            module: IrModule::new(),
            next_loop_id: 0,
        }
    }
}

impl Default for FrontendSession {
    fn default() -> Self {
        FrontendSession::new()
    }
}

pub struct Interpreter<W: Write> {
    frontend: FrontendSession,
    runtime: Runtime<W>,
}

impl<W: Write> Interpreter<W> {
    pub fn new(output: W) -> Self {
        let frontend = FrontendSession::new();
        let runtime = Runtime::new(output);
        // Builtins are ordinary global values; bind them up front so
        // `LoadGlobal` on e.g. `+` resolves immediately, without a special
        // "is this a builtin" check anywhere in the IR pipeline.
        for (name, value) in crate::builtin::all() {
            let id = frontend
                .globals
                .lookup(name)
                .expect("builtins were interned in FrontendSession::new");
            runtime.global_values.define(id, value);
        }
        Interpreter { frontend, runtime }
    }

    /// Expands, lowers, verifies, and evaluates a single reader expression
    /// as a top-level form (so `def` is permitted).
    pub fn eval_expr(&mut self, expr: &Expr) -> Result<Value, LispError> {
        let core = {
            let constants = RuntimeConstants {
                registry: &self.frontend.globals,
                store: &self.runtime.global_values,
            };
            let context = ExpansionContext::new(&constants);
            expand::expand(expr, &context)?
        };

        let base_function_count = self.frontend.module.functions.len();
        let mut lower_context = LowerContext::with_next_ids(
            &mut self.frontend.globals,
            base_function_count as u32,
            self.frontend.next_loop_id,
        );
        let (top_level, new_functions) = match lower::lower_top_level(&core, &mut lower_context) {
            Ok(result) => result,
            Err(err) => {
                // Nothing has been committed to `self.frontend.module` yet
                // (`lower_top_level` only ever mutates its own pending
                // function list), so a lowering failure leaves the module
                // exactly as it was.
                return Err(err.into());
            }
        };
        self.frontend.next_loop_id = lower_context.next_loop_id();

        // Commit the new functions only now that lowering the whole
        // top-level form succeeded, so a half-lowered function (e.g. one
        // that failed on a `break` outside any loop) never lands in the
        // module.
        self.frontend.module.functions.extend(new_functions);

        if let Err(err) =
            verify::verify_top_level(&top_level, &self.frontend.module, &self.frontend.globals)
        {
            self.frontend.module.functions.truncate(base_function_count);
            return Err(err.into());
        }
        for function in &self.frontend.module.functions[base_function_count..] {
            if let Err(err) =
                verify::verify_function(function, &self.frontend.module, &self.frontend.globals)
            {
                self.frontend.module.functions.truncate(base_function_count);
                return Err(err.into());
            }
        }

        ir_eval::eval_ir_top_level(&top_level, &self.frontend.module, &mut self.runtime)
    }

    /// Evaluates every top-level expression in `source`, in order.
    pub fn eval_source(&mut self, source: &str) -> Result<Vec<Value>, LispError> {
        let exprs = parser::parse_program(source)?;
        let mut results = Vec::with_capacity(exprs.len());
        for expr in &exprs {
            results.push(self.eval_expr(expr)?);
        }
        Ok(results)
    }

    pub fn output(&self) -> &W {
        &self.runtime.output
    }

    pub fn output_mut(&mut self) -> &mut W {
        &mut self.runtime.output
    }

    pub fn into_output(self) -> W {
        self.runtime.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_writes_to_captured_output() {
        let mut interp = Interpreter::new(Vec::new());
        interp.eval_source(r#"(print "hello")"#).unwrap();
        assert_eq!(interp.output(), b"hello");
    }

    #[test]
    fn output_and_eval_results_stay_in_order() {
        let mut interp = Interpreter::new(Vec::new());
        let values = interp
            .eval_source("(print \"a\")\n(print \"b\")\n(+ 1 2)")
            .unwrap();
        assert_eq!(interp.output(), b"ab");
        assert_eq!(values.len(), 3);
    }
}
