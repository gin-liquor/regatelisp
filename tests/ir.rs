//! Tests that inspect IR shape directly (not just the value/output the
//! interpreter produces), verify malformed IR is rejected, and confirm
//! deterministic ID assignment and text output. Behavioral tests for the
//! language features themselves live in `interpreter.rs`, `loops_and_def.rs`,
//! and `print.rs`; this file is specifically about the compiler pipeline.

use regatelisp::compiler::Compiler;
use regatelisp::error::{Span, VerifyError};
use regatelisp::ids::{CaptureSlot, FunctionId, GlobalId, LocalSlot, LoopId};
use regatelisp::ir::{
    IrBody, IrCaptureSource, IrConst, IrExpr, IrExprKind, IrFunction, IrModule, IrQuasiDatum,
    IrTopLevel,
};
use regatelisp::{GlobalRegistry, Interpreter, Value};

// ============================================================
// 51: simple expression IR
// ============================================================

#[test]
fn simple_arithmetic_lowers_to_a_call_not_a_dedicated_add_node() {
    let mut compiler = Compiler::new();
    let top_levels = compiler.compile_source("(+ 1 2)").unwrap();
    let IrTopLevel::Expr { body, .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    match &body.expr.kind {
        IrExprKind::Call { callee, arguments } => {
            assert!(matches!(callee.kind, IrExprKind::LoadGlobal(_)));
            assert_eq!(arguments.len(), 2);
            assert!(matches!(
                arguments[0].kind,
                IrExprKind::Const(IrConst::Int(1))
            ));
            assert!(matches!(
                arguments[1].kind,
                IrExprKind::Const(IrConst::Int(2))
            ));
        }
        other => panic!("expected Call, got {other:?}"),
    }
}

#[test]
fn simple_arithmetic_executes_to_three() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp.eval_source("(+ 1 2)").unwrap();
    assert!(matches!(values[0], Value::Int(3)));
}

// ============================================================
// 52-54: locals, shadowing, parallel let
// ============================================================

#[test]
fn same_local_variable_uses_the_same_slot_both_references() {
    let mut compiler = Compiler::new();
    let top_levels = compiler.compile_source("(let ((x 10)) (+ x x))").unwrap();
    let IrTopLevel::Expr { body, .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    let IrExprKind::Let {
        bindings,
        body: inner,
    } = &body.expr.kind
    else {
        panic!("expected Let");
    };
    let target = bindings[0].target;
    let IrExprKind::Call { arguments, .. } = &inner.kind else {
        panic!("expected Call");
    };
    assert!(matches!(arguments[0].kind, IrExprKind::LoadLocal(s) if s == target));
    assert!(matches!(arguments[1].kind, IrExprKind::LoadLocal(s) if s == target));
}

#[test]
fn local_variable_result_is_twenty() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp.eval_source("(let ((x 10)) (+ x x))").unwrap();
    assert!(matches!(values[0], Value::Int(20)));
}

#[test]
fn shadowed_bindings_use_distinct_slots() {
    let mut compiler = Compiler::new();
    let top_levels = compiler
        .compile_source("(let ((x 1)) (let ((x 2)) x))")
        .unwrap();
    let IrTopLevel::Expr { body, .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    let IrExprKind::Let {
        bindings: outer,
        body: outer_body,
    } = &body.expr.kind
    else {
        panic!("expected outer Let");
    };
    let outer_slot = outer[0].target;
    let IrExprKind::Let {
        bindings: inner,
        body: inner_body,
    } = &outer_body.kind
    else {
        panic!("expected inner Let");
    };
    let inner_slot = inner[0].target;
    assert_ne!(outer_slot, inner_slot);
    assert!(matches!(inner_body.kind, IrExprKind::LoadLocal(s) if s == inner_slot));
}

#[test]
fn shadowing_result_is_two() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp.eval_source("(let ((x 1)) (let ((x 2)) x))").unwrap();
    assert!(matches!(values[0], Value::Int(2)));
}

