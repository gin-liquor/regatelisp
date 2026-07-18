//! Integration tests for `for` (expansion-time), `loop` (runtime), and
//! `def` (global bindings), driven through the public `Interpreter` API.

use regatelisp::{Interpreter, Value};

fn output_of(source: &str) -> String {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source(source).expect("eval should succeed");
    String::from_utf8(interp.into_output()).expect("output should be valid utf-8")
}

fn eval_err(source: &str) -> bool {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source(source).is_err()
}

fn last_int(source: &str) -> i64 {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp.eval_source(source).expect("eval should succeed");
    match values.last() {
        Some(Value::Int(n)) => *n,
        other => panic!("expected trailing Int, got {other:?}"),
    }
}

// ============================================================
// for
// ============================================================

#[test]
fn for_basic() {
    assert_eq!(output_of(r#"(for (i 0 4) (print "{}" i))"#), "0123");
}

#[test]
fn for_returns_unit() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp
        .eval_source("(for (i 0 4) (print \"{}\" i))")
        .unwrap();
    assert!(matches!(values[0], Value::Unit));
}

#[test]
fn for_step() {
    assert_eq!(output_of(r#"(for (i 0 8 2) (print "{}" i))"#), "0246");
}

#[test]
fn for_negative_step() {
    assert_eq!(output_of(r#"(for (i 4 0 -1) (print "{}" i))"#), "4321");
}

#[test]
fn for_zero_iterations() {
    assert_eq!(output_of(r#"(for (i 4 0) (print "never"))"#), "");
}

#[test]
fn for_constant_expression_bound() {
    assert_eq!(output_of(r#"(for (i 0 (+ 2 2)) (print "{}" i))"#), "0123");
}

#[test]
fn for_global_constant_bound() {
    assert_eq!(
        output_of("(def count 3)\n(for (i 0 count) (print \"{}\" i))"),
        "012"
    );
}

#[test]
fn for_nested() {
    assert_eq!(
        output_of("(for (x 0 3) (for (y 0 (+ x 1)) (print \"{}{}\\n\" x y)))"),
        "00\n10\n11\n20\n21\n22\n"
    );
}

#[test]
fn for_shadows_outer_binding() {
    assert_eq!(
        output_of(r#"(let ((i 100)) (for (i 0 3) (print "{}" i)))"#),
        "012"
    );
}

#[test]
fn for_expansion_limit_exceeded_produces_no_output() {
    // A single top-level `for` requesting far more than the default
    // expansion budget (65_536) must fail during expansion, before any
    // `print` runs -- so no partial output and no partial Core AST.
    let mut interp = Interpreter::new(Vec::new());
    let result = interp.eval_source(r#"(for (i 0 1000000) (print "{}" i))"#);
    assert!(result.is_err());
    assert!(interp.output().is_empty());
}

// for syntax errors

#[test]
fn for_missing_binding_is_error() {
    assert!(eval_err("(for () body)"));
}

#[test]
fn for_non_symbol_variable_is_error() {
    assert!(eval_err("(for (10 0 4) body)"));
}

#[test]
fn for_missing_end_is_error() {
    assert!(eval_err("(for (i 0) body)"));
}

#[test]
fn for_too_many_binding_elements_is_error() {
    assert!(eval_err("(for (i 0 10 1 2) body)"));
}

#[test]
fn for_missing_body_is_error() {
    assert!(eval_err("(for (i 0 4))"));
}

#[test]
fn for_multiple_bodies_is_error() {
    assert!(eval_err("(for (i 0 4) 1 2)"));
}

#[test]
fn for_runtime_bound_is_error() {
    assert!(eval_err("(fn (n) (for (i 0 n) i))"));
}

#[test]
fn for_zero_step_is_error() {
    assert!(eval_err("(for (i 0 10 0) body)"));
}

// ============================================================
// loop
// ============================================================

#[test]
fn loop_basic() {
    assert_eq!(output_of(r#"(loop (i 0 4) (print "{}" i))"#), "0123");
}

#[test]
fn loop_runtime_bound_from_function_argument() {
    assert_eq!(
        output_of(r#"((fn (count) (loop (i 0 count) (print "{}" i))) 4)"#),
        "0123"
    );
}

#[test]
fn loop_step() {
    assert_eq!(output_of(r#"(loop (i 0 8 2) (print "{}" i))"#), "0246");
}

#[test]
fn loop_negative_step() {
    assert_eq!(output_of(r#"(loop (i 4 0 -1) (print "{}" i))"#), "4321");
}

#[test]
fn loop_forward_out_of_range_is_empty() {
    assert_eq!(output_of(r#"(loop (i 4 0 1) (print "never"))"#), "");
}

#[test]
fn loop_backward_out_of_range_is_empty() {
    assert_eq!(output_of(r#"(loop (i 0 4 -1) (print "never"))"#), "");
}

#[test]
fn loop_local_value_bound() {
    assert_eq!(
        output_of(r#"(let ((count 3)) (loop (i 0 count) (print "{}" i)))"#),
        "012"
    );
}

#[test]
fn loop_shadows_outer_binding() {
    assert_eq!(
        output_of(r#"(let ((i 100)) (loop (i 0 3) (print "{}" i)))"#),
        "012"
    );
}

#[test]
fn loop_returns_unit() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp
        .eval_source("(loop (i 0 0) (print \"never\"))")
        .unwrap();
    assert!(matches!(values[0], Value::Unit));
    assert!(interp.output().is_empty());
}

#[test]
fn loop_zero_step_is_error() {
    assert!(eval_err(r#"(loop (i 0 4 0) (print "{}" i))"#));
}

#[test]
fn loop_non_integer_bound_is_error() {
    assert!(eval_err(r#"(loop (i "0" 4) (print "{}" i))"#));
}

#[test]
fn loop_counter_overflow_is_error() {
    let source = format!(
        "(loop (i {} {} 2) (print \"{{}}\" i))",
        i64::MAX - 1,
        i64::MAX
    );
    assert!(eval_err(&source));
}

// loop syntax errors

#[test]
fn loop_missing_binding_is_error() {
    assert!(eval_err("(loop () body)"));
}

#[test]
fn loop_non_symbol_variable_is_error() {
    assert!(eval_err("(loop (10 0 4) body)"));
}

#[test]
fn loop_missing_end_is_error() {
    assert!(eval_err("(loop (i 0) body)"));
}

#[test]
fn loop_too_many_binding_elements_is_error() {
    assert!(eval_err("(loop (i 0 4 1 2) body)"));
}

#[test]
fn loop_missing_body_is_error() {
    assert!(eval_err("(loop (i 0 4))"));
}

#[test]
fn loop_multiple_bodies_is_error() {
    assert!(eval_err("(loop (i 0 4) 1 2)"));
}

// ============================================================
// def
// ============================================================

#[test]
fn def_integer() {
    assert_eq!(last_int("(def answer 42)\nanswer"), 42);
}

#[test]
fn def_function() {
    assert_eq!(last_int("(def square (fn (x) (* x x)))\n(square 5)"), 25);
}

#[test]
fn def_visible_to_later_top_level_forms_in_same_source() {
    assert_eq!(last_int("(def x 10)\n(def y (+ x 5))\ny"), 15);
}

#[test]
fn def_visible_across_separate_eval_source_calls() {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source("(def x 10)").unwrap();
    let values = interp.eval_source("x").unwrap();
    assert!(matches!(values[0], Value::Int(10)));
}

#[test]
fn def_redefinition() {
    assert_eq!(last_int("(def x 10)\n(def x 20)\nx"), 20);
}

#[test]
fn def_local_shadowing_does_not_affect_global() {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source("(def x 10)").unwrap();
    let values = interp.eval_source("(let ((x 20)) x)").unwrap();
    assert!(matches!(values[0], Value::Int(20)));
    let values = interp.eval_source("x").unwrap();
    assert!(matches!(values[0], Value::Int(10)));
}

#[test]
fn def_closure_sees_latest_global_value() {
    assert_eq!(
        last_int("(def x 10)\n(def read-x (fn () x))\n(def x 20)\n(read-x)"),
        20
    );
}

#[test]
fn def_closure_keeps_local_value() {
    assert_eq!(
        last_int("(def read-x (let ((x 10)) (fn () x)))\n(def x 20)\n(read-x)"),
        10
    );
}

#[test]
fn def_failed_definition_is_atomic() {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source("(def x 10)").unwrap();
    let result = interp.eval_source("(def x unknown-name)");
    assert!(result.is_err());
    let values = interp.eval_source("x").unwrap();
    assert!(matches!(values[0], Value::Int(10)));
}

#[test]
fn def_can_redefine_builtins() {
    assert_eq!(
        last_int("(def add +)\n(def + (fn (x y) (- x y)))\n(+ 10 3)"),
        7
    );
    assert_eq!(last_int("(def add +)\n(add 10 3)"), 13);
}

#[test]
fn def_self_referential_function_definition_succeeds() {
    let mut interp = Interpreter::new(Vec::new());
    let result = interp.eval_source("(def recurse (fn (x) (recurse x)))");
    assert!(result.is_ok());
}

// def syntax / scoping errors

#[test]
fn def_no_arguments_is_error() {
    assert!(eval_err("(def)"));
}

#[test]
fn def_name_only_is_error() {
    assert!(eval_err("(def x)"));
}

#[test]
fn def_too_many_arguments_is_error() {
    assert!(eval_err("(def x 1 2)"));
}

#[test]
fn def_non_symbol_name_is_error() {
    assert!(eval_err("(def 10 20)"));
}

#[test]
fn def_reserved_name_is_error() {
    for name in ["fn", "let", "for", "loop", "def"] {
        let source = format!("(def {name} 10)");
        assert!(eval_err(&source), "expected error defining '{name}'");
    }
}

#[test]
fn def_inside_let_is_error() {
    assert!(eval_err("(let ((x 1)) (def y 2))"));
}

#[test]
fn def_inside_fn_call_is_error() {
    assert!(eval_err("((fn () (def x 10)))"));
}

#[test]
fn def_inside_loop_is_error() {
    assert!(eval_err("(loop (i 0 4) (def x i))"));
}

// ============================================================
// for vs loop
// ============================================================

#[test]
fn for_captures_definition_time_global_value() {
    let source = concat!(
        "(def count 3)\n",
        "(def show-for (fn () (for (i 0 count) (print \"{}\" i))))\n",
        "(def count 5)\n",
        "(show-for)\n",
    );
    assert_eq!(output_of(source), "012");
}

#[test]
fn loop_uses_current_runtime_global_value() {
    let source = concat!(
        "(def count 3)\n",
        "(def show-loop (fn () (loop (i 0 count) (print \"{}\" i))))\n",
        "(def count 5)\n",
        "(show-loop)\n",
    );
    assert_eq!(output_of(source), "01234");
}
