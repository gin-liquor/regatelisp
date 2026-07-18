//! The IR interpreter: the canonical runtime for the language. Unlike the
//! old Core AST evaluator, it never looks up a variable by name at
//! runtime -- every reference is already a `LocalSlot`, `CaptureSlot`, or
//! `GlobalId` by the time IR reaches this module.

use std::io::Write;
use std::rc::Rc;

use crate::builtin::apply_builtin;
use crate::datum::Datum;
use crate::error::{EvalError, LispError};
use crate::globals::GlobalStore;
use crate::ids::{CaptureSlot, LocalSlot, LoopId};
use crate::ir::{
    IrBody, IrCaptureSource, IrConst, IrExpr, IrExprKind, IrModule, IrQuasiDatum, IrTopLevel,
};
use crate::symbol::{GensymId, Symbol};
use crate::value::{Closure, Value};

/// Execution state shared across an entire `Interpreter`'s lifetime:
/// global values, the output target, and runtime limits. Kept separate
/// from the IR module (function bodies) and the global registry (names),
/// per the module's single responsibility.
pub struct Runtime<W: Write> {
    pub global_values: Rc<GlobalStore>,
    pub output: W,
    pub limits: RuntimeLimits,
    next_gensym_id: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeLimits {
    /// Upper bound on total iterations a single runtime `loop` (range or
    /// general) may execute, guarding against runaway loops during
    /// interactive use. Not related to `for`'s separate expansion-time
    /// limit.
    pub max_loop_iterations: u64,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        RuntimeLimits {
            max_loop_iterations: 10_000_000,
        }
    }
}

impl<W: Write> Runtime<W> {
    pub fn new(output: W) -> Self {
        Runtime {
            global_values: Rc::new(GlobalStore::new()),
            output,
            limits: RuntimeLimits::default(),
            next_gensym_id: 0,
        }
    }

    fn allocate_gensym(&mut self, hint: Option<String>) -> Result<Symbol, EvalError> {
        let next = self
            .next_gensym_id
            .checked_add(1)
            .ok_or(EvalError::GensymIdOverflow)?;
        let id = GensymId(self.next_gensym_id);
        self.next_gensym_id = next;
        Ok(Symbol::generated(id, hint))
    }

    pub(crate) fn next_gensym_id(&self) -> u64 {
        self.next_gensym_id
    }

    pub(crate) fn set_next_gensym_id(&mut self, next: u64) {
        self.next_gensym_id = next;
    }
}

/// A single call/top-level frame: local variable storage sized to the
/// current `IrBody`/`IrFunction`'s `local_count`, plus the values this
/// frame's closure (if any) captured.
struct Frame {
    locals: Vec<Option<Value>>,
    captures: Vec<Value>,
}

impl Frame {
    fn new(local_count: u32, captures: Vec<Value>) -> Self {
        Frame {
            locals: vec![None; local_count as usize],
            captures,
        }
    }

    fn load_local(&self, slot: LocalSlot) -> Result<Value, EvalError> {
        match self.locals.get(slot.index()) {
            Some(Some(value)) => Ok(value.clone()),
            _ => Err(EvalError::Io {
                message: format!("internal IR error: uninitialized local slot {}", slot.0),
            }),
        }
    }

    fn store_local(&mut self, slot: LocalSlot, value: Value) {
        self.locals[slot.index()] = Some(value);
    }

    fn load_capture(&self, slot: CaptureSlot) -> Result<Value, EvalError> {
        self.captures
            .get(slot.index())
            .cloned()
            .ok_or_else(|| EvalError::Io {
                message: format!("internal IR error: invalid capture slot {}", slot.0),
            })
    }
}

/// Distinguishes an ordinary result from an in-flight `break`, which must
/// propagate up past intervening expressions (but never past a function
/// call boundary in well-formed IR) until it reaches the loop whose
/// `LoopId` it targets.
enum Flow {
    Value(Value),
    Break { target: LoopId, value: Value },
}

pub fn eval_ir_top_level<W: Write>(
    top_level: &IrTopLevel,
    module: &IrModule,
    runtime: &mut Runtime<W>,
) -> Result<Value, LispError> {
    match top_level {
        IrTopLevel::Expr { body, .. } => {
            let value = eval_body(body, &[], module, runtime)?;
            Ok(value)
        }
        IrTopLevel::Define {
            target,
            initializer,
            ..
        } => {
            let value = eval_body(initializer, &[], module, runtime)?;
            // Only reaching here (no error propagated out of
            // `eval_body`) means the initializer succeeded, so it is now
            // safe to update the global -- a failed initializer never
            // touches the existing binding.
            runtime.global_values.define(*target, value.clone());
            Ok(Value::Unit)
        }
    }
}

