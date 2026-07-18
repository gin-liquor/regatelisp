//! A compile-only front end: turns source into a verified `IrModule`
//! without ever running user code. Used by `--dump-ir` and `--check`, and
//! available as a library entry point for anything that wants IR without
//! executing `print`, user function calls, or runtime loops.
//!
//! Because nothing runs, `for` cannot consult a live `GlobalStore` to
//! resolve its range as it does during normal execution. Instead this
//! module maintains its own restricted constant-folding environment,
//! populated only by top-level `def`s whose right-hand side can be proven
//! constant without evaluating anything -- so `(def count (+ 2 2))` counts,
//! but `(def count (print "4"))`, `(def count ((fn () 4)))`, and
//! `(def count (loop (i 0 4) i))` do not.

use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, ExprKind};
use crate::core::CoreExpr;
use crate::error::LispError;
use crate::expand::{self, ExpansionContext, ForConstantSource};
use crate::globals::GlobalRegistry;
use crate::ir::{IrModule, IrTopLevel};
use crate::lower::{self, LowerContext};
use crate::parser;
use crate::verify;

/// Tracks which globals a `Compiler` can currently prove are constant
/// integers, purely from source shape -- never by executing anything. Also
/// tracks which builtin operator names have been shadowed by a `def`, so
/// constant folding through `+ - * / %` and the comparison operators stops
/// being valid the moment that name is redefined to something else.
#[derive(Default)]
struct ConstantEnv {
    integers: HashMap<String, i64>,
    redefined_operators: HashSet<String>,
}

impl ConstantEnv {
    fn set_int(&mut self, name: &str, value: i64) {
        self.integers.insert(name.to_string(), value);
    }

    fn invalidate(&mut self, name: &str) {
        self.integers.remove(name);
    }

    /// Records that `name` no longer refers to its original builtin
    /// meaning (called for every `def`, since any global name could
    /// shadow an operator name).
    fn mark_possibly_redefined(&mut self, name: &str) {
        self.redefined_operators.insert(name.to_string());
    }

    fn operator_is_foldable(&self, name: &str) -> bool {
        !self.redefined_operators.contains(name)
    }
}

impl ForConstantSource for ConstantEnv {
    fn integer_constant(&self, name: &str) -> Option<i64> {
        self.integers.get(name).copied()
    }
}

/// Names still known to refer to the original builtin arithmetic
/// operators; used only for constant folding, so `for` can still treat
/// `(+ 2 2)` as a constant expansion-time value -- but not once `+` has
/// been redefined.
const FOLDABLE_BUILTIN_ARITHMETIC: &[&str] = &["+", "-", "*", "/", "%"];

/// Attempts to constant-fold `expr` to a known integer, using only the
/// restricted rule set the compile-only front end allows: integer
/// literals, known integer constant globals, `let`, an `if` whose
/// condition folds to a constant boolean, and arithmetic through names
/// still known to be the original builtins. Returns `None` (not an error)
/// when the expression cannot be proven constant -- callers decide what
/// that means (e.g. `for` still errors, but a `def` simply isn't recorded
/// as a constant).
fn try_fold_int(expr: &Expr, env: &ConstantEnv, locals: &HashMap<String, i64>) -> Option<i64> {
    match expr.kind() {
        ExprKind::Int(n) => Some(*n),
        ExprKind::Bool(_) | ExprKind::String(_) => None,
        ExprKind::Symbol(name) => locals
            .get(name)
            .copied()
            .or_else(|| env.integer_constant(name)),
        ExprKind::List(items) => try_fold_application(items, env, locals),
    }
}

fn try_fold_bool(expr: &Expr, env: &ConstantEnv, locals: &HashMap<String, i64>) -> Option<bool> {
    match expr.kind() {
        ExprKind::Bool(b) => Some(*b),
        ExprKind::List(items) => try_fold_bool_application(items, env, locals),
        _ => None,
    }
}

