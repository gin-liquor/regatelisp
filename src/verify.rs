//! Verifies that IR is internally consistent before it is executed or
//! handed to a backend. The lowerer should always produce IR that passes
//! this check, but verification is implemented independently so it can
//! also catch hand-built or otherwise malformed IR.

use std::collections::HashSet;

use crate::error::VerifyError;
use crate::globals::GlobalRegistry;
use crate::ids::LoopId;
use crate::ir::{IrBody, IrExpr, IrExprKind, IrFunction, IrModule, IrTopLevel};

pub fn verify_top_level(
    top_level: &IrTopLevel,
    module: &IrModule,
    globals: &GlobalRegistry,
) -> Result<(), VerifyError> {
    match top_level {
        IrTopLevel::Expr { body, .. } => verify_body(body, module, globals),
        IrTopLevel::Define {
            target,
            initializer,
            ..
        } => {
            if target.index() >= globals.len() {
                return Err(VerifyError::InvalidGlobalId(*target));
            }
            verify_body(initializer, module, globals)
        }
    }
}

/// Verifies every function currently in `module` (used after lowering a
/// whole top-level form, so any functions it just added get checked too).
pub fn verify_module(module: &IrModule, globals: &GlobalRegistry) -> Result<(), VerifyError> {
    for function in &module.functions {
        verify_function(function, module, globals)?;
    }
    Ok(())
}

fn verify_body(
    body: &IrBody,
    module: &IrModule,
    globals: &GlobalRegistry,
) -> Result<(), VerifyError> {
    verify_expr(&body.expr, body.local_count, 0, module, globals, &[])
}

/// Verifies a single function in isolation (used to check just the
/// functions newly added by lowering one top-level form, without
/// re-verifying every function already in the module).
pub fn verify_function(
    function: &IrFunction,
    module: &IrModule,
    globals: &GlobalRegistry,
) -> Result<(), VerifyError> {
    if function.parameter_count > function.local_count {
        return Err(VerifyError::InvalidParameterCount);
    }
    verify_expr(
        &function.body,
        function.local_count,
        function.capture_count,
        module,
        globals,
        &[],
    )
}

