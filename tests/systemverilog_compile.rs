use regatelisp::compile_systemverilog;

#[test]
fn emits_deterministic_passthrough() {
    let source = "(module passthrough (ports (input (meta ((width 8)) a)) (output (meta ((width 8)) y))) (assign y a))";
    let expected = "module passthrough (\n    input  wire [7:0] a,\n    output wire [7:0] y\n);\n\nassign y = a;\n\nendmodule\n";
    assert_eq!(compile_systemverilog(source).unwrap(), expected);
    assert_eq!(compile_systemverilog(source).unwrap(), expected);
}

#[test]
fn emits_arithmetic_mux_and_typed_constants() {
    let source = "(module mux (ports (input (meta ((width 1)) s)) (input (meta ((width 8)) a)) (input (meta ((width 8)) b)) (output (meta ((width 8)) y))) (assign y (if s (+ a b) (meta ((width 8)) 42))))";
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("assign y = (s ? (a + b) : 8'd42);"));
}

#[test]
fn rejects_hardware_type_and_assignment_errors() {
    let width = "(module bad (ports (input (meta ((width 8)) a)) (input (meta ((width 16)) b)) (output (meta ((width 8)) y))) (assign y (+ a b)))";
    assert!(compile_systemverilog(width).is_err());
    let missing = "(module bad (ports (input (meta ((width 8)) a)) (output (meta ((width 8)) y))))";
    assert!(compile_systemverilog(missing).is_err());
    let input = "(module bad (ports (input (meta ((width 8)) a)) (output (meta ((width 8)) y))) (assign a y) (assign y a))";
    assert!(compile_systemverilog(input).is_err());
}

#[test]
fn emits_rising_and_falling_registers_with_synchronous_controls() {
    let source = "(module dual (ports (input (meta ((width 1)) clk)) (input (meta ((width 1)) rst)) (input (meta ((width 1)) en)) (input (meta ((width 1)) d)) (output (meta ((width 1)) q))) (registers (register state (meta ((width 1)) state) (clock clk falling) (reset sync rst high 0) (enable en) (next d))) (assign q state))";
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("logic state;"));
    assert!(output.contains("always_ff @(negedge clk)"));
    assert!(output.contains("state <= 1'd0;"));
    assert!(output.contains("else if (en)"));
    assert!(output.contains("assign q = state;"));
}