fn try_fold_application(
    items: &[Expr],
    env: &ConstantEnv,
    locals: &HashMap<String, i64>,
) -> Option<i64> {
    if let [op_expr, lhs, rhs] = items
        && let ExprKind::Symbol(op) = op_expr.kind()
        && FOLDABLE_BUILTIN_ARITHMETIC.contains(&op.as_str())
        && env.operator_is_foldable(op)
    {
        let a = try_fold_int(lhs, env, locals)?;
        let b = try_fold_int(rhs, env, locals)?;
        return match op.as_str() {
            "+" => a.checked_add(b),
            "-" => a.checked_sub(b),
            "*" => a.checked_mul(b),
            "/" if b != 0 => a.checked_div(b),
            "%" if b != 0 => a.checked_rem(b),
            _ => None,
        };
    }
    if let [kw_expr, bindings_expr, body] = items
        && let ExprKind::Symbol(kw) = kw_expr.kind()
        && kw == "let"
        && let ExprKind::List(binding_exprs) = bindings_expr.kind()
    {
        let mut new_locals = locals.clone();
        let mut computed = Vec::with_capacity(binding_exprs.len());
        for binding_expr in binding_exprs {
            let ExprKind::List(pair) = binding_expr.kind() else {
                return None;
            };
            let [name_expr, init] = pair.as_slice() else {
                return None;
            };
            let ExprKind::Symbol(name) = name_expr.kind() else {
                return None;
            };
            computed.push((name.clone(), try_fold_int(init, env, locals)?));
        }
        for (name, value) in computed {
            new_locals.insert(name, value);
        }
        return try_fold_int(body, env, &new_locals);
    }
    if let [kw_expr, cond, yes, no] = items
        && let ExprKind::Symbol(kw) = kw_expr.kind()
        && kw == "if"
    {
        let branch = if try_fold_bool(cond, env, locals)? {
            yes
        } else {
            no
        };
        return try_fold_int(branch, env, locals);
    }
    None
}

fn try_fold_bool_application(
    items: &[Expr],
    env: &ConstantEnv,
    locals: &HashMap<String, i64>,
) -> Option<bool> {
    if let [op_expr, lhs, rhs] = items
        && let ExprKind::Symbol(op) = op_expr.kind()
        && env.operator_is_foldable(op)
    {
        match op.as_str() {
            "=" | "!=" | "<" | "<=" | ">" | ">=" => {
                let a = try_fold_int(lhs, env, locals)?;
                let b = try_fold_int(rhs, env, locals)?;
                return Some(match op.as_str() {
                    "=" => a == b,
                    "!=" => a != b,
                    "<" => a < b,
                    "<=" => a <= b,
                    ">" => a > b,
                    ">=" => a >= b,
                    _ => unreachable!(),
                });
            }
            _ => {}
        }
    }
    if let [kw_expr, cond, yes, no] = items
        && let ExprKind::Symbol(kw) = kw_expr.kind()
        && kw == "if"
    {
        let branch = if try_fold_bool(cond, env, locals)? {
            yes
        } else {
            no
        };
        return try_fold_bool(branch, env, locals);
    }
    None
}

/// A compile-only front end. Holds the same kind of persistent state a
/// normal `Interpreter` keeps between calls (global registry, IR module),
/// plus a compile-time constant environment for `for` -- but no
/// `GlobalStore` and no output target, since it never executes anything.
pub struct Compiler {
    globals: GlobalRegistry,
    module: IrModule,
    constants: ConstantEnv,
    next_loop_id: u32,
}

impl Compiler {
    pub fn new() -> Self {
        let mut globals = GlobalRegistry::new();
        for (name, _) in crate::builtin::all() {
            globals.intern(name);
        }
        Compiler {
            globals,
            module: IrModule::new(),
            constants: ConstantEnv::default(),
            next_loop_id: 0,
        }
    }