/// Walks `expr`, checking every slot/ID reference against the bounds of
/// the function it belongs to, and every `break` against `enclosing_loops`
/// (the stack of loop IDs lexically enclosing this point, within the same
/// function -- a `break` may not cross a function boundary because
/// `MakeClosure`/function bodies are verified with a fresh, empty stack).
fn verify_expr(
    expr: &IrExpr,
    local_count: u32,
    capture_count: u32,
    module: &IrModule,
    globals: &GlobalRegistry,
    enclosing_loops: &[LoopId],
) -> Result<(), VerifyError> {
    match &expr.kind {
        IrExprKind::Const(_) => Ok(()),
        IrExprKind::LoadLocal(slot) => {
            if slot.index() as u32 >= local_count {
                return Err(VerifyError::InvalidLocalSlot(*slot));
            }
            Ok(())
        }
        IrExprKind::LoadCapture(slot) => {
            if slot.index() as u32 >= capture_count {
                return Err(VerifyError::InvalidCaptureSlot(*slot));
            }
            Ok(())
        }
        IrExprKind::LoadGlobal(id) => {
            if id.index() >= globals.len() {
                return Err(VerifyError::InvalidGlobalId(*id));
            }
            Ok(())
        }
        IrExprKind::Let { bindings, body } => {
            for binding in bindings {
                verify_expr(
                    &binding.initializer,
                    local_count,
                    capture_count,
                    module,
                    globals,
                    enclosing_loops,
                )?;
                if binding.target.index() as u32 >= local_count {
                    return Err(VerifyError::InvalidLocalSlot(binding.target));
                }
            }
            verify_expr(
                body,
                local_count,
                capture_count,
                module,
                globals,
                enclosing_loops,
            )
        }
        IrExprKind::MakeClosure { function, captures } => {
            let target = module
                .function(*function)
                .ok_or(VerifyError::InvalidFunctionId(*function))?;
            if target.capture_count as usize != captures.len() {
                return Err(VerifyError::CaptureCountMismatch {
                    expected: target.capture_count,
                    got: captures.len() as u32,
                });
            }
            for source in captures {
                match source {
                    crate::ir::IrCaptureSource::Local(slot) => {
                        if slot.index() as u32 >= local_count {
                            return Err(VerifyError::InvalidLocalSlot(*slot));
                        }
                    }
                    crate::ir::IrCaptureSource::Capture(slot) => {
                        if slot.index() as u32 >= capture_count {
                            return Err(VerifyError::InvalidCaptureSlot(*slot));
                        }
                    }
                }
            }
            // The callee function itself is verified separately (with its
            // own fresh, empty loop stack) by `verify_module`.
            Ok(())
        }
        IrExprKind::Call { callee, arguments } => {
            verify_expr(
                callee,
                local_count,
                capture_count,
                module,
                globals,
                enclosing_loops,
            )?;
            for arg in arguments {
                verify_expr(
                    arg,
                    local_count,
                    capture_count,
                    module,
                    globals,
                    enclosing_loops,
                )?;
            }
            Ok(())
        }
        IrExprKind::If {
            condition,
            then_expr,
            else_expr,
        } => {
            verify_expr(
                condition,
                local_count,
                capture_count,
                module,
                globals,
                enclosing_loops,
            )?;
            verify_expr(
                then_expr,
                local_count,
                capture_count,
                module,
                globals,
                enclosing_loops,
            )?;
            verify_expr(
                else_expr,
                local_count,
                capture_count,
                module,
                globals,
                enclosing_loops,
            )
        }
        IrExprKind::RangeLoop {
            loop_id,
            variable,
            start,
            end,
            step,
            body,
        } => {
            if enclosing_loops.contains(loop_id) {
                return Err(VerifyError::DuplicateLoopId(*loop_id));
            }
            if variable.index() as u32 >= local_count {
                return Err(VerifyError::InvalidLocalSlot(*variable));
            }
            verify_expr(
                start,
                local_count,
                capture_count,
                module,
                globals,
                enclosing_loops,
            )?;
            verify_expr(
                end,
                local_count,
                capture_count,
                module,
                globals,
                enclosing_loops,
            )?;
            verify_expr(
                step,
                local_count,
                capture_count,
                module,
                globals,
                enclosing_loops,
            )?;
            let mut nested = enclosing_loops.to_vec();
            nested.push(*loop_id);
            verify_expr(body, local_count, capture_count, module, globals, &nested)
        }
        IrExprKind::StateLoop {
            loop_id,
            states,
            condition,
            updates,
            body,
        } => {
            if enclosing_loops.contains(loop_id) {
                return Err(VerifyError::DuplicateLoopId(*loop_id));
            }
            let mut declared = HashSet::new();
            for state in states {
                verify_expr(
                    &state.initializer,
                    local_count,
                    capture_count,
                    module,
                    globals,
                    enclosing_loops,
                )?;
                if state.target.index() as u32 >= local_count {
                    return Err(VerifyError::InvalidLocalSlot(state.target));
                }
                declared.insert(state.target);
            }
            verify_expr(
                condition,
                local_count,
                capture_count,
                module,
                globals,
                enclosing_loops,
            )?;
            let mut nested = enclosing_loops.to_vec();
            nested.push(*loop_id);
            verify_expr(body, local_count, capture_count, module, globals, &nested)?;
            if updates.len() != states.len() {
                return Err(VerifyError::InvalidStateLoopUpdate);
            }
            let mut updated = HashSet::new();
            for update in updates {
                if !declared.contains(&update.target) {
                    return Err(VerifyError::InvalidStateLoopUpdate);
                }
                updated.insert(update.target);
                verify_expr(
                    &update.value,
                    local_count,
                    capture_count,
                    module,
                    globals,
                    enclosing_loops,
                )?;
            }
            if updated.len() != declared.len() {
                return Err(VerifyError::InvalidStateLoopUpdate);
            }
            Ok(())
        }
        IrExprKind::Break { target, value } => {
            if !enclosing_loops.contains(target) {
                return Err(VerifyError::BreakTargetNotEnclosing(*target));
            }
            if let Some(value) = value {
                verify_expr(
                    value,
                    local_count,
                    capture_count,
                    module,
                    globals,
                    enclosing_loops,
                )?;
            }
            Ok(())
        }
        IrExprKind::Sequence(items) => {
            for item in items {
                verify_expr(
                    item,
                    local_count,
                    capture_count,
                    module,
                    globals,
                    enclosing_loops,
                )?;
            }
            Ok(())
        }
    }
}