fn eval_body<W: Write>(
    body: &IrBody,
    captures: &[Value],
    module: &IrModule,
    runtime: &mut Runtime<W>,
) -> Result<Value, LispError> {
    let mut frame = Frame::new(body.local_count, captures.to_vec());
    match eval_expr(&body.expr, &mut frame, module, runtime)? {
        Flow::Value(v) => Ok(v),
        Flow::Break { .. } => Err(LispError::Eval(EvalError::Io {
            message: "internal IR error: break escaped its enclosing loop".to_string(),
        })),
    }
}

fn eval_expr<W: Write>(
    expr: &IrExpr,
    frame: &mut Frame,
    module: &IrModule,
    runtime: &mut Runtime<W>,
) -> Result<Flow, LispError> {
    match &expr.kind {
        IrExprKind::Const(c) => Ok(Flow::Value(const_to_value(c))),
        IrExprKind::Quote(datum) => Ok(Flow::Value(Value::Datum(Rc::new(datum.clone())))),
        IrExprKind::QuasiQuote(template) => eval_quasi_datum(template, frame, module, runtime),
        IrExprKind::Gensym { prefix } => {
            let hint = if let Some(prefix) = prefix {
                let value = match eval_expr(prefix, frame, module, runtime)? {
                    Flow::Value(value) => value,
                    flow @ Flow::Break { .. } => return Ok(flow),
                };
                match value {
                    Value::Datum(datum) => match datum.as_ref() {
                        Datum::Symbol(Symbol::Interned(name)) => Some(name.clone()),
                        _ => return Err(EvalError::InvalidGensymPrefix("datum").into()),
                    },
                    other => return Err(EvalError::InvalidGensymPrefix(other.type_name()).into()),
                }
            } else {
                None
            };
            let symbol = runtime.allocate_gensym(hint)?;
            Ok(Flow::Value(Value::Datum(Rc::new(Datum::Symbol(symbol)))))
        }
        IrExprKind::LoadLocal(slot) => Ok(Flow::Value(frame.load_local(*slot)?)),
        IrExprKind::LoadCapture(slot) => Ok(Flow::Value(frame.load_capture(*slot)?)),
        IrExprKind::LoadGlobal(id) => {
            let value = runtime
                .global_values
                .get(*id)
                .ok_or_else(|| LispError::Eval(EvalError::UndefinedSymbol(format!("@{}", id.0))))?;
            Ok(Flow::Value(value))
        }
        IrExprKind::Let { bindings, body } => {
            // Parallel binding: evaluate every initializer first (against
            // the frame as it stood before this `let`), collect the
            // results, and only then write them into their slots -- so an
            // initializer can never observe a sibling binding's slot.
            let mut computed = Vec::with_capacity(bindings.len());
            for binding in bindings {
                match eval_expr(&binding.initializer, frame, module, runtime)? {
                    Flow::Value(v) => computed.push(v),
                    flow @ Flow::Break { .. } => return Ok(flow),
                }
            }
            for (binding, value) in bindings.iter().zip(computed) {
                frame.store_local(binding.target, value);
            }
            eval_expr(body, frame, module, runtime)
        }
        IrExprKind::MakeClosure { function, captures } => {
            let mut values = Vec::with_capacity(captures.len());
            for source in captures {
                let value = match source {
                    IrCaptureSource::Local(slot) => frame.load_local(*slot)?,
                    IrCaptureSource::Capture(slot) => frame.load_capture(*slot)?,
                };
                values.push(value);
            }
            Ok(Flow::Value(Value::Closure(Rc::new(Closure {
                function: *function,
                captures: values,
            }))))
        }
        IrExprKind::Call { callee, arguments } => {
            let callee_value = match eval_expr(callee, frame, module, runtime)? {
                Flow::Value(v) => v,
                flow @ Flow::Break { .. } => return Ok(flow),
            };
            let mut arg_values = Vec::with_capacity(arguments.len());
            for arg in arguments {
                match eval_expr(arg, frame, module, runtime)? {
                    Flow::Value(v) => arg_values.push(v),
                    flow @ Flow::Break { .. } => return Ok(flow),
                }
            }
            Ok(Flow::Value(apply(
                callee_value,
                arg_values,
                module,
                runtime,
            )?))
        }
        IrExprKind::If {
            condition,
            then_expr,
            else_expr,
        } => match eval_expr(condition, frame, module, runtime)? {
            Flow::Value(Value::Bool(true)) => eval_expr(then_expr, frame, module, runtime),
            Flow::Value(Value::Bool(false)) => eval_expr(else_expr, frame, module, runtime),
            Flow::Value(other) => Err(LispError::Eval(EvalError::NonBooleanCondition(
                other.type_name(),
            ))),
            flow @ Flow::Break { .. } => Ok(flow),
        },
        IrExprKind::RangeLoop {
            loop_id,
            variable,
            start,
            end,
            step,
            body,
        } => eval_range_loop(
            *loop_id, *variable, start, end, step, body, frame, module, runtime,
        ),
        IrExprKind::StateLoop {
            loop_id,
            states,
            condition,
            updates,
            body,
        } => eval_state_loop(
            *loop_id, states, condition, updates, body, frame, module, runtime,
        ),
        IrExprKind::Break { target, value } => {
            let value = match value {
                Some(expr) => match eval_expr(expr, frame, module, runtime)? {
                    Flow::Value(v) => v,
                    flow @ Flow::Break { .. } => return Ok(flow),
                },
                None => Value::Unit,
            };
            Ok(Flow::Break {
                target: *target,
                value,
            })
        }
        IrExprKind::Sequence(items) => {
            for item in items {
                match eval_expr(item, frame, module, runtime)? {
                    Flow::Value(_) => {}
                    flow @ Flow::Break { .. } => return Ok(flow),
                }
            }
            Ok(Flow::Value(Value::Unit))
        }
    }
}

