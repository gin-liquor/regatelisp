//! Evaluates the sample programs under `examples/` through the public
//! library API and checks their results, so the example files stay in sync
//! with the language the interpreter actually implements.

use regatelisp::{Interpreter, Value};

fn eval_example(path: &str) -> Vec<Value> {
    let source = std::fs::read_to_string(path).expect("example file should exist");
    let mut interp = Interpreter::new(Vec::new());
    interp
        .eval_source(&source)
        .expect("example should evaluate without error")
}

fn eval_example_with_output(path: &str) -> (Vec<Value>, String) {
    let source = std::fs::read_to_string(path).expect("example file should exist");
    let mut interp = Interpreter::new(Vec::new());
    let values = interp
        .eval_source(&source)
        .expect("example should evaluate without error");
    let output = String::from_utf8(interp.into_output()).expect("output should be valid utf-8");
    (values, output)
}

fn as_ints(values: &[Value]) -> Vec<i64> {
    values
        .iter()
        .map(|v| match v {
            Value::Int(n) => *n,
            other => panic!("expected Int, got {other}"),
        })
        .collect()
}

#[test]
fn arithmetic_example() {
    let values = eval_example("examples/arithmetic.lisp");
    assert_eq!(as_ints(&values), vec![3, 7, 42, 3, 7]);
}

#[test]
fn let_and_shadowing_example() {
    let values = eval_example("examples/let_and_shadowing.lisp");
    assert_eq!(as_ints(&values), vec![30, 2, 10, 7]);
}

#[test]
fn closures_example() {
    let values = eval_example("examples/closures.lisp");
    assert_eq!(as_ints(&values), vec![6, 14, 123, 11, 36]);
}

#[test]
fn formatting_example() {
    let (_values, output) = eval_example_with_output("examples/formatting.lisp");
    assert_eq!(
        output,
        concat!(
            "hello\n",
            "x=10 y=20\n",
            "20 10\n",
            "dec=255 hex=0xff bin=0b11111111\n",
            "|123     |     123|  123   |\n",
            "|*****123|\n",
            "+10 -10\n",
        )
    );
}

#[test]
fn macros_example() {
    let values = eval_example("examples/macros.lisp");
    assert_eq!(as_ints(&values), vec![42]);
}

#[test]
fn hardware_macros_example() {
    let source = std::fs::read_to_string("examples/macros_sv.lisp").unwrap();
    let output = regatelisp::compile_systemverilog(&source).unwrap();
    assert!(output.contains("module macro_adder"));
    assert!(output.contains("assign y = (a + 8'd1);"));
}
