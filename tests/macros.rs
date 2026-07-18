use regatelisp::{
    Expr, ExprKind, GensymId, Interpreter, LispError, MacroExpandError, Properties, PropertyValue,
    Symbol, compile_systemverilog, macro_expand,
};

fn eval(source: &str) -> Vec<regatelisp::Value> {
    Interpreter::new(Vec::new()).eval_source(source).unwrap()
}

#[test]
fn macro_calls_may_precede_their_definitions_and_arguments_are_unevaluated() {
    let values = eval(
        "(ignore unknown-name)\n\
         (defmacro ignore (unused) (quote 42))",
    );
    assert_eq!(values, vec![regatelisp::Value::Int(42)]);
}

#[test]
fn recursively_expands_macros_returned_by_macros() {
    let values = eval(
        "(defmacro inc (x) (quasiquote (+ (unquote x) 1)))\n\
         (defmacro inc-twice (x) (quasiquote (inc (inc (unquote x)))))\n\
         (inc-twice 5)",
    );
    assert_eq!(values, vec![regatelisp::Value::Int(7)]);
}

#[test]
fn gensym_bindings_are_hygienic_and_runtime_numbering_continues() {
    let values = eval(
        "(defmacro twice (x)\n\
           (let ((temporary (gensym (quote temporary))))\n\
             (quasiquote\n\
               (let (((unquote temporary) (unquote x)))\n\
                 (+ (unquote temporary) (unquote temporary))))))\n\
         (let ((temporary 100)) (twice 4))\n\
         (gensym (quote temporary))",
    );
    assert_eq!(values[0], regatelisp::Value::Int(8));
    assert_eq!(
        values[1],
        regatelisp::Value::Datum(std::rc::Rc::new(regatelisp::Datum::Symbol(
            Symbol::generated(GensymId(1), Some("temporary".into()))
        )))
    );
}

#[test]
fn quote_protects_macro_calls() {
    let values = eval("(defmacro one () (quote 1)) (quote (one))");
    assert_eq!(values[0].to_string(), "(one)");
}

#[test]
fn rejects_invalid_definitions_and_arity() {
    let error = Interpreter::new(Vec::new())
        .eval_source("(defmacro bad (x x) (quote 0))")
        .unwrap_err();
    assert!(matches!(
        error,
        LispError::Macro(MacroExpandError::DuplicateMacroParameter(name)) if name == "x"
    ));

    let error = Interpreter::new(Vec::new())
        .eval_source("(defmacro one (x) (quote 1)) (one)")
        .unwrap_err();
    assert!(matches!(
        error,
        LispError::Macro(MacroExpandError::MacroArityMismatch { .. })
    ));
}

#[test]
fn macro_parameter_bindings_do_not_leak_between_invocations() {
    let error = Interpreter::new(Vec::new())
        .eval_source(
            "(defmacro bind-x (x) (quote 0))\n\
             (defmacro read-x () x)\n\
             (bind-x 1)\n\
             (read-x)",
        )
        .unwrap_err();
    assert!(matches!(
        error,
        LispError::Macro(MacroExpandError::MacroEvaluationFailed { name, .. }) if name == "read-x"
    ));
}

#[test]
fn compile_time_environment_cannot_reach_output_even_through_an_alias() {
    let error = Interpreter::new(Vec::new())
        .eval_source(
            "(defmacro noisy ()\n\
               (let ((write print)) (write \"not visible\")))\n\
             (noisy)",
        )
        .unwrap_err();
    assert!(matches!(
        error,
        LispError::Macro(MacroExpandError::MacroEvaluationFailed { .. })
    ));
}

#[test]
fn rejects_nested_and_generated_macro_definitions() {
    let nested = Interpreter::new(Vec::new())
        .eval_source("(if true (defmacro bad () (quote 0)) 0)")
        .unwrap_err();
    assert!(matches!(
        nested,
        LispError::Macro(MacroExpandError::NestedMacroDefinition)
    ));

    let generated = Interpreter::new(Vec::new())
        .eval_source("(defmacro bad () (quasiquote (defmacro generated () (quote 0)))) (bad)")
        .unwrap_err();
    assert!(matches!(
        generated,
        LispError::Macro(MacroExpandError::GeneratedMacroDefinition)
    ));
}

#[test]
fn systemverilog_entry_expands_macros_before_hardware_lowering() {
    let output = compile_systemverilog(
        "(defmacro increment (value)\n\
           (quasiquote (+ (unquote value) (meta ((width 8)) 1))))\n\
         (module adder\n\
           (ports\n\
             (input (meta ((width 8)) a))\n\
             (output (meta ((width 8)) y)))\n\
           (assign y (increment a)))",
    )
    .unwrap();
    assert!(output.contains("assign y = (a + 8'd1);"));
}

#[test]
fn macros_generate_clocked_statements_and_still_use_hardware_verification() {
    let output = compile_systemverilog(
        "(defmacro bump ()\n\
           (quasiquote (set count (+ count (meta ((width 8)) 1)))))\n\
         (module counter\n\
           (ports\n\
             (input (meta ((width 1)) clk))\n\
             (output (meta ((width 8)) count)))\n\
           (register count)\n\
           (clocked (clock clk rising) (bump)))",
    )
    .unwrap();
    assert!(output.contains("always_ff @(posedge clk)"));
    assert!(output.contains("count <= (count + 8'd1);"));

    let error = compile_systemverilog(
        "(defmacro bad-value () (quasiquote (meta ((width 4)) 1)))\n\
         (module invalid\n\
           (ports (output (meta ((width 8)) y)))\n\
           (assign y (bad-value)))",
    )
    .unwrap_err();
    assert!(matches!(
        error,
        regatelisp::hardware::HardwareError::TypeMismatch
    ));
}

#[test]
fn argument_template_and_call_root_properties_are_preserved() {
    let definition =
        regatelisp::parse_one("(defmacro identity (value) (quasiquote (unquote value)))").unwrap();
    let argument = Expr::with_properties(
        ExprKind::Int(7),
        Properties::new().with("argument", PropertyValue::Bool(true)),
    );
    let invocation = Expr::with_properties(
        ExprKind::List(vec![Expr::symbol("identity"), argument]),
        Properties::new().with("call", PropertyValue::Bool(true)),
    );
    let (expanded, _) = macro_expand::expand_program(&[definition, invocation], 0).unwrap();
    assert_eq!(
        expanded[0].get_property("argument"),
        Some(&PropertyValue::Bool(true))
    );
    assert_eq!(
        expanded[0].get_property("call"),
        Some(&PropertyValue::Bool(true))
    );
}

#[test]
fn quasiquote_template_properties_are_preserved() {
    let template = Expr::with_properties(
        ExprKind::Int(9),
        Properties::new().with("template", PropertyValue::Bool(true)),
    );
    let definition = Expr::list(vec![
        Expr::symbol("defmacro"),
        Expr::symbol("nine"),
        Expr::list(Vec::new()),
        Expr::list(vec![Expr::symbol("quasiquote"), template]),
    ]);
    let invocation = Expr::list(vec![Expr::symbol("nine")]);
    let (expanded, _) = macro_expand::expand_program(&[definition, invocation], 0).unwrap();
    assert_eq!(
        expanded[0].get_property("template"),
        Some(&PropertyValue::Bool(true))
    );
}