fn eval_quasi_datum<W: Write>(
    template: &IrQuasiDatum,
    frame: &mut Frame,
    module: &IrModule,
    runtime: &mut Runtime<W>,
) -> Result<Flow, LispError> {
    match template {
        IrQuasiDatum::Datum(datum) => Ok(Flow::Value(Value::Datum(Rc::new(datum.clone())))),
        IrQuasiDatum::List(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                match eval_quasi_datum(item, frame, module, runtime)? {
                    Flow::Value(Value::Datum(datum)) => values.push(datum.as_ref().clone()),
                    flow @ Flow::Break { .. } => return Ok(flow),
                    Flow::Value(_) => {
                        return Err(EvalError::Io {
                            message: "internal IR error: quasiquote produced a non-datum".into(),
                        }
                        .into());
                    }
                }
            }
            Ok(Flow::Value(Value::Datum(Rc::new(Datum::List(values)))))
        }
        IrQuasiDatum::Evaluate(expression) => {
            match eval_expr(expression, frame, module, runtime)? {
                Flow::Value(value) => {
                    Ok(Flow::Value(Value::Datum(Rc::new(value_to_datum(value)?))))
                }
                flow @ Flow::Break { .. } => Ok(flow),
            }
        }
    }
}

fn value_to_datum(value: Value) -> Result<Datum, EvalError> {
    match value {
        Value::Int(value) => Ok(Datum::Integer(value)),
        Value::Bool(value) => Ok(Datum::Bool(value)),
        Value::Datum(datum) => Ok(datum.as_ref().clone()),
        other => Err(EvalError::CannotConvertToDatum(other.type_name())),
    }
}

