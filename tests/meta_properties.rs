use regatelisp::compiler::Compiler;
use regatelisp::{ExpandError, Interpreter, LispError, PropertyValue, Value};

fn expand(source: &str) -> regatelisp::CoreExpr {
    let mut compiler = Compiler::new();
    compiler
        .expand_only(&regatelisp::parse_one(source).unwrap())
        .unwrap()
}

#[test]
fn meta_attaches_multiple_properties_without_affecting_evaluation() {
    let source = "(meta ((width 8) (target vhdl) (signed false)) (+ 1 2))";
    let core = expand(source);
    assert_eq!(core.property("width"), Some(&PropertyValue::Int(8)));
    assert_eq!(
        core.property("target"),
        Some(&PropertyValue::Symbol("vhdl".into()))
    );
    assert_eq!(core.property("signed"), Some(&PropertyValue::Bool(false)));

    let mut interpreter = Interpreter::new(Vec::new());
    assert_eq!(
        interpreter.eval_source(source).unwrap(),
        vec![Value::Int(3)]
    );
}

#[test]
fn meta_accepts_unknown_and_recursive_inert_values() {
    let core = expand(
        "(meta ((anything-you-like 123) (clock (clk rising)) (options ((keep true) (depth 4)))) 1)",
    );
    assert_eq!(
        core.property("anything-you-like"),
        Some(&PropertyValue::Int(123))
    );
    assert_eq!(
        core.property("clock"),
        Some(&PropertyValue::List(vec![
            PropertyValue::Symbol("clk".into()),
            PropertyValue::Symbol("rising".into()),
        ]))
    );
    assert_eq!(
        core.property("options"),
        Some(&PropertyValue::List(vec![
            PropertyValue::List(vec![
                PropertyValue::Symbol("keep".into()),
                PropertyValue::Bool(true),
            ]),
            PropertyValue::List(vec![
                PropertyValue::Symbol("depth".into()),
                PropertyValue::Int(4),
            ]),
        ]))
    );
}

#[test]
fn nested_meta_gives_inner_values_precedence() {
    let core = expand("(meta ((width 8) (target vhdl)) (meta ((width 16) (signed true)) 1))");
    assert_eq!(core.property("width"), Some(&PropertyValue::Int(16)));
    assert_eq!(
        core.property("target"),
        Some(&PropertyValue::Symbol("vhdl".into()))
    );
    assert_eq!(core.property("signed"), Some(&PropertyValue::Bool(true)));
}

#[test]
fn empty_metadata_is_a_no_op() {
    let mut interpreter = Interpreter::new(Vec::new());
    assert_eq!(
        interpreter.eval_source("(meta () 123)").unwrap(),
        vec![Value::Int(123)]
    );
}

#[test]
fn malformed_meta_forms_are_structured_expand_errors() {
    let cases = [
        ("(meta)", ExpandError::InvalidMetaSyntax),
        ("(meta ((width 8)))", ExpandError::InvalidMetaSyntax),
        ("(meta ((width 8)) x y)", ExpandError::InvalidMetaSyntax),
        ("(meta width x)", ExpandError::MetaPropertiesNotList),
        ("(meta (width) x)", ExpandError::InvalidMetaProperty),
        ("(meta ((width)) x)", ExpandError::InvalidMetaProperty),
        ("(meta ((width 8 16)) x)", ExpandError::InvalidMetaProperty),
        ("(meta ((123 8)) x)", ExpandError::InvalidMetaPropertyKey),
    ];
    for (source, expected) in cases {
        let mut compiler = Compiler::new();
        let expression = regatelisp::parse_one(source).unwrap();
        let Err(LispError::Expand(actual)) = compiler.expand_only(&expression) else {
            panic!("expected expand error for {source}");
        };
        assert_eq!(actual, expected);
    }

    let mut compiler = Compiler::new();
    let expression = regatelisp::parse_one("(meta ((width 8) (width 16)) x)").unwrap();
    let Err(LispError::Expand(actual)) = compiler.expand_only(&expression) else {
        panic!("expected duplicate property error");
    };
    assert_eq!(actual, ExpandError::DuplicatePropertyKey("width".into()));
}

#[test]
fn display_uses_deterministic_meta_s_expressions() {
    let expression = regatelisp::parse_one("x")
        .unwrap()
        .with_property("width", PropertyValue::Int(8))
        .with_property("target", PropertyValue::Symbol("vhdl".into()));
    assert_eq!(expression.to_string(), "(meta ((target vhdl) (width 8)) x)");
    assert_eq!(regatelisp::parse_one("x").unwrap().to_string(), "x");
}
