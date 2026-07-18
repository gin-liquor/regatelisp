use regatelisp::error::{EvalError, ExpandError, LispError, LowerError, Span};
use regatelisp::hardware::HardwareError;
use regatelisp::ir::{IrBody, IrExpr, IrExprKind, IrModule, IrQuasiDatum, IrTopLevel};
use regatelisp::property::PropertyValue;
use regatelisp::symbol::{GensymId, Symbol};
use regatelisp::{
    Datum, Expr, ExprKind, GlobalRegistry, Interpreter, Value, compile_systemverilog,
    datum_to_expr, expr_to_datum, parse_one,
};

fn eval_one(source: &str) -> Value {
    Interpreter::new(Vec::new())
        .eval_source(source)
        .unwrap()
        .pop()
        .unwrap()
}

fn datum(source: &str) -> Datum {
    let Value::Datum(value) = eval_one(source) else {
        panic!("expected datum result");
    };
    value.as_ref().clone()
}

#[test]
fn quote_returns_atoms_lists_and_unevaluated_symbols() {
    assert_eq!(datum("(quote abc)"), Datum::Symbol(Symbol::interned("abc")));
    assert_eq!(datum("(quote 42)"), Datum::Integer(42));
    assert_eq!(datum("(quote true)"), Datum::Bool(true));
    assert_eq!(datum("(quote ())"), Datum::List(vec![]));
    assert_eq!(datum("(quote (a (b 1)))").to_string(), "(a (b 1))");
    assert_eq!(
        datum("(quote (unquote missing))").to_string(),
        "(unquote missing)"
    );
    assert_eq!(datum("(quote missing)").to_string(), "missing");
}

#[test]
fn quasiquote_evaluates_matching_unquotes_without_splicing() {
    assert_eq!(
        datum("(let ((x 10)) (quasiquote (+ (unquote x) 2)))").to_string(),
        "(+ 10 2)"
    );
    assert_eq!(
        datum("(let ((x false)) (quasiquote (value (unquote x))))").to_string(),
        "(value false)"
    );
    assert_eq!(
        datum("(let ((x (quote (a b)))) (quasiquote (head (unquote x) tail)))").to_string(),
        "(head (a b) tail)"
    );
    assert_eq!(
        datum("(let ((x 7)) (quasiquote (a (b (unquote x)))))").to_string(),
        "(a (b 7))"
    );
}

#[test]
fn quasiquote_tracks_depth_and_treats_quote_as_opaque() {
    assert_eq!(
        datum("(let ((x 10)) (quasiquote (outer (quasiquote (inner (unquote x))))))").to_string(),
        "(outer (quasiquote (inner (unquote x))))"
    );
    assert_eq!(
        datum("(let ((x 10)) (quasiquote (quote (unquote x))))").to_string(),
        "(quote (unquote x))"
    );
}

#[test]
fn multiple_unquotes_are_evaluated_from_left_to_right() {
    assert_eq!(
        datum("(quasiquote ((unquote (gensym)) (unquote (gensym))))").to_string(),
        "(g__g0 g__g1)"
    );
}

#[test]
fn quote_unquote_and_gensym_report_specific_syntax_and_type_errors() {
    let cases = [
        ("(quote)", "quote expects exactly 1 argument"),
        ("(quote a b)", "quote expects exactly 1 argument"),
        ("(quasiquote)", "quasiquote expects exactly 1 argument"),
        ("(unquote x)", "unquote may only be used inside quasiquote"),
        ("(unquote)", "unquote expects exactly 1 argument"),
        ("(unquote a b)", "unquote expects exactly 1 argument"),
        ("(gensym a b)", "gensym expects 0 or 1 arguments"),
        (
            "(gensym 123)",
            "gensym prefix must be an interned symbol datum",
        ),
        (
            "(gensym (quote (a b)))",
            "gensym prefix must be an interned symbol datum",
        ),
        (
            "(quasiquote ((unquote (fn () 1))))",
            "unquote result cannot be converted to datum: function",
        ),
    ];
    for (source, expected) in cases {
        let error = Interpreter::new(Vec::new())
            .eval_source(source)
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{source}: {error}");
    }
}

