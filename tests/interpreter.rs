use regatelisp::{Interpreter, Value, parse_one};

fn eval_int(source: &str) -> i64 {
    let mut interp = Interpreter::new(Vec::new());
    match interp.eval_source(source).expect("eval should succeed")[0] {
        Value::Int(n) => n,
        ref other => panic!("expected Int, got {other}"),
    }
}

fn eval_err(source: &str) -> bool {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source(source).is_err()
}

// --- lexer ---

#[test]
fn lexer_distinguishes_plus_symbol_and_negative_int() {
    use regatelisp::Token;
    let tokens = regatelisp::tokenize("(+ 12 -3)").expect("should tokenize");
    let kinds: Vec<Token> = tokens.into_iter().map(|t| t.token).collect();
    assert_eq!(
        kinds,
        vec![
            Token::LParen,
            Token::Symbol("+".to_string()),
            Token::Int(12),
            Token::Int(-3),
            Token::RParen,
        ]
    );
}

#[test]
fn lexer_standalone_minus_is_symbol() {
    use regatelisp::Token;
    let tokens = regatelisp::tokenize("(- 12 3)").expect("should tokenize");
    let kinds: Vec<Token> = tokens.into_iter().map(|t| t.token).collect();
    assert_eq!(
        kinds,
        vec![
            Token::LParen,
            Token::Symbol("-".to_string()),
            Token::Int(12),
            Token::Int(3),
            Token::RParen,
        ]
    );
}

#[test]
fn lexer_handles_comments_and_whitespace() {
    use regatelisp::Token;
    let tokens = regatelisp::tokenize("  (+ 1 2)   ; trailing comment\n").expect("should tokenize");
    let kinds: Vec<Token> = tokens.into_iter().map(|t| t.token).collect();
    assert_eq!(
        kinds,
        vec![
            Token::LParen,
            Token::Symbol("+".to_string()),
            Token::Int(1),
            Token::Int(2),
            Token::RParen,
        ]
    );
}

// --- parser ---

#[test]
fn parser_builds_expected_structure() {
    use regatelisp::Expr;
    let expr = parse_one("(+ 1 (* 2 3))").expect("should parse");
    assert_eq!(
        expr,
        Expr::list(vec![
            Expr::symbol("+"),
            Expr::int(1),
            Expr::list(vec![Expr::symbol("*"), Expr::int(2), Expr::int(3),]),
        ])
    );
}

#[test]
fn parser_rejects_unmatched_open_paren() {
    assert!(parse_one("(+ 1 2").is_err());
}

#[test]
fn parser_rejects_unmatched_close_paren() {
    assert!(parse_one("(+ 1 2))").is_err());
}

#[test]
fn parser_rejects_empty_input() {
    assert!(parse_one("").is_err());
}

// --- arithmetic ---

#[test]
fn arithmetic_add() {
    assert_eq!(eval_int("(+ 1 2)"), 3);
}

#[test]
fn arithmetic_sub() {
    assert_eq!(eval_int("(- 10 3)"), 7);
}

#[test]
fn arithmetic_mul() {
    assert_eq!(eval_int("(* 6 7)"), 42);
}

#[test]
fn arithmetic_div_truncates() {
    assert_eq!(eval_int("(/ 7 2)"), 3);
}

#[test]
fn nested_arithmetic() {
    assert_eq!(eval_int("(+ 1 (* 2 3))"), 7);
}

// --- let ---

#[test]
fn let_binds_two_names() {
    assert_eq!(eval_int("(let ((x 10) (y 20)) (+ x y))"), 30);
}

#[test]
fn let_shadowing() {
    assert_eq!(eval_int("(let ((x 1)) (let ((x 2)) x))"), 2);
}

#[test]
fn let_is_parallel_not_sequential() {
    // Inner `y` sees the outer `x` (10), not the inner `x` (1).
    assert_eq!(eval_int("(let ((x 10)) (let ((x 1) (y x)) y))"), 10);
}

#[test]
fn let_inner_binding_cannot_see_sibling_without_outer_definition() {
    assert!(parse_one("(let ((x 1) (y x)) y)").is_ok());
    assert!(eval_err("(let ((x 1) (y x)) y)"));
}

// --- fn ---

#[test]
fn fn_application() {
    assert_eq!(eval_int("((fn (x) (+ x 1)) 5)"), 6);
}

#[test]
fn fn_multiple_args() {
    assert_eq!(eval_int("((fn (x y) (* (+ x y) 2)) 3 4)"), 14);
}

#[test]
fn fn_no_args() {
    assert_eq!(eval_int("((fn () 123))"), 123);
}

// --- closures ---

#[test]
fn closure_capture() {
    let source = "(let ((make-adder (fn (x) (fn (y) (+ x y))))) ((make-adder 10) 5))";
    assert_eq!(eval_int(source), 15);
}

#[test]
fn lexical_scope_not_dynamic() {
    let source = "(let ((x 10)) (let ((f (fn (y) (+ x y)))) (let ((x 100)) (f 1))))";
    assert_eq!(eval_int(source), 11);
}

#[test]
fn higher_order_function() {
    let source = "((fn (f x) (f x)) (fn (n) (* n n)) 6)";
    assert_eq!(eval_int(source), 36);
}

