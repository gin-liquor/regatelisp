use regatelisp::error::{EvalError, ExpandError, VerifyError};
use regatelisp::ir::{IrBody, IrExpr, IrExprKind, IrModule, IrQuasiDatum, IrTopLevel};
use regatelisp::{
    Datum, Expr, ExprKind, GlobalRegistry, Interpreter, LispError, MacroExpandError,
    MacroExpansionLimits, MacroExpansionSession, Properties, PropertyValue, Value,
    compile_systemverilog, macro_expand, parse_program, verify_top_level,
};

fn eval(source: &str) -> Result<Vec<Value>, LispError> {
    Interpreter::new(Vec::new()).eval_source(source)
}

fn datum(source: &str) -> Datum {
    let values = eval(source).unwrap();
    let Value::Datum(value) = &values[0] else {
        panic!("expected datum")
    };
    value.as_ref().clone()
}

#[test]
fn splices_empty_single_multiple_and_several_positions() {
    assert_eq!(
        datum("(quasiquote ((unquote-splicing (quote (a))) b))").to_string(),
        "(a b)"
    );
    assert_eq!(
        datum("(quasiquote (a (unquote-splicing (quote (b c))) d))").to_string(),
        "(a b c d)"
    );
    assert_eq!(
        datum("(quasiquote (a (unquote-splicing (quote ())) b))").to_string(),
        "(a b)"
    );
    assert_eq!(
        datum(
            "(quasiquote ((unquote-splicing (quote (a b))) middle (unquote-splicing (quote (c d)))))"
        )
        .to_string(),
        "(a b middle c d)"
    );
}

#[test]
fn splice_obeys_quote_and_nested_quasiquote_depth() {
    assert_eq!(
        datum(
            "(let ((xs (quote (a b))))\n\
               (quasiquote\n\
                 (outer\n\
                   (quote (unquote-splicing xs))\n\
                   (quasiquote (inner (unquote-splicing xs)))\n\
                   (unquote-splicing xs))))"
        )
        .to_string(),
        "(outer (quote (unquote-splicing xs)) (quasiquote (inner (unquote-splicing xs))) a b)"
    );
}

#[test]
fn splice_rejects_invalid_placement_arity_and_values() {
    assert!(matches!(
        eval("(unquote-splicing xs)"),
        Err(LispError::Expand(
            ExpandError::UnquoteSplicingOutsideQuasiquote
        ))
    ));
    assert!(matches!(
        eval("(quasiquote (unquote-splicing (quote (a))))"),
        Err(LispError::Expand(
            ExpandError::UnquoteSplicingWithoutListContext
        ))
    ));
    assert!(matches!(
        eval("(quasiquote (a (unquote-splicing)))"),
        Err(LispError::Expand(
            ExpandError::InvalidUnquoteSplicingSyntax { got: 0 }
        ))
    ));
    assert!(matches!(
        eval("(quasiquote (a (unquote-splicing 1)))"),
        Err(LispError::Eval(EvalError::SpliceExpectedDatumList(
            "integer"
        )))
    ));
    assert!(matches!(
        eval("(quasiquote (a (unquote-splicing (quote symbol))))"),
        Err(LispError::Eval(EvalError::SpliceExpectedDatumList(_)))
    ));
    assert!(matches!(
        eval("(quasiquote (a (unquote-splicing (fn () 1))))"),
        Err(LispError::Eval(EvalError::SpliceExpectedDatumList(
            "function"
        )))
    ));
}

#[test]
fn rest_macros_support_zero_and_many_extra_arguments() {
    let values = eval(
        "(defmacro begin (&rest forms)\n\
           (quasiquote (do (unquote-splicing forms))))\n\
         (defmacro invoke (operator &rest operands) (cons operator operands))\n\
         (begin)\n\
         (begin 1 2 3)\n\
         (invoke + 20 22)",
    )
    .unwrap();
    assert_eq!(values, vec![Value::Unit, Value::Int(3), Value::Int(42)]);
}