#[test]
fn syntax_errors_keep_their_structured_variants() {
    assert!(matches!(
        Interpreter::new(Vec::new()).eval_source("(quote)"),
        Err(LispError::Expand(ExpandError::InvalidQuoteSyntax {
            got: 0
        }))
    ));
    assert!(matches!(
        Interpreter::new(Vec::new()).eval_source("(unquote x)"),
        Err(LispError::Expand(ExpandError::UnquoteOutsideQuasiquote))
    ));
    assert!(matches!(
        Interpreter::new(Vec::new()).eval_source("(gensym a b)"),
        Err(LispError::Lower(LowerError::InvalidGensymSyntax { got: 2 }))
    ));
    assert!(matches!(
        Interpreter::new(Vec::new()).eval_source("(gensym 1)"),
        Err(LispError::Eval(EvalError::InvalidGensymPrefix("integer")))
    ));
}

#[test]
fn gensym_is_unique_identity_based_and_session_deterministic() {
    let mut first = Interpreter::new(Vec::new());
    let values = first
        .eval_source("(gensym (quote tmp)) (gensym (quote tmp))")
        .unwrap();
    assert_eq!(values[0].to_string(), "tmp__g0");
    assert_eq!(values[1].to_string(), "tmp__g1");
    assert_ne!(values[0], values[1]);

    let fresh = eval_one("(gensym (quote tmp))");
    assert_eq!(fresh.to_string(), "tmp__g0");
    assert_eq!(
        eval_one("(= (gensym (quote tmp)) (quote tmp__g0))"),
        Value::Bool(false)
    );
}

#[test]
fn generated_symbol_survives_structural_ast_round_trip() {
    let generated = Datum::Symbol(Symbol::generated(GensymId(7), Some("tmp".into())));
    let expression = datum_to_expr(&generated);
    assert!(expression.properties().is_empty());
    assert_eq!(expr_to_datum(&expression), generated);

    let reparsed = parse_one(&generated.to_string()).unwrap();
    assert_ne!(expr_to_datum(&reparsed), generated);
    assert_ne!(
        Symbol::interned("tmp__g7"),
        Symbol::generated(GensymId(7), Some("tmp".into()))
    );
}

#[test]
fn datum_conversion_drops_properties_without_mutating_the_source() {
    let mut child = Expr::symbol("x");
    child.set_property("child", PropertyValue::Bool(true));
    let mut source = Expr::list(vec![child]);
    source.set_property("root", PropertyValue::Int(1));
    let original = source.clone();

    let converted = expr_to_datum(&source);
    assert_eq!(converted.to_string(), "(x)");
    assert_eq!(source, original);

    fn assert_empty(expression: &Expr) {
        assert!(expression.properties().is_empty());
        if let ExprKind::List(items) = expression.kind() {
            for item in items {
                assert_empty(item);
            }
        }
    }
    assert_empty(&datum_to_expr(&converted));
}

#[test]
fn verifier_recursively_checks_unquote_ir() {
    let top_level = IrTopLevel::Expr {
        body: IrBody {
            local_count: 0,
            expr: IrExpr::new(
                Span::new(0, 0),
                IrExprKind::QuasiQuote(IrQuasiDatum::Evaluate(Box::new(IrExpr::new(
                    Span::new(0, 0),
                    IrExprKind::LoadLocal(regatelisp::ids::LocalSlot(0)),
                )))),
            ),
        },
        span: Span::new(0, 0),
    };
    assert!(
        regatelisp::verify_top_level(&top_level, &IrModule::new(), &GlobalRegistry::new()).is_err()
    );
}

#[test]
fn datum_values_are_rejected_by_hardware_lowering() {
    let assign = "(module bad (ports (output (meta ((width 8)) y))) (assign y (quote 1)))";
    let set = "(module bad (ports (input (meta ((width 1)) clk)) (output (meta ((width 8)) y))) (register y) (clocked (clock clk rising) (set y (quote 1))))";
    let case = "(module bad (ports (output (meta ((width 8)) y))) (assign y (case (quote 1) (1 1) (else 0))))";
    for source in [assign, set, case] {
        assert_eq!(
            compile_systemverilog(source).unwrap_err(),
            HardwareError::DatumNotHardwareValue
        );
    }
}