#[test]
fn builtin_shadowing() {
    assert_eq!(eval_int("(let ((+ (fn (x y) (- x y)))) (+ 10 3))"), 7);
}

// --- errors ---

#[test]
fn undefined_symbol_is_error() {
    assert!(eval_err("unknown"));
}

#[test]
fn calling_non_function_is_error() {
    assert!(eval_err("(1 2)"));
}

#[test]
fn wrong_arg_count_is_error() {
    assert!(eval_err("(+ 1)"));
}

#[test]
fn division_by_zero_is_error() {
    assert!(eval_err("(/ 1 0)"));
}

#[test]
fn duplicate_parameter_is_error() {
    assert!(eval_err("(fn (x x) x)"));
}

#[test]
fn duplicate_let_binding_is_error() {
    assert!(eval_err("(let ((x 1) (x 2)) x)"));
}

#[test]
fn empty_list_evaluation_is_error() {
    assert!(eval_err("()"));
}

#[test]
fn addition_overflow_is_error() {
    let source = format!("(+ {} 1)", i64::MAX);
    assert!(eval_err(&source));
}

#[test]
fn subtraction_overflow_is_error() {
    let source = format!("(- {} 1)", i64::MIN);
    assert!(eval_err(&source));
}

#[test]
fn multiplication_overflow_is_error() {
    let source = format!("(* {} 2)", i64::MAX);
    assert!(eval_err(&source));
}

#[test]
fn division_overflow_is_error() {
    let source = format!("(/ {} -1)", i64::MIN);
    assert!(eval_err(&source));
}

// --- eval_source / multiple top-level expressions ---

#[test]
fn eval_source_evaluates_each_top_level_expression() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp
        .eval_source("(+ 1 2)\n(* 3 4)")
        .expect("should evaluate");
    assert_eq!(values.len(), 2);
    assert!(matches!(values[0], Value::Int(3)));
    assert!(matches!(values[1], Value::Int(12)));
}

// --- radix integer literals ---

#[test]
fn binary_octal_hex_literals() {
    assert_eq!(eval_int("0b0001"), 1);
    assert_eq!(eval_int("0o17"), 15);
    assert_eq!(eval_int("0x01"), 1);
    assert_eq!(eval_int("0xFF"), 255);
}

#[test]
fn negative_radix_literals() {
    assert_eq!(eval_int("-0b1000"), -8);
    assert_eq!(eval_int("-0o10"), -8);
    assert_eq!(eval_int("-0x80"), -128);
}

#[test]
fn underscore_separated_literals() {
    assert_eq!(eval_int("0b1111_0000"), 240);
    assert_eq!(eval_int("0xDEAD_BEEF"), 3735928559);
}

#[test]
fn radix_i64_boundaries() {
    assert_eq!(eval_int("0x7fff_ffff_ffff_ffff"), i64::MAX);
    assert_eq!(eval_int("-0x8000_0000_0000_0000"), i64::MIN);
}

#[test]
fn malformed_radix_literals_are_errors() {
    for src in [
        "0b", "0o", "0x", "0b102", "0o89", "0x12g", "0x_ff", "0xff_", "1__000", "0Xff",
    ] {
        assert!(eval_err(src), "expected error for {src}");
    }
}

#[test]
fn out_of_range_radix_literals_are_errors() {
    assert!(eval_err("0x8000_0000_0000_0000"));
    assert!(eval_err("-0x8000_0000_0000_0001"));
}

// --- remainder ---

#[test]
fn remainder_basic() {
    assert_eq!(eval_int("(% 10 3)"), 1);
}

#[test]
fn remainder_sign_follows_dividend() {
    assert_eq!(eval_int("(% -10 3)"), -1);
    assert_eq!(eval_int("(% 10 -3)"), 1);
    assert_eq!(eval_int("(% -10 -3)"), -1);
}

#[test]
fn remainder_by_zero_is_error() {
    assert!(eval_err("(% 10 0)"));
}

#[test]
fn remainder_overflow_is_error() {
    let source = format!("(% {} -1)", i64::MIN);
    assert!(eval_err(&source));
}

#[test]
fn remainder_wrong_arity_is_error() {
    assert!(eval_err("(% 1)"));
    assert!(eval_err("(% 1 2 3)"));
}

#[test]
fn remainder_type_error() {
    assert!(eval_err(r#"(% 10 "3")"#));
}

// --- strings ---

#[test]
fn string_literal_evaluates_to_itself() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp.eval_source(r#""hello""#).unwrap();
    assert!(matches!(&values[0], Value::String(s) if s.as_str() == "hello"));
}

#[test]
fn string_escape_newline() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp.eval_source(r#""a\nb""#).unwrap();
    assert!(matches!(&values[0], Value::String(s) if s.as_str() == "a\nb"));
}

#[test]
fn string_escape_quote() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp.eval_source(r#""\"hello\"""#).unwrap();
    assert!(matches!(&values[0], Value::String(s) if s.as_str() == "\"hello\""));
}

#[test]
fn unterminated_string_is_error() {
    assert!(eval_err("\"unterminated"));
}

#[test]
fn invalid_escape_is_error() {
    assert!(eval_err(r#""\q""#));
}
