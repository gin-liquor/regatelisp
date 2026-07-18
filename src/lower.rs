//! Lowers Core AST into the structured IR: resolves every symbol to a
//! local slot, a capture slot, or a global ID; assigns local slots
//! (handling shadowing); turns `fn` into a function-table entry plus an
//! explicit `MakeClosure`; and resolves every `break` to a concrete
//! enclosing `LoopId`.
//!
//! Lowering never executes user code -- it only inspects the Core AST
//! shape and consults `GlobalRegistry`/known-integer-constant state (for
//! `for`, which must already be gone by this point) and, for ordinary
//! evaluation, the current `GlobalStore` only insofar as later stages read
//! it at runtime.

use std::collections::HashMap;

use crate::capture::CaptureList;
use crate::core::{CoreExpr, CoreExprKind, QuasiDatum};
use crate::error::{LowerError, Span};
use crate::globals::GlobalRegistry;
use crate::ids::{CaptureSlot, FunctionId, LocalSlot, LoopId};
use crate::ir::{
    IrBody, IrConst, IrExpr, IrExprKind, IrFunction, IrLetBinding, IrQuasiDatum, IrStateBinding,
    IrStateUpdate, IrTopLevel,
};
use crate::symbol::Symbol;

/// Special-form names that `def` may never bind -- recognized as syntax
/// before any name resolution happens, so a binding under one of these
/// names could never be referenced.
const RESERVED_DEF_NAMES: &[&str] = &[
    "fn",
    "let",
    "for",
    "loop",
    "def",
    "if",
    "break",
    "quote",
    "quasiquote",
    "unquote",
    "unquote-splicing",
    "gensym",
    "do",
    "true",
    "false",
];

fn binding_symbol(expr: &CoreExpr) -> Option<Symbol> {
    match expr.kind() {
        CoreExprKind::Symbol(name) => Some(Symbol::interned(name.clone())),
        CoreExprKind::GeneratedSymbol(symbol) => Some(symbol.clone()),
        _ => None,
    }
}

/// Where a resolved name lives, from the referencing function's point of
/// view. Resolution never falls back to a runtime string search: every
/// non-global reference becomes a concrete slot during lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedBinding {
    Local(LocalSlot),
    Capture(CaptureSlot),
}

/// One lexical block's name -> slot bindings within a single function.
type Scope = HashMap<Symbol, LocalSlot>;

/// Per-function lowering state: its lexical scope stack (innermost last),
/// its capture list (built up as free variables are discovered), its
/// running local-slot counter, and the stack of enclosing runtime loop IDs
/// (reset to empty at each function boundary, so `break` can never target
/// a loop in a different function).
struct FunctionScope {
    scopes: Vec<Scope>,
    captures: CaptureList,
    next_local: u32,
    loop_stack: Vec<LoopId>,
}

impl FunctionScope {
    fn new() -> Self {
        FunctionScope {
            scopes: vec![Scope::new()],
            captures: CaptureList::default(),
            next_local: 0,
            loop_stack: Vec::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &Symbol) -> Result<LocalSlot, LowerError> {
        let slot = LocalSlot(self.next_local);
        self.next_local = self
            .next_local
            .checked_add(1)
            .ok_or(LowerError::TooManyLocals)?;
        self.scopes
            .last_mut()
            .expect("a function scope always has at least one block")
            .insert(name.clone(), slot);
        Ok(slot)
    }

    /// Looks up `name` in this function's own lexical scopes only
    /// (innermost first), without consulting enclosing functions.
    fn resolve_own(&self, name: &Symbol) -> Option<LocalSlot> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }
}

/// Lowering context shared across an entire top-level form: the global
/// registry (shared with the rest of the interpreter across calls) and the
/// stack of function scopes currently being lowered (innermost last).
pub struct LowerContext<'a> {
    globals: &'a mut GlobalRegistry,
    /// Functions produced while lowering the current top-level form,
    /// indexed by `FunctionId(base_function_id + i)` -- i.e. slot `i` here
    /// always holds the function with ID `base_function_id + i`, even
    /// though functions are *completed* innermost-first (a nested `fn`
    /// finishes lowering, and so is known, before its enclosing `fn`
    /// does). Using `push` here instead would put functions in completion
    /// order, which does not match their allocation order and silently
    /// corrupts every later `FunctionId` lookup.
    functions: Vec<Option<IrFunction>>,
    base_function_id: u32,
    function_stack: Vec<FunctionScope>,
    next_function_id: u32,
    next_loop_id: u32,
}