#[test]
fn compiler_expansion_path_removes_rest_and_splice_syntax() {
    let mut compiler = regatelisp::compiler::Compiler::new();
    let expanded = compiler
        .expand_source(
            "(defmacro begin (&rest forms)\n\
               (quasiquote (do (unquote-splicing forms))))\n\
             (begin 1 2 3)",
        )
        .unwrap();
    let rendered = format!("{:?}", expanded[0]);
    assert!(rendered.contains("Symbol(\"do\")"));
    assert!(!rendered.contains("unquote-splicing"));
    assert!(!rendered.contains("defmacro"));
}

#[test]
fn rest_definition_and_minimum_arity_errors_are_structured() {
    for source in [
        "(defmacro bad (a &rest) (quote 0))",
        "(defmacro bad (a &rest xs y) (quote 0))",
        "(defmacro bad (a &rest xs &rest ys) (quote 0))",
        "(defmacro bad (a &rest (x y)) (quote 0))",
    ] {
        assert!(matches!(
            eval(source),
            Err(LispError::Macro(
                MacroExpandError::InvalidMacroRestParameter
            ))
        ));
    }
    assert!(matches!(
        eval("(defmacro bad (a &rest a) (quote 0))"),
        Err(LispError::Macro(MacroExpandError::DuplicateMacroParameter(name))) if name == "a"
    ));
    assert!(matches!(
        eval("(defmacro at-least-two (a b &rest xs) (quote 0)) (at-least-two 1)"),
        Err(LispError::Macro(
            MacroExpandError::MacroMinimumArityMismatch {
                minimum: 2,
                got: 1,
                ..
            }
        ))
    ));
}

#[test]
fn datum_list_primitives_cover_construction_access_and_predicates() {
    let values = eval(
        "(list)\n\
         (list (quote a) 1 true \"s\")\n\
         (cons (quote a) (quote (b c)))\n\
         (car (quote (a b)))\n\
         (cdr (quote (a b)))\n\
         (append (quote (a)) (quote ()) (quote (b c)))\n\
         (null? (quote ()))\n\
         (pair? (quote (a)))\n\
         (list? (quote atom))",
    )
    .unwrap();
    let rendered: Vec<String> = values.iter().map(ToString::to_string).collect();
    assert_eq!(
        rendered,
        [
            "()",
            "(a 1 true \"s\")",
            "(a b c)",
            "a",
            "(b)",
            "(a b c)",
            "true",
            "true",
            "false",
        ]
    );
}

#[test]
fn datum_list_primitives_reject_bad_arity_types_and_empty_lists() {
    assert!(matches!(
        eval("(cons (quote a) (quote b))"),
        Err(LispError::Eval(EvalError::DatumPrimitiveTypeMismatch {
            primitive: "cons",
            ..
        }))
    ));
    assert!(matches!(
        eval("(car (quote ()))"),
        Err(LispError::Eval(EvalError::DatumPrimitiveEmptyList("car")))
    ));
    assert!(matches!(
        eval("(cdr (quote ()))"),
        Err(LispError::Eval(EvalError::DatumPrimitiveEmptyList("cdr")))
    ));
    assert!(matches!(
        eval("(append (quote (a)) 1)"),
        Err(LispError::Eval(EvalError::DatumPrimitiveTypeMismatch {
            primitive: "append",
            argument: 1,
            ..
        }))
    ));
    assert!(matches!(
        eval("(car)"),
        Err(LispError::Eval(EvalError::WrongArgCount {
            expected: 1,
            got: 0
        }))
    ));
    assert!(matches!(
        eval("(list (fn () 1))"),
        Err(LispError::Eval(EvalError::DatumPrimitiveTypeMismatch {
            primitive: "list",
            ..
        }))
    ));
}