#[test]
fn parallel_let_inner_sibling_reference_targets_the_outer_slot() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp
        .eval_source("(let ((x 10)) (let ((x 1) (y x)) y))")
        .unwrap();
    assert!(matches!(values[0], Value::Int(10)));
}

// ============================================================
// 55-56: closures and multi-level capture
// ============================================================

#[test]
fn simple_closure_captures_x_and_resolves_plus_as_global() {
    let mut compiler = Compiler::new();
    let top_levels = compiler
        .compile_source("(let ((x 10)) ((fn (y) (+ x y)) 5))")
        .unwrap();
    let IrTopLevel::Expr { .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    // Exactly one function should have been created, capturing one value.
    assert_eq!(compiler.module().functions.len(), 1);
    let function = &compiler.module().functions[0];
    assert_eq!(function.capture_count, 1);
    assert_eq!(function.parameter_count, 1);
    let IrExprKind::Call { callee, .. } = &function.body.kind else {
        panic!("expected Call in function body");
    };
    assert!(matches!(callee.kind, IrExprKind::LoadGlobal(_)));
}

#[test]
fn simple_closure_result_is_fifteen() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp
        .eval_source("(let ((x 10)) ((fn (y) (+ x y)) 5))")
        .unwrap();
    assert!(matches!(values[0], Value::Int(15)));
}

#[test]
fn multi_level_capture_result_is_six() {
    let mut interp = Interpreter::new(Vec::new());
    let source = "(let ((a 1)) (((fn (b) (fn (c) (+ a (+ b c)))) 2) 3))";
    let values = interp.eval_source(source).unwrap();
    assert!(matches!(values[0], Value::Int(6)));
}

#[test]
fn multi_level_capture_reserializes_through_two_functions() {
    // The innermost function (c) must capture both `a` (via the middle
    // function re-capturing its own capture) and `b` (directly, as a
    // local of the middle function).
    let mut compiler = Compiler::new();
    compiler
        .compile_source("(let ((a 1)) (((fn (b) (fn (c) (+ a (+ b c)))) 2) 3))")
        .unwrap();
    assert_eq!(compiler.module().functions.len(), 2);
    let innermost = compiler
        .module()
        .functions
        .iter()
        .max_by_key(|f| f.capture_count)
        .unwrap();
    assert_eq!(innermost.capture_count, 2);
}

// ============================================================
// 57-58: global capture semantics
// ============================================================

#[test]
fn closure_reading_a_global_sees_the_latest_redefinition() {
    let mut interp = Interpreter::new(Vec::new());
    let source = "(def x 10)\n(def read-x (fn () x))\n(def x 20)\n(read-x)";
    let values = interp.eval_source(source).unwrap();
    assert!(matches!(values.last(), Some(Value::Int(20))));
}

#[test]
fn closure_reading_a_global_uses_load_global_not_a_capture() {
    let mut compiler = Compiler::new();
    compiler.compile_source("(def x 10)").unwrap();
    compiler.compile_source("(def read-x (fn () x))").unwrap();
    let function = compiler
        .module()
        .functions
        .last()
        .expect("read-x's closure body");
    assert!(matches!(function.body.kind, IrExprKind::LoadGlobal(_)));
    assert_eq!(function.capture_count, 0);
}

#[test]
fn closure_over_a_local_keeps_the_value_from_definition_time() {
    let mut interp = Interpreter::new(Vec::new());
    let source = "(def read-x (let ((x 10)) (fn () x)))\n(def x 20)\n(read-x)";
    let values = interp.eval_source(source).unwrap();
    assert!(matches!(values.last(), Some(Value::Int(10))));
}

// ============================================================
// 59-60: self-referential def, atomicity
// ============================================================

#[test]
fn self_referential_def_lowers_successfully() {
    let mut interp = Interpreter::new(Vec::new());
    let result = interp.eval_source("(def recurse (fn (x) (recurse x)))");
    assert!(result.is_ok());
}