impl<'a> LowerContext<'a> {
    /// Creates a lowering context that continues `FunctionId` allocation
    /// from `next_function_id` (the number of functions already committed
    /// to the module) and `LoopId` allocation from `next_loop_id`, so IDs
    /// stay unique and stable across every top-level form lowered over the
    /// lifetime of one `Interpreter`/`Compiler`, not just within a single
    /// call.
    pub fn new(globals: &'a mut GlobalRegistry) -> Self {
        LowerContext::with_next_ids(globals, 0, 0)
    }

    pub fn with_next_ids(
        globals: &'a mut GlobalRegistry,
        next_function_id: u32,
        next_loop_id: u32,
    ) -> Self {
        LowerContext {
            globals,
            functions: Vec::new(),
            base_function_id: next_function_id,
            function_stack: Vec::new(),
            next_function_id,
            next_loop_id,
        }
    }

    /// The next `LoopId` this context would allocate, i.e. the running
    /// total after everything lowered through it so far. Callers that
    /// lower more than one top-level form over time should feed this back
    /// in as `next_loop_id` for the following context, so loop IDs stay
    /// unique across the whole session.
    pub fn next_loop_id(&self) -> u32 {
        self.next_loop_id
    }

    fn alloc_function_id(&mut self) -> Result<FunctionId, LowerError> {
        let id = FunctionId(self.next_function_id);
        self.next_function_id = self
            .next_function_id
            .checked_add(1)
            .ok_or(LowerError::TooManyFunctions)?;
        self.functions.push(None);
        Ok(id)
    }

    /// Stores a completed function at the slot matching its `FunctionId`
    /// (reserved by `alloc_function_id` when the ID was first allocated),
    /// regardless of the order functions finish lowering in.
    fn place_function(&mut self, function: IrFunction) {
        let index = (function.id.0 - self.base_function_id) as usize;
        self.functions[index] = Some(function);
    }

    /// Drains the completed functions in `FunctionId` order, for the
    /// caller to append to the module. Every slot must be filled by the
    /// time this runs (lowering never returns successfully with a
    /// still-in-progress `fn`).
    fn take_functions(&mut self) -> Vec<IrFunction> {
        std::mem::take(&mut self.functions)
            .into_iter()
            .map(|f| f.expect("every allocated FunctionId is completed before lowering succeeds"))
            .collect()
    }

    fn alloc_loop_id(&mut self) -> Result<LoopId, LowerError> {
        let id = LoopId(self.next_loop_id);
        self.next_loop_id = self
            .next_loop_id
            .checked_add(1)
            .ok_or(LowerError::TooManyLoops)?;
        Ok(id)
    }

    fn current(&mut self) -> &mut FunctionScope {
        self.function_stack
            .last_mut()
            .expect("lowering always runs inside at least one function scope")
    }

    /// Resolves `name` against the current function's own scopes, then
    /// walks outward through enclosing functions, threading a capture
    /// through every function in between (so a value captured by an outer
    /// closure can be re-captured by an inner one). Returns `None` if no
    /// enclosing function scope binds the name at all, meaning it must be
    /// a global.
    fn resolve(&mut self, name: &Symbol) -> Option<ResolvedBinding> {
        let depth = self.function_stack.len();
        if depth == 0 {
            return None;
        }

        if let Some(slot) = self.function_stack[depth - 1].resolve_own(name) {
            return Some(ResolvedBinding::Local(slot));
        }

        // Find the nearest enclosing function (if any) that binds `name`,
        // either as one of its own locals or as something it has already
        // captured.
        let mut found_at: Option<(usize, ResolvedBinding)> = None;
        for i in (0..depth - 1).rev() {
            if let Some(slot) = self.function_stack[i].resolve_own(name) {
                found_at = Some((i, ResolvedBinding::Local(slot)));
                break;
            }
        }
        let (found_index, mut carry) = found_at?;

        // Thread the capture forward through every function from
        // `found_index + 1` up to and including the current function,
        // turning each hop into a `Local` or `Capture` re-capture.
        for i in (found_index + 1)..depth {
            let slot = match carry {
                ResolvedBinding::Local(local) => {
                    self.function_stack[i].captures.capture_local(local)
                }
                ResolvedBinding::Capture(cap) => {
                    self.function_stack[i].captures.capture_capture(cap)
                }
            };
            carry = ResolvedBinding::Capture(slot);
        }

        Some(carry)
    }
}