fn const_to_value(c: &IrConst) -> Value {
    match c {
        IrConst::Int(n) => Value::Int(*n),
        IrConst::Bool(b) => Value::Bool(*b),
        IrConst::String(s) => Value::String(Rc::new(s.clone())),
        IrConst::Unit => Value::Unit,
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_range_loop<W: Write>(
    loop_id: LoopId,
    variable: LocalSlot,
    start: &IrExpr,
    end: &IrExpr,
    step: &IrExpr,
    body: &IrExpr,
    frame: &mut Frame,
    module: &IrModule,
    runtime: &mut Runtime<W>,
) -> Result<Flow, LispError> {
    let start = match eval_expr(start, frame, module, runtime)? {
        Flow::Value(v) => require_int_value(v)?,
        flow @ Flow::Break { .. } => return Ok(flow),
    };
    let end = match eval_expr(end, frame, module, runtime)? {
        Flow::Value(v) => require_int_value(v)?,
        flow @ Flow::Break { .. } => return Ok(flow),
    };
    let step = match eval_expr(step, frame, module, runtime)? {
        Flow::Value(v) => require_int_value(v)?,
        flow @ Flow::Break { .. } => return Ok(flow),
    };
    if step == 0 {
        return Err(LispError::Eval(EvalError::ZeroLoopStep));
    }

    let mut current = start;
    let mut iterations: u64 = 0;
    loop {
        let should_continue = if step > 0 {
            current < end
        } else {
            current > end
        };
        if !should_continue {
            break;
        }
        iterations += 1;
        if iterations > runtime.limits.max_loop_iterations {
            return Err(LispError::Eval(EvalError::LoopCounterOverflow));
        }

        frame.store_local(variable, Value::Int(current));
        match eval_expr(body, frame, module, runtime)? {
            Flow::Value(_) => {}
            Flow::Break {
                target,
                value: break_value,
            } => {
                if target == loop_id {
                    return Ok(Flow::Value(break_value));
                }
                return Ok(Flow::Break {
                    target,
                    value: break_value,
                });
            }
        }

        current = current
            .checked_add(step)
            .ok_or(EvalError::LoopCounterOverflow)?;
    }

    Ok(Flow::Value(Value::Unit))
}

fn require_int_value(value: Value) -> Result<i64, LispError> {
    match value {
        Value::Int(n) => Ok(n),
        _ => Err(LispError::Eval(EvalError::NonIntegerLoopBound)),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_state_loop<W: Write>(
    loop_id: LoopId,
    states: &[crate::ir::IrStateBinding],
    condition: &IrExpr,
    updates: &[crate::ir::IrStateUpdate],
    body: &IrExpr,
    frame: &mut Frame,
    module: &IrModule,
    runtime: &mut Runtime<W>,
) -> Result<Flow, LispError> {
    for state in states {
        match eval_expr(&state.initializer, frame, module, runtime)? {
            Flow::Value(v) => frame.store_local(state.target, v),
            flow @ Flow::Break { .. } => return Ok(flow),
        }
    }

    let mut iterations: u64 = 0;
    loop {
        match eval_expr(condition, frame, module, runtime)? {
            Flow::Value(Value::Bool(false)) => return Ok(Flow::Value(Value::Unit)),
            Flow::Value(Value::Bool(true)) => {}
            Flow::Value(other) => {
                return Err(LispError::Eval(EvalError::NonBooleanCondition(
                    other.type_name(),
                )));
            }
            flow @ Flow::Break { .. } => return Ok(flow),
        }

        iterations += 1;
        if iterations > runtime.limits.max_loop_iterations {
            return Err(LispError::Eval(EvalError::LoopCounterOverflow));
        }

        match eval_expr(body, frame, module, runtime)? {
            Flow::Value(_) => {}
            Flow::Break {
                target,
                value: break_value,
            } => {
                if target == loop_id {
                    return Ok(Flow::Value(break_value));
                }
                return Ok(Flow::Break {
                    target,
                    value: break_value,
                });
            }
        }

        // Parallel state update: evaluate every `next` expression against
        // the *current* state, then write all results at once.
        let mut computed = Vec::with_capacity(updates.len());
        for update in updates {
            match eval_expr(&update.value, frame, module, runtime)? {
                Flow::Value(v) => computed.push((update.target, v)),
                flow @ Flow::Break { .. } => return Ok(flow),
            }
        }
        for (target, value) in computed {
            frame.store_local(target, value);
        }
    }
}

fn apply<W: Write>(
    callee: Value,
    arguments: Vec<Value>,
    module: &IrModule,
    runtime: &mut Runtime<W>,
) -> Result<Value, LispError> {
    match callee {
        Value::Builtin(builtin) => Ok(apply_builtin(builtin, &arguments, &mut runtime.output)?),
        Value::Closure(closure) => {
            let function = module.function(closure.function).ok_or_else(|| {
                LispError::Eval(EvalError::Io {
                    message: format!(
                        "internal IR error: unknown function id {}",
                        closure.function.0
                    ),
                })
            })?;
            if arguments.len() != function.parameter_count as usize {
                return Err(LispError::Eval(EvalError::WrongArgCount {
                    expected: function.parameter_count as usize,
                    got: arguments.len(),
                }));
            }
            let mut frame = Frame::new(function.local_count, closure.captures.clone());
            for (i, arg) in arguments.into_iter().enumerate() {
                frame.store_local(LocalSlot(i as u32), arg);
            }
            match eval_expr(&function.body, &mut frame, module, runtime)? {
                Flow::Value(v) => Ok(v),
                Flow::Break { .. } => Err(LispError::Eval(EvalError::Io {
                    message: "internal IR error: break escaped its enclosing loop".to_string(),
                })),
            }
        }
        other => Err(LispError::Eval(EvalError::NotCallable(other.to_string()))),
    }
}

#[cfg(test)]
mod stage_eight_tests {
    use super::*;

    #[test]
    fn gensym_counter_overflow_is_an_error() {
        let mut runtime = Runtime::new(Vec::new());
        runtime.next_gensym_id = u64::MAX;
        assert!(matches!(
            runtime.allocate_gensym(None),
            Err(EvalError::GensymIdOverflow)
        ));
    }
}