#[test]
fn gensym_identity_survives_list_primitives_and_splicing() {
    let values = eval(
        "(defmacro hygienic (value)\n\
           (let ((temporary (gensym (quote temporary))))\n\
             (quasiquote\n\
               (let (((unquote temporary) (unquote value)))\n\
                 (unquote (car (list temporary)))))))\n\
         (let ((temporary 100)) (hygienic 42))",
    )
    .unwrap();
    assert_eq!(values, vec![Value::Int(42)]);
}

#[test]
fn rest_splicing_preserves_argument_properties() {
    let definition = regatelisp::parse_one(
        "(defmacro collect (&rest values) (quasiquote (list (unquote-splicing values))))",
    )
    .unwrap();
    let first = Expr::with_properties(
        ExprKind::Int(1),
        Properties::new().with("source", PropertyValue::String("first".into())),
    );
    let second = Expr::with_properties(
        ExprKind::Int(2),
        Properties::new().with("source", PropertyValue::String("second".into())),
    );
    let call = Expr::list(vec![Expr::symbol("collect"), first, second]);
    let (expanded, _) = macro_expand::expand_program(&[definition, call], 0).unwrap();
    let ExprKind::List(items) = expanded[0].kind() else {
        panic!("expected list call")
    };
    assert_eq!(
        items[1].get_property("source"),
        Some(&PropertyValue::String("first".into()))
    );
    assert_eq!(
        items[2].get_property("source"),
        Some(&PropertyValue::String("second".into()))
    );
}

#[test]
fn verifier_rejects_a_handmade_root_splice() {
    let top = IrTopLevel::Expr {
        body: IrBody {
            local_count: 0,
            expr: IrExpr::new(
                regatelisp::error::Span::new(0, 0),
                IrExprKind::QuasiQuote(IrQuasiDatum::Splice(Box::new(IrExpr::new(
                    regatelisp::error::Span::new(0, 0),
                    IrExprKind::Quote(Datum::List(Vec::new())),
                )))),
            ),
        },
        span: regatelisp::error::Span::new(0, 0),
    };
    assert!(matches!(
        verify_top_level(&top, &IrModule::new(), &GlobalRegistry::new()),
        Err(LispError::Verify(
            VerifyError::InvalidQuasiquoteSplicePlacement
        ))
    ));
}

#[test]
fn generated_datum_node_budget_is_enforced() {
    let expressions = parse_program("(defmacro many () (quote (a b c d))) (many)").unwrap();
    let mut session = MacroExpansionSession::with_limits(
        0,
        MacroExpansionLimits {
            max_depth: 10,
            max_invocations: 10,
            max_generated_datum_nodes: 4,
        },
    );
    assert!(matches!(
        session.expand_program(&expressions),
        Err(MacroExpandError::GeneratedDatumNodeLimitExceeded { limit: 4, .. })
    ));
}

#[test]
fn hardware_macro_splices_multiple_clocked_statements_and_keeps_verification() {
    let source = "(defmacro clocked-rising (clk &rest statements)\n\
           (quasiquote\n\
             (clocked (clock (unquote clk) rising)\n\
               (unquote-splicing statements))))\n\
         (module counter\n\
           (ports\n\
             (input (meta ((width 1)) clk))\n\
             (output (meta ((width 8)) count))\n\
             (output (meta ((width 1)) pulse)))\n\
           (register count)\n\
           (register pulse)\n\
           (clocked-rising clk\n\
             (set count (+ count (meta ((width 8)) 1)))\n\
             (set pulse (= count (meta ((width 8)) 255)))))";
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("count <= (count + 8'd1);"));
    assert!(output.contains("pulse <= (count == 8'd255);"));

    let invalid = source.replace(
        "(set pulse (= count (meta ((width 8)) 255)))",
        "(set count (meta ((width 8)) 0))",
    );
    assert!(compile_systemverilog(&invalid).is_err());
}