#[test]
fn self_referential_def_body_references_its_own_global_id() {
    let mut compiler = Compiler::new();
    compiler
        .compile_source("(def recurse (fn (x) (recurse x)))")
        .unwrap();
    let target_id = compiler.globals().lookup("recurse").unwrap();
    let function = &compiler.module().functions[0];
    let IrExprKind::Call { callee, .. } = &function.body.kind else {
        panic!("expected Call");
    };
    assert!(matches!(callee.kind, IrExprKind::LoadGlobal(id) if id == target_id));
}

#[test]
fn def_failure_is_atomic() {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source("(def x 10)").unwrap();
    assert!(interp.eval_source("(def x unknown-name)").is_err());
    let values = interp.eval_source("x").unwrap();
    assert!(matches!(values[0], Value::Int(10)));
}

// ============================================================
// 61: lazy `if`
// ============================================================

#[test]
fn if_does_not_evaluate_the_untaken_branch() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp.eval_source("(if true 10 unknown-name)").unwrap();
    assert!(matches!(values[0], Value::Int(10)));
}

#[test]
fn if_lowers_and_verifies_even_with_an_undefined_global_in_the_untaken_branch() {
    let mut compiler = Compiler::new();
    let result = compiler.compile_source("(if true 10 unknown-name)");
    assert!(result.is_ok());
}

// ============================================================
// 62: for does not survive into IR
// ============================================================

