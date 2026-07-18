//! Integration tests for `print` and its Rust-style format strings, driven
//! entirely through the public `Interpreter` API against an in-memory
//! `Vec<u8>` output target (never real stdout).

use regatelisp::Interpreter;

fn output_of(source: &str) -> String {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source(source).expect("eval should succeed");
    String::from_utf8(interp.into_output()).expect("output should be valid utf-8")
}

fn eval_err(source: &str) -> bool {
    let mut interp = Interpreter::new(Vec::new());
    interp.eval_source(source).is_err()
}

#[test]
fn basic_print() {
    assert_eq!(output_of(r#"(print "hello")"#), "hello");
}

#[test]
fn print_returns_unit_and_cli_suppresses_it() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp.eval_source(r#"(print "hello\n")"#).unwrap();
    assert!(matches!(values[0], regatelisp::Value::Unit));
}

#[test]
fn print_does_not_auto_append_newline() {
    assert_eq!(output_of(r#"(print "hello\n")"#), "hello\n");
}

#[test]
fn print_basic_formatting() {
    assert_eq!(output_of(r#"(print "x={} y={}\n" 10 20)"#), "x=10 y=20\n");
}

#[test]
fn print_positional_arguments() {
    assert_eq!(output_of(r#"(print "{1} {0} {1}" 10 20)"#), "20 10 20");
}

#[test]
fn print_escaped_braces() {
    assert_eq!(output_of(r#"(print "{{{}}}" 10)"#), "{10}");
}

#[test]
fn print_radix_formats() {
    assert_eq!(
        output_of(r#"(print "{:b} {:o} {:x} {:X}" 255 255 255 255)"#),
        "11111111 377 ff FF"
    );
}

#[test]
fn print_alternate_prefixes() {
    assert_eq!(
        output_of(r#"(print "{:#b} {:#o} {:#x} {:#X}" 255 255 255 255)"#),
        "0b11111111 0o377 0xff 0xFF"
    );
}

#[test]
fn print_zero_padding() {
    assert_eq!(output_of(r#"(print "{:08x}" 255)"#), "000000ff");
}

#[test]
fn print_zero_padding_with_prefix() {
    assert_eq!(output_of(r#"(print "{:#010x}" 255)"#), "0x000000ff");
}

#[test]
fn print_alignment() {
    assert_eq!(
        output_of(r#"(print "|{:<8}|{:>8}|{:^8}|" 123 123 123)"#),
        "|123     |     123|  123   |"
    );
}

#[test]
fn print_fill_character() {
    assert_eq!(output_of(r#"(print "|{:*>8}|" 123)"#), "|*****123|");
}

#[test]
fn print_sign() {
    assert_eq!(output_of(r#"(print "{:+} {:+}" 10 -10)"#), "+10 -10");
}

#[test]
fn print_string_alignment() {
    assert_eq!(
        output_of(r#"(print "|{:<8}|{:>8}|" "abc" "abc")"#),
        "|abc     |     abc|"
    );
}

#[test]
fn print_negative_hex_two_complement() {
    assert_eq!(output_of(r#"(print "{:x}" -1)"#), "ffffffffffffffff");
}

#[test]
fn print_readme_example() {
    assert_eq!(
        output_of(
            r#"(let ((value 255)) (print "dec={} hex={:#04x} bin={:#010b}\n" value value value))"#
        ),
        "dec=255 hex=0xff bin=0b11111111\n"
    );
}

// --- print errors ---

#[test]
fn print_no_arguments_is_error() {
    assert!(eval_err("(print)"));
}

#[test]
fn print_first_argument_not_string_is_error() {
    assert!(eval_err("(print 123)"));
}

#[test]
fn print_missing_argument_is_error() {
    assert!(eval_err(r#"(print "{} {}" 10)"#));
}

#[test]
fn print_unused_argument_is_error() {
    assert!(eval_err(r#"(print "{}" 10 20)"#));
}

#[test]
fn print_unclosed_placeholder_is_error() {
    assert!(eval_err(r#"(print "{")"#));
}

#[test]
fn print_stray_close_brace_is_error() {
    assert!(eval_err(r#"(print "}")"#));
}

#[test]
fn print_unsupported_format_is_error() {
    assert!(eval_err(r#"(print "{:?}" 10)"#));
}

#[test]
fn print_hex_on_string_is_error() {
    assert!(eval_err(r#"(print "{:x}" "hello")"#));
}

#[test]
fn print_function_value_is_error() {
    assert!(eval_err(r#"(print "{}" (fn (x) x))"#));
}

#[test]
fn print_out_of_range_argument_index_is_error() {
    assert!(eval_err(r#"(print "{1}" 10)"#));
}

#[test]
fn print_error_leaves_output_unchanged() {
    let mut interp = Interpreter::new(Vec::new());
    let result = interp.eval_source(r#"(print "first={} second={}" 10)"#);
    assert!(result.is_err());
    assert!(interp.output().is_empty());
}

#[test]
fn print_and_toplevel_results_interleave_in_order() {
    let mut interp = Interpreter::new(Vec::new());
    let values = interp
        .eval_source("(print \"answer={}\\n\" 42)\n(+ 1 2)")
        .unwrap();
    assert!(matches!(values[0], regatelisp::Value::Unit));
    assert!(matches!(values[1], regatelisp::Value::Int(3)));
    assert_eq!(interp.output(), b"answer=42\n");
}