/// Lowers one reader/core top-level form into `IrTopLevel`, appending any
/// functions it defines to `context`'s pending function table. Call
/// `LowerContext::into_module` (via the caller) once the whole form has
/// lowered successfully to commit those functions -- on error, simply drop
/// the partially-built list so no half-formed function is ever kept.
pub fn lower_top_level(
    core: &CoreExpr,
    context: &mut LowerContext,
) -> Result<(IrTopLevel, Vec<IrFunction>), LowerError> {
    let span = Span::new(0, 0);

    if let CoreExprKind::List(items) = core.kind()
        && let Some(first) = items.first()
        && let CoreExprKind::Symbol(name) = first.kind()
        && name == "def"
    {
        let top_level = lower_def(&items[1..], context, span)?;
        return Ok((top_level, context.take_functions()));
    }

    context.function_stack.push(FunctionScope::new());
    let expr = lower_expr(core, context)?;
    let scope = context
        .function_stack
        .pop()
        .expect("just pushed a function scope");
    let body = IrBody {
        local_count: scope.next_local,
        expr,
    };
    Ok((IrTopLevel::Expr { body, span }, context.take_functions()))
}

fn lower_def(
    rest: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrTopLevel, LowerError> {
    let [name_expr, value_expr] = rest else {
        return Err(LowerError::InvalidCoreForm);
    };
    let CoreExprKind::Symbol(name) = name_expr.kind() else {
        return Err(LowerError::InvalidCoreForm);
    };
    if RESERVED_DEF_NAMES.contains(&name.as_str()) {
        return Err(LowerError::ReservedDefinitionName(name.clone()));
    }

    // Intern the target name *before* lowering the initializer, so a
    // self-referential definition (`(def recurse (fn (x) (recurse x)))`)
    // sees its own global id while its body is being lowered.
    let target = context.globals.intern(name);

    context.function_stack.push(FunctionScope::new());
    let expr = lower_expr(value_expr, context)?;
    let scope = context
        .function_stack
        .pop()
        .expect("just pushed a function scope");
    let initializer = IrBody {
        local_count: scope.next_local,
        expr,
    };

    Ok(IrTopLevel::Define {
        target,
        initializer,
        span,
    })
}

fn lower_expr(core: &CoreExpr, context: &mut LowerContext) -> Result<IrExpr, LowerError> {
    let span = Span::new(0, 0);
    match core.kind() {
        CoreExprKind::Int(n) => Ok(IrExpr::new(span, IrExprKind::Const(IrConst::Int(*n)))),
        CoreExprKind::Bool(b) => Ok(IrExpr::new(span, IrExprKind::Const(IrConst::Bool(*b)))),
        CoreExprKind::String(s) => Ok(IrExpr::new(
            span,
            IrExprKind::Const(IrConst::String(s.clone())),
        )),
        CoreExprKind::Symbol(name) => lower_symbol(&Symbol::interned(name.clone()), context, span),
        CoreExprKind::GeneratedSymbol(symbol) => lower_symbol(symbol, context, span),
        CoreExprKind::Quote(datum) => Ok(IrExpr::new(span, IrExprKind::Quote(datum.clone()))),
        CoreExprKind::QuasiQuote(template) => Ok(IrExpr::new(
            span,
            IrExprKind::QuasiQuote(lower_quasi_datum(template, context)?),
        )),
        CoreExprKind::Sequence(items) => {
            let mut lowered = Vec::with_capacity(items.len());
            for item in items {
                lowered.push(lower_expr(item, context)?);
            }
            Ok(IrExpr::new(span, IrExprKind::Sequence(lowered)))
        }
        CoreExprKind::List(items) => lower_list(items, context, span),
    }
}

fn lower_symbol(
    name: &Symbol,
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    match context.resolve(name) {
        Some(ResolvedBinding::Local(slot)) => Ok(IrExpr::new(span, IrExprKind::LoadLocal(slot))),
        Some(ResolvedBinding::Capture(slot)) => {
            Ok(IrExpr::new(span, IrExprKind::LoadCapture(slot)))
        }
        None => {
            // Not resolvable as local or capture: intern as a global. This
            // does *not* fail just because the global is not yet defined
            // -- that is a runtime concern (an unbound global only errors
            // when actually loaded), which is what preserves lazy `if`
            // branch evaluation and forward references like a function
            // reading a global defined after it.
            let Some(name) = name.as_interned() else {
                return Err(LowerError::UnboundGeneratedSymbol(name.to_string()));
            };
            let id = context.globals.intern(name);
            Ok(IrExpr::new(span, IrExprKind::LoadGlobal(id)))
        }
    }
}

fn lower_list(
    items: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    if items.is_empty() {
        // An empty application; carried through as a zero-argument call to
        // a `Unit` constant so the same "not callable" error surfaces at
        // runtime as before, without inventing a new IR node.
        return Ok(IrExpr::new(
            span,
            IrExprKind::Call {
                callee: Box::new(IrExpr::new(span, IrExprKind::Const(IrConst::Unit))),
                arguments: Vec::new(),
            },
        ));
    }

    if let CoreExprKind::Symbol(name) = items[0].kind() {
        match name.as_str() {
            "fn" => return lower_fn(&items[1..], context, span),
            "let" => return lower_let(&items[1..], context, span),
            "if" => return lower_if(&items[1..], context, span),
            "break" => return lower_break(&items[1..], context, span),
            "loop" => return lower_loop(&items[1..], context, span),
            "gensym" => return lower_gensym(&items[1..], context, span),
            "do" => return lower_do(&items[1..], context, span),
            "def" => return Err(LowerError::DefOutsideDirectTopLevel),
            _ => {}
        }
    }

    let callee = lower_expr(&items[0], context)?;
    let mut arguments = Vec::with_capacity(items.len() - 1);
    for item in &items[1..] {
        arguments.push(lower_expr(item, context)?);
    }
    Ok(IrExpr::new(
        span,
        IrExprKind::Call {
            callee: Box::new(callee),
            arguments,
        },
    ))
}

fn lower_do(
    expressions: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    let expressions = expressions
        .iter()
        .map(|expression| lower_expr(expression, context))
        .collect::<Result<_, _>>()?;
    Ok(IrExpr::new(span, IrExprKind::Do(expressions)))
}

fn lower_gensym(
    rest: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    let prefix = match rest {
        [] => None,
        [prefix] => Some(Box::new(lower_expr(prefix, context)?)),
        _ => return Err(LowerError::InvalidGensymSyntax { got: rest.len() }),
    };
    Ok(IrExpr::new(span, IrExprKind::Gensym { prefix }))
}

fn lower_quasi_datum(
    template: &QuasiDatum,
    context: &mut LowerContext,
) -> Result<IrQuasiDatum, LowerError> {
    Ok(match template {
        QuasiDatum::Datum(datum) => IrQuasiDatum::Datum(datum.clone()),
        QuasiDatum::List(items) => IrQuasiDatum::List(
            items
                .iter()
                .map(|item| lower_quasi_datum(item, context))
                .collect::<Result<_, _>>()?,
        ),
        QuasiDatum::Evaluate(expression) => {
            IrQuasiDatum::Evaluate(Box::new(lower_expr(expression, context)?))
        }
        QuasiDatum::Splice(expression) => {
            IrQuasiDatum::Splice(Box::new(lower_expr(expression, context)?))
        }
    })
}

fn lower_fn(
    rest: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    let [params_expr, body_expr] = rest else {
        return Err(LowerError::InvalidCoreForm);
    };
    let CoreExprKind::List(param_exprs) = params_expr.kind() else {
        return Err(LowerError::InvalidCoreForm);
    };

    let function_id = context.alloc_function_id()?;
    context.function_stack.push(FunctionScope::new());

    let mut param_names: Vec<Symbol> = Vec::with_capacity(param_exprs.len());
    for param_expr in param_exprs {
        let Some(param_name) = binding_symbol(param_expr) else {
            context.function_stack.pop();
            return Err(LowerError::InvalidCoreForm);
        };
        if param_names.contains(&param_name) {
            context.function_stack.pop();
            return Err(LowerError::DuplicateParameter(param_name.to_string()));
        }
        context.current().declare(&param_name)?;
        param_names.push(param_name);
    }

    let body_result = lower_expr(body_expr, context);
    let scope = context
        .function_stack
        .pop()
        .expect("just pushed a function scope for this fn");
    let body = body_result?;

    let function = IrFunction {
        id: function_id,
        name_hint: None,
        parameter_count: param_names.len() as u32,
        capture_count: scope.captures.len(),
        local_count: scope.next_local,
        body,
        span,
    };
    context.place_function(function);

    let captures = scope.captures.into_sources();
    Ok(IrExpr::new(
        span,
        IrExprKind::MakeClosure {
            function: function_id,
            captures,
        },
    ))
}

/// `(let ((name init) ...) body)`, parallel binding: every initializer is
/// lowered (and, at runtime, evaluated) in the *outer* scope before any
/// new slot is declared, so an initializer can never see a sibling binding
/// from the same `let` -- matching the existing Core-evaluator semantics.
fn lower_let(
    rest: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    let [bindings_expr, body_expr] = rest else {
        return Err(LowerError::InvalidCoreForm);
    };
    let CoreExprKind::List(binding_exprs) = bindings_expr.kind() else {
        return Err(LowerError::InvalidCoreForm);
    };

    let mut names = Vec::with_capacity(binding_exprs.len());
    let mut initializers = Vec::with_capacity(binding_exprs.len());
    for binding_expr in binding_exprs {
        let CoreExprKind::List(pair) = binding_expr.kind() else {
            return Err(LowerError::InvalidCoreForm);
        };
        let [name_expr, init_expr] = pair.as_slice() else {
            return Err(LowerError::InvalidCoreForm);
        };
        let Some(name) = binding_symbol(name_expr) else {
            return Err(LowerError::InvalidCoreForm);
        };
        if names.contains(&name) {
            return Err(LowerError::DuplicateBinding(name.to_string()));
        }
        // All initializers lower in the scope active before this `let`
        // introduces any of its own names.
        initializers.push(lower_expr(init_expr, context)?);
        names.push(name);
    }

    context.current().push_scope();
    let mut bindings = Vec::with_capacity(names.len());
    for (name, initializer) in names.into_iter().zip(initializers) {
        let target = context.current().declare(&name)?;
        bindings.push(IrLetBinding {
            target,
            initializer,
            span,
        });
    }
    let body_result = lower_expr(body_expr, context);
    context.current().pop_scope();
    let body = body_result?;

    Ok(IrExpr::new(
        span,
        IrExprKind::Let {
            bindings,
            body: Box::new(body),
        },
    ))
}

fn lower_if(
    rest: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    let [cond, yes, no] = rest else {
        return Err(LowerError::InvalidCoreForm);
    };
    let condition = lower_expr(cond, context)?;
    let then_expr = lower_expr(yes, context)?;
    let else_expr = lower_expr(no, context)?;
    Ok(IrExpr::new(
        span,
        IrExprKind::If {
            condition: Box::new(condition),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        },
    ))
}

fn lower_break(
    rest: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    let target = *context
        .current()
        .loop_stack
        .last()
        .ok_or(LowerError::BreakOutsideRuntimeLoop)?;
    let value = match rest {
        [] => None,
        [expr] => Some(Box::new(lower_expr(expr, context)?)),
        _ => return Err(LowerError::InvalidCoreForm),
    };
    Ok(IrExpr::new(span, IrExprKind::Break { target, value }))
}

/// Dispatches to the range-loop or general (state) loop form based on
/// shape, matching the existing Core-evaluator's own dispatch rule: a
/// 4-element form whose second element is `(while ...)` is a general loop.
fn lower_loop(
    rest: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    let is_general = rest.len() == 4
        && matches!(
            rest[1].kind(),
            CoreExprKind::List(x)
                if x.len() == 2 && matches!(x[0].kind(), CoreExprKind::Symbol(s) if s == "while")
        );
    if is_general {
        lower_general_loop(rest, context, span)
    } else {
        lower_range_loop(rest, context, span)
    }
}

fn lower_range_loop(
    rest: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    let [binding_expr, body_expr] = rest else {
        return Err(LowerError::InvalidCoreForm);
    };
    let CoreExprKind::List(binding_items) = binding_expr.kind() else {
        return Err(LowerError::InvalidCoreForm);
    };
    let (name, start_expr, end_expr, step_expr) = match binding_items.as_slice() {
        [name, start, end] => (name, start, end, None),
        [name, start, end, step] => (name, start, end, Some(step)),
        _ => return Err(LowerError::InvalidCoreForm),
    };
    let Some(var_name) = binding_symbol(name) else {
        return Err(LowerError::InvalidCoreForm);
    };

    // Bounds lower in the scope *before* the loop variable is declared,
    // matching runtime semantics (bounds evaluate once, outside the loop
    // variable's own scope).
    let start = lower_expr(start_expr, context)?;
    let end = lower_expr(end_expr, context)?;
    let step = match step_expr {
        Some(e) => lower_expr(e, context)?,
        None => IrExpr::new(span, IrExprKind::Const(IrConst::Int(1))),
    };

    let loop_id = context.alloc_loop_id()?;
    context.current().push_scope();
    let variable = context.current().declare(&var_name)?;
    context.current().loop_stack.push(loop_id);
    let body_result = lower_expr(body_expr, context);
    context.current().loop_stack.pop();
    context.current().pop_scope();
    let body = body_result?;

    Ok(IrExpr::new(
        span,
        IrExprKind::RangeLoop {
            loop_id,
            variable,
            start: Box::new(start),
            end: Box::new(end),
            step: Box::new(step),
            body: Box::new(body),
        },
    ))
}

/// `(loop ((state init)...) (while cond) (next ((state expr)...)) (do body))`
fn lower_general_loop(
    rest: &[CoreExpr],
    context: &mut LowerContext,
    span: Span,
) -> Result<IrExpr, LowerError> {
    let (states_expr, while_expr, next_expr, do_expr) = (&rest[0], &rest[1], &rest[2], &rest[3]);
    let CoreExprKind::List(state_items) = states_expr.kind() else {
        return Err(LowerError::InvalidCoreForm);
    };
    let CoreExprKind::List(w) = while_expr.kind() else {
        return Err(LowerError::InvalidCoreForm);
    };
    let CoreExprKind::List(n) = next_expr.kind() else {
        return Err(LowerError::InvalidCoreForm);
    };
    let CoreExprKind::List(d) = do_expr.kind() else {
        return Err(LowerError::InvalidCoreForm);
    };
    let [_, cond_core] = w.as_slice() else {
        return Err(LowerError::InvalidCoreForm);
    };
    let [_, next_states] = n.as_slice() else {
        return Err(LowerError::InvalidCoreForm);
    };
    let [_, body_core] = d.as_slice() else {
        return Err(LowerError::InvalidCoreForm);
    };
    let CoreExprKind::List(update_items) = next_states.kind() else {
        return Err(LowerError::InvalidCoreForm);
    };

    // State initializers lower in the outer scope, before any state name
    // is declared (parallel binding, same rule as `let`).
    let mut names = Vec::with_capacity(state_items.len());
    let mut initializer_irs = Vec::with_capacity(state_items.len());
    for item in state_items {
        let CoreExprKind::List(pair) = item.kind() else {
            return Err(LowerError::InvalidCoreForm);
        };
        let [name_expr, init] = pair.as_slice() else {
            return Err(LowerError::InvalidCoreForm);
        };
        let Some(name) = binding_symbol(name_expr) else {
            return Err(LowerError::InvalidCoreForm);
        };
        if names.contains(&name) {
            return Err(LowerError::DuplicateLoopState(name.to_string()));
        }
        initializer_irs.push(lower_expr(init, context)?);
        names.push(name);
    }

    let loop_id = context.alloc_loop_id()?;
    context.current().push_scope();
    let mut states = Vec::with_capacity(names.len());
    for (name, initializer) in names.iter().zip(initializer_irs) {
        let target = context.current().declare(name)?;
        states.push(IrStateBinding {
            target,
            initializer,
            span,
        });
    }

    let inner = (|| -> Result<(IrExpr, Vec<IrStateUpdate>, IrExpr), LowerError> {
        let condition = lower_expr(cond_core, context)?;
        context.current().loop_stack.push(loop_id);
        let body = lower_expr(body_core, context);
        context.current().loop_stack.pop();
        let body = body?;

        if update_items.len() != names.len() {
            return Err(LowerError::InvalidCoreForm);
        }
        let mut updates = Vec::with_capacity(update_items.len());
        for (update_item, expected_name) in update_items.iter().zip(names.iter()) {
            let CoreExprKind::List(pair) = update_item.kind() else {
                return Err(LowerError::InvalidCoreForm);
            };
            let [name_expr, value_core] = pair.as_slice() else {
                return Err(LowerError::InvalidCoreForm);
            };
            let Some(n) = binding_symbol(name_expr) else {
                return Err(LowerError::InvalidCoreForm);
            };
            if &n != expected_name {
                return Err(LowerError::InvalidCoreForm);
            }
            let value = lower_expr(value_core, context)?;
            let target = *context
                .current()
                .scopes
                .last()
                .and_then(|s| s.get(&n))
                .expect("state name was just declared in this scope");
            updates.push(IrStateUpdate {
                target,
                value,
                span,
            });
        }
        Ok((condition, updates, body))
    })();

    context.current().pop_scope();
    let (condition, updates, body) = inner?;

    Ok(IrExpr::new(
        span,
        IrExprKind::StateLoop {
            loop_id,
            states,
            condition: Box::new(condition),
            updates,
            body: Box::new(body),
        },
    ))
}