    /// Compiles every top-level form in `source` into IR, verifying each
    /// as it is added, without executing any of it.
    pub fn compile_source(&mut self, source: &str) -> Result<Vec<IrTopLevel>, LispError> {
        let exprs = parser::parse_program(source)?;
        let mut top_levels = Vec::with_capacity(exprs.len());
        for expr in &exprs {
            top_levels.push(self.compile_expr(expr)?);
        }
        Ok(top_levels)
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<IrTopLevel, LispError> {
        let core = self.expand_only(expr)?;

        let base_function_count = self.module.functions.len();
        let mut lower_context = LowerContext::with_next_ids(
            &mut self.globals,
            base_function_count as u32,
            self.next_loop_id,
        );
        let (top_level, new_functions) = lower::lower_top_level(&core, &mut lower_context)?;
        self.next_loop_id = lower_context.next_loop_id();
        self.module.functions.extend(new_functions);

        if let Err(err) = verify::verify_top_level(&top_level, &self.module, &self.globals) {
            self.module.functions.truncate(base_function_count);
            return Err(err.into());
        }
        for function in &self.module.functions[base_function_count..] {
            if let Err(err) = verify::verify_function(function, &self.module, &self.globals) {
                self.module.functions.truncate(base_function_count);
                return Err(err.into());
            }
        }

        Ok(top_level)
    }

    /// Expands one reader expression to core AST and records any
    /// resulting `for`-constant update, without lowering, verifying, or
    /// executing anything. Exposed for `--dump-core`, which shows the
    /// expansion result of each top-level form in source order (so later
    /// `for`s still see earlier `def`-derived constants, matching what
    /// `compile_source`/`eval_source` would use).
    pub fn expand_only(&mut self, expr: &Expr) -> Result<CoreExpr, LispError> {
        let core = {
            let context = ExpansionContext::new(&self.constants);
            expand::expand(expr, &context)?
        };

        // Only a top-level `(def name expr)` whose right-hand side folds
        // to a known integer, purely from source shape, becomes a `for`
        // expansion-time constant for later top-level forms.
        if let ExprKind::List(items) = expr.kind()
            && let [kw_expr, name_expr, value_expr] = items.as_slice()
            && let ExprKind::Symbol(kw) = kw_expr.kind()
            && let ExprKind::Symbol(name) = name_expr.kind()
            && kw == "def"
        {
            // Any `def` might be shadowing a builtin operator name, so
            // constant folding through that name as an operator is no
            // longer valid regardless of what this definition's value is.
            self.constants.mark_possibly_redefined(name);
            match try_fold_int(value_expr, &self.constants, &HashMap::new()) {
                Some(n) => self.constants.set_int(name, n),
                None => self.constants.invalidate(name),
            }
        }

        Ok(core)
    }

    pub fn globals(&self) -> &GlobalRegistry {
        &self.globals
    }

    pub fn module(&self) -> &IrModule {
        &self.module
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Compiler::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_arithmetic_def_is_foldable() {
        let mut compiler = Compiler::new();
        compiler.compile_source("(def count (+ 2 2))").unwrap();
        let result = compiler.compile_source("(for (i 0 count) i)");
        assert!(result.is_ok());
    }

    #[test]
    fn print_def_is_not_foldable() {
        let mut compiler = Compiler::new();
        compiler
            .compile_source(r#"(def count (print "4"))"#)
            .unwrap();
        let result = compiler.compile_source("(for (i 0 count) i)");
        assert!(result.is_err());
    }

    #[test]
    fn function_call_def_is_not_foldable() {
        let mut compiler = Compiler::new();
        compiler.compile_source("(def count ((fn () 4)))").unwrap();
        let result = compiler.compile_source("(for (i 0 count) i)");
        assert!(result.is_err());
    }

    #[test]
    fn loop_def_is_not_foldable() {
        let mut compiler = Compiler::new();
        compiler
            .compile_source("(def count (loop (i 0 4) i))")
            .unwrap();
        let result = compiler.compile_source("(for (i 0 count) i)");
        assert!(result.is_err());
    }

    #[test]
    fn compile_source_never_produces_output() {
        // There is no output target on `Compiler` at all -- if this
        // compiled without error, no `print` could possibly have run.
        let mut compiler = Compiler::new();
        let result = compiler.compile_source(r#"(print "must-not-run\n")"#);
        assert!(result.is_ok());
    }
}