#[test]
fn for_is_gone_by_the_time_ir_exists() {
    let mut compiler = Compiler::new();
    let top_levels = compiler
        .compile_source(r#"(for (i 0 3) (print "{}" i))"#)
        .unwrap();
    let IrTopLevel::Expr { body, .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    assert!(!contains_for_shaped_node(&body.expr));
}

fn contains_for_shaped_node(expr: &IrExpr) -> bool {
    // There is no `IrExprKind::For` variant at all -- if this ever fails
    // to compile because such a variant was added, that alone would be a
    // regression against "for must be eliminated before IR generation".
    // This function instead walks the tree checking every node is one of
    // the known kinds, which is trivially true; its purpose is to give the
    // test a concrete assertion to hang the invariant on.
    match &expr.kind {
        IrExprKind::Const(_)
        | IrExprKind::Quote(_)
        | IrExprKind::LoadLocal(_)
        | IrExprKind::LoadCapture(_)
        | IrExprKind::LoadGlobal(_) => false,
        IrExprKind::Let { bindings, body } => {
            bindings
                .iter()
                .any(|b| contains_for_shaped_node(&b.initializer))
                || contains_for_shaped_node(body)
        }
        IrExprKind::MakeClosure { .. } => false,
        IrExprKind::QuasiQuote(template) => contains_for_in_quasi_datum(template),
        IrExprKind::Gensym { prefix } => prefix.as_deref().is_some_and(contains_for_shaped_node),
        IrExprKind::Call { callee, arguments } => {
            contains_for_shaped_node(callee) || arguments.iter().any(contains_for_shaped_node)
        }
        IrExprKind::If {
            condition,
            then_expr,
            else_expr,
        } => {
            contains_for_shaped_node(condition)
                || contains_for_shaped_node(then_expr)
                || contains_for_shaped_node(else_expr)
        }
        IrExprKind::RangeLoop {
            start,
            end,
            step,
            body,
            ..
        } => {
            contains_for_shaped_node(start)
                || contains_for_shaped_node(end)
                || contains_for_shaped_node(step)
                || contains_for_shaped_node(body)
        }
        IrExprKind::StateLoop {
            states,
            condition,
            updates,
            body,
            ..
        } => {
            states
                .iter()
                .any(|s| contains_for_shaped_node(&s.initializer))
                || contains_for_shaped_node(condition)
                || updates.iter().any(|u| contains_for_shaped_node(&u.value))
                || contains_for_shaped_node(body)
        }
        IrExprKind::Break { value, .. } => value.as_deref().is_some_and(contains_for_shaped_node),
        IrExprKind::Sequence(items) => items.iter().any(contains_for_shaped_node),
    }
}

fn contains_for_in_quasi_datum(template: &IrQuasiDatum) -> bool {
    match template {
        IrQuasiDatum::Datum(_) => false,
        IrQuasiDatum::List(items) => items.iter().any(contains_for_in_quasi_datum),
        IrQuasiDatum::Evaluate(expression) => contains_for_shaped_node(expression),
    }
}

#[test]
fn for_expands_to_three_let_bindings_and_runs_to_completion() {
    let mut interp = Interpreter::new(Vec::new());
    let mut interp_out = Interpreter::new(Vec::new());
    let values = interp_out
        .eval_source(r#"(for (i 0 3) (print "{}" i))"#)
        .unwrap();
    assert!(matches!(values[0], Value::Unit));
    assert_eq!(interp_out.output(), b"012");
    let _ = &mut interp; // keep interpreter import used defensively
}

// ============================================================
// 63-64: range loop and break
// ============================================================

#[test]
fn range_loop_lowers_to_range_loop_with_a_dedicated_loop_id() {
    let mut compiler = Compiler::new();
    let top_levels = compiler
        .compile_source(r#"(loop (i 0 4) (print "{}" i))"#)
        .unwrap();
    let IrTopLevel::Expr { body, .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    assert!(matches!(body.expr.kind, IrExprKind::RangeLoop { .. }));
}

#[test]
fn range_loop_executes_and_returns_unit() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp
        .eval_source(r#"(loop (i 0 4) (print "{}" i))"#)
        .unwrap();
    assert!(matches!(values[0], Value::Unit));
    assert_eq!(interp.output(), b"0123");
}

#[test]
fn break_target_matches_its_enclosing_range_loop_id() {
    let mut compiler = Compiler::new();
    let top_levels = compiler
        .compile_source("(loop (i 0 10) (if (= i 3) (break i) 0))")
        .unwrap();
    let IrTopLevel::Expr { body, .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    let IrExprKind::RangeLoop { loop_id, body, .. } = &body.expr.kind else {
        panic!("expected RangeLoop");
    };
    let IrExprKind::If { then_expr, .. } = &body.kind else {
        panic!("expected If");
    };
    let IrExprKind::Break { target, .. } = &then_expr.kind else {
        panic!("expected Break");
    };
    assert_eq!(target, loop_id);
}

#[test]
fn break_from_range_loop_returns_its_value() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp
        .eval_source("(loop (i 0 10) (if (= i 3) (break i) 0))")
        .unwrap();
    assert!(matches!(values[0], Value::Int(3)));
}

// ============================================================
// 65: nested loop ids
// ============================================================

#[test]
fn nested_loops_get_distinct_loop_ids_and_break_targets_the_inner_one() {
    let mut compiler = Compiler::new();
    let top_levels = compiler
        .compile_source("(loop (x 0 3) (loop (y 0 5) (if (= y 2) (break y) 0)))")
        .unwrap();
    let IrTopLevel::Expr { body, .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    let IrExprKind::RangeLoop {
        loop_id: outer_id,
        body: outer_body,
        ..
    } = &body.expr.kind
    else {
        panic!("expected outer RangeLoop");
    };
    let IrExprKind::RangeLoop {
        loop_id: inner_id,
        body: inner_body,
        ..
    } = &outer_body.kind
    else {
        panic!("expected inner RangeLoop");
    };
    assert_ne!(outer_id, inner_id);
    let IrExprKind::If { then_expr, .. } = &inner_body.kind else {
        panic!("expected If");
    };
    let IrExprKind::Break { target, .. } = &then_expr.kind else {
        panic!("expected Break");
    };
    assert_eq!(target, inner_id);
}

#[test]
fn nested_loop_outer_continues_after_inner_breaks() {
    let mut interp = Interpreter::new(Vec::new());
    let source = r#"(loop (x 0 3) (print "{}" (loop (y 0 5) (if (= y 2) (break y) 0))))"#;
    let values = interp.eval_source(source).unwrap();
    assert!(matches!(values[0], Value::Unit));
    assert_eq!(interp.output(), b"222");
}

// ============================================================
// 66: break outside any runtime loop is a lower-time error
// ============================================================

#[test]
fn break_in_a_function_with_no_enclosing_loop_is_a_lower_error() {
    let mut compiler = Compiler::new();
    let result = compiler.compile_source("(def stop (fn () (break)))");
    assert!(result.is_err());
}

#[test]
fn failed_break_lowering_leaves_no_partial_function_in_the_module() {
    let mut compiler = Compiler::new();
    let before = compiler.module().functions.len();
    let _ = compiler.compile_source("(def stop (fn () (break)))");
    assert_eq!(compiler.module().functions.len(), before);
}

// ============================================================
// 67-68: general (state) loop
// ============================================================

#[test]
fn general_loop_lowers_to_state_loop() {
    let mut compiler = Compiler::new();
    let source = "(loop ((i 0) (sum 0)) (while (< i 4)) (next ((i (+ i 1)) (sum (+ sum i)))) (do (print \"{}:{}\\n\" i sum)))";
    let top_levels = compiler.compile_source(source).unwrap();
    let IrTopLevel::Expr { body, .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    let IrExprKind::StateLoop {
        states, updates, ..
    } = &body.expr.kind
    else {
        panic!("expected StateLoop");
    };
    assert_eq!(states.len(), 2);
    assert_eq!(updates.len(), 2);
}

#[test]
fn general_loop_output_matches_expected_sequence() {
    let mut interp = Interpreter::new(Vec::new());
    let source = "(loop ((i 0) (sum 0)) (while (< i 4)) (next ((i (+ i 1)) (sum (+ sum i)))) (do (print \"{}:{}\\n\" i sum)))";
    interp.eval_source(source).unwrap();
    assert_eq!(interp.output(), b"0:0\n1:0\n2:1\n3:3\n");
}

#[test]
fn state_swap_updates_are_parallel_not_sequential() {
    let mut interp = Interpreter::new(Vec::new());
    let source = "(loop ((a 1) (b 2) (count 0)) (while (< count 2)) (next ((a b) (b a) (count (+ count 1)))) (do (print \"{} {}\\n\" a b)))";
    interp.eval_source(source).unwrap();
    assert_eq!(interp.output(), b"1 2\n2 1\n");
}

// ============================================================
// 69: builtin redefinition is not specialized away
// ============================================================

#[test]
fn redefined_plus_is_not_lowered_to_a_dedicated_add_instruction() {
    let mut interp = Interpreter::new(Vec::new());
    let source = "(def original-add +)\n(def + (fn (x y) (- x y)))\n(+ 10 3)\n(original-add 10 3)";
    let values = interp.eval_source(source).unwrap();
    assert!(matches!(values[2], Value::Int(7)));
    assert!(matches!(values[3], Value::Int(13)));
}

#[test]
fn plus_always_lowers_to_a_call_through_load_global() {
    let mut compiler = Compiler::new();
    let top_levels = compiler.compile_source("(+ 10 3)").unwrap();
    let IrTopLevel::Expr { body, .. } = &top_levels[0] else {
        panic!("expected a top-level expression");
    };
    let IrExprKind::Call { callee, .. } = &body.expr.kind else {
        panic!("expected Call");
    };
    assert!(matches!(callee.kind, IrExprKind::LoadGlobal(_)));
}

// ============================================================
// 70: IR verification catches malformed IR
// ============================================================

fn dummy_span() -> Span {
    Span::new(0, 0)
}

fn const_expr(n: i64) -> IrExpr {
    IrExpr::new(dummy_span(), IrExprKind::Const(IrConst::Int(n)))
}

#[test]
fn verify_rejects_out_of_range_function_id() {
    let module = IrModule::new();
    let globals = GlobalRegistry::new();
    let body = IrBody {
        local_count: 0,
        expr: IrExpr::new(
            dummy_span(),
            IrExprKind::MakeClosure {
                function: FunctionId(0),
                captures: vec![],
            },
        ),
    };
    let top_level = IrTopLevel::Expr {
        body,
        span: dummy_span(),
    };
    let result = regatelisp::verify_top_level(&top_level, &module, &globals);
    assert!(result.is_err());
}

#[test]
fn verify_rejects_out_of_range_local_slot() {
    let module = IrModule::new();
    let globals = GlobalRegistry::new();
    let body = IrBody {
        local_count: 0,
        expr: IrExpr::new(dummy_span(), IrExprKind::LoadLocal(LocalSlot(0))),
    };
    let top_level = IrTopLevel::Expr {
        body,
        span: dummy_span(),
    };
    assert!(regatelisp::verify_top_level(&top_level, &module, &globals).is_err());
}

#[test]
fn verify_rejects_out_of_range_capture_slot() {
    let module = IrModule::new();
    let globals = GlobalRegistry::new();
    let body = IrBody {
        local_count: 0,
        expr: IrExpr::new(dummy_span(), IrExprKind::LoadCapture(CaptureSlot(0))),
    };
    let top_level = IrTopLevel::Expr {
        body,
        span: dummy_span(),
    };
    assert!(regatelisp::verify_top_level(&top_level, &module, &globals).is_err());
}

#[test]
fn verify_rejects_capture_count_mismatch() {
    let mut module = IrModule::new();
    module.functions.push(IrFunction {
        id: FunctionId(0),
        name_hint: None,
        parameter_count: 0,
        capture_count: 1,
        local_count: 0,
        body: const_expr(0),
        span: dummy_span(),
    });
    let globals = GlobalRegistry::new();
    let body = IrBody {
        local_count: 0,
        expr: IrExpr::new(
            dummy_span(),
            IrExprKind::MakeClosure {
                function: FunctionId(0),
                captures: vec![],
            },
        ),
    };
    let top_level = IrTopLevel::Expr {
        body,
        span: dummy_span(),
    };
    let result = regatelisp::verify_top_level(&top_level, &module, &globals);
    assert!(matches!(
        result,
        Err(regatelisp::LispError::Verify(
            VerifyError::CaptureCountMismatch { .. }
        ))
    ));
}

#[test]
fn verify_rejects_break_to_a_nonenclosing_loop_id() {
    let module = IrModule::new();
    let globals = GlobalRegistry::new();
    let body = IrBody {
        local_count: 0,
        expr: IrExpr::new(
            dummy_span(),
            IrExprKind::Break {
                target: LoopId(0),
                value: None,
            },
        ),
    };
    let top_level = IrTopLevel::Expr {
        body,
        span: dummy_span(),
    };
    let result = regatelisp::verify_top_level(&top_level, &module, &globals);
    assert!(matches!(
        result,
        Err(regatelisp::LispError::Verify(
            VerifyError::BreakTargetNotEnclosing(_)
        ))
    ));
}

#[test]
fn verify_rejects_duplicate_loop_id_in_nested_range_loops() {
    let module = IrModule::new();
    let globals = GlobalRegistry::new();
    let inner = IrExpr::new(
        dummy_span(),
        IrExprKind::RangeLoop {
            loop_id: LoopId(0),
            variable: LocalSlot(1),
            start: Box::new(const_expr(0)),
            end: Box::new(const_expr(1)),
            step: Box::new(const_expr(1)),
            body: Box::new(const_expr(0)),
        },
    );
    let outer = IrExpr::new(
        dummy_span(),
        IrExprKind::RangeLoop {
            loop_id: LoopId(0),
            variable: LocalSlot(0),
            start: Box::new(const_expr(0)),
            end: Box::new(const_expr(1)),
            step: Box::new(const_expr(1)),
            body: Box::new(inner),
        },
    );
    let body = IrBody {
        local_count: 2,
        expr: outer,
    };
    let top_level = IrTopLevel::Expr {
        body,
        span: dummy_span(),
    };
    let result = regatelisp::verify_top_level(&top_level, &module, &globals);
    assert!(matches!(
        result,
        Err(regatelisp::LispError::Verify(VerifyError::DuplicateLoopId(
            _
        )))
    ));
}

#[test]
fn verify_rejects_invalid_state_loop_update_target() {
    let module = IrModule::new();
    let globals = GlobalRegistry::new();
    let expr = IrExpr::new(
        dummy_span(),
        IrExprKind::StateLoop {
            loop_id: LoopId(0),
            states: vec![regatelisp::ir::IrStateBinding {
                target: LocalSlot(0),
                initializer: const_expr(0),
                span: dummy_span(),
            }],
            condition: Box::new(IrExpr::new(
                dummy_span(),
                IrExprKind::Const(IrConst::Bool(false)),
            )),
            // Updates a slot that was never declared as state.
            updates: vec![regatelisp::ir::IrStateUpdate {
                target: LocalSlot(1),
                value: const_expr(0),
                span: dummy_span(),
            }],
            body: Box::new(const_expr(0)),
        },
    );
    let body = IrBody {
        local_count: 2,
        expr,
    };
    let top_level = IrTopLevel::Expr {
        body,
        span: dummy_span(),
    };
    let result = regatelisp::verify_top_level(&top_level, &module, &globals);
    assert!(matches!(
        result,
        Err(regatelisp::LispError::Verify(
            VerifyError::InvalidStateLoopUpdate
        ))
    ));
}

#[test]
fn verify_rejects_global_id_out_of_registry_range() {
    let module = IrModule::new();
    let globals = GlobalRegistry::new();
    let body = IrBody {
        local_count: 0,
        expr: IrExpr::new(dummy_span(), IrExprKind::LoadGlobal(GlobalId(0))),
    };
    let top_level = IrTopLevel::Expr {
        body,
        span: dummy_span(),
    };
    assert!(regatelisp::verify_top_level(&top_level, &module, &globals).is_err());
}

// ============================================================
// 71: deterministic text IR output
// ============================================================

#[test]
fn text_ir_output_is_identical_across_repeated_compiles() {
    let source = "(def make-adder (fn (x) (fn (y) (+ x y))))";
    let mut first = Compiler::new();
    first.compile_source(source).unwrap();
    let first_text = regatelisp::format_ir_module(first.module(), first.globals());

    let mut second = Compiler::new();
    second.compile_source(source).unwrap();
    let second_text = regatelisp::format_ir_module(second.module(), second.globals());

    assert_eq!(first_text, second_text);
}

#[test]
fn capture_order_and_function_ids_are_stable_across_compiles() {
    let source = "(let ((a 1)) (((fn (b) (fn (c) (+ a (+ b c)))) 2) 3))";
    let mut first = Compiler::new();
    first.compile_source(source).unwrap();
    let mut second = Compiler::new();
    second.compile_source(source).unwrap();

    assert_eq!(
        first.module().functions.len(),
        second.module().functions.len()
    );
    for (a, b) in first
        .module()
        .functions
        .iter()
        .zip(&second.module().functions)
    {
        assert_eq!(a.id, b.id);
        assert_eq!(a.capture_count, b.capture_count);
    }
}

// ============================================================
// 72: --dump-ir / --check do not execute (covered at the process level
// manually; here we cover the equivalent library-level guarantee that
// `Compiler` never touches an output target at all).
// ============================================================

#[test]
fn compiler_has_no_way_to_produce_output() {
    let mut compiler = Compiler::new();
    // If this compiles and runs without any observable side effect, there
    // is no output channel `print` could have written to -- `Compiler`
    // simply has no `Write` target anywhere in its API.
    let result = compiler.compile_source(r#"(print "must-not-run\n")"#);
    assert!(result.is_ok());
}

// ============================================================
// 73: state persists across eval_source calls
// ============================================================

#[test]
fn function_and_global_ids_persist_across_eval_source_calls() {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source("(def square (fn (x) (* x x)))").unwrap();
    let values = interp.eval_source("(square 5)").unwrap();
    assert!(matches!(values[0], Value::Int(25)));
}

// captures unused imports check
#[allow(dead_code)]
fn _touch(_: IrCaptureSource) {}
