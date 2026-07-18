use regatelisp::compile_systemverilog;
use regatelisp::hardware::{
    HwDesign, HwEnum, HwEnumId, HwEnumMember, HwEnumMemberId, HwExpr, HwExprKind, HwModule, HwPort,
    HwPortDirection, HwSignalId, HwSignalRef, HwType, verify_hardware_design,
};
use regatelisp::property::Properties;

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

#[test]
fn register_next_infers_an_unannotated_increment_from_its_width() {
    let source = "(module counter (ports (input (meta ((width 1)) clk)) (input (meta ((width 1)) rst)) (output (meta ((width 8)) count_out))) (registers (register count (meta ((width 8)) count) (clock clk rising) (reset sync rst high 0) (next (+ count 1))) ) (assign count_out count))";
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("count <= (count + 8'd1);"));
    assert!(output.contains("count <= 8'd0;"));
}

#[test]
fn direct_output_register_uses_the_output_signal_and_short_reset() {
    let source = "(module counter (ports (input (meta ((width 1)) clk)) (input (meta ((width 1)) reset)) (input (meta ((width 1)) enable)) (output (meta ((width 8)) count))) (register count (clock clk rising) (reset reset (meta ((width 8)) 0)) (enable enable) (next (+ count (meta ((width 8)) 1)))))";
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("output logic [7:0] count"));
    assert!(output.contains("count <= 8'd0;"));
    assert!(output.contains("count <= (count + 8'd1);"));
    assert!(!output.contains("logic [7:0] count;"));
    assert!(!output.contains("assign count = count;"));
}

#[test]
fn emits_all_comparisons_and_a_typed_nested_mux() {
    let source = "(module comparisons (ports (input (meta ((width 1)) select)) (input (meta ((width 8)) count)) (output (meta ((width 1)) eq)) (output (meta ((width 1)) ne)) (output (meta ((width 1)) lt)) (output (meta ((width 1)) le)) (output (meta ((width 1)) gt)) (output (meta ((width 1)) ge)) (output (meta ((width 8)) next))) (assign eq (= count 255)) (assign ne (!= count 0)) (assign lt (< 10 count)) (assign le (<= count 10)) (assign gt (> count 10)) (assign ge (>= count 10)) (assign next (if select (if (= count 255) 0 10) (+ count 1))))";
    let output = compile_systemverilog(source).unwrap();
    for comparison in [
        "(count == 8'd255)",
        "(count != 8'd0)",
        "(8'd10 < count)",
        "(count <= 8'd10)",
        "(count > 8'd10)",
        "(count >= 8'd10)",
    ] {
        assert!(
            output.contains(comparison),
            "missing {comparison} in {output}"
        );
    }
    assert!(
        output.contains(
            "assign next = (select ? ((count == 8'd255) ? 8'd0 : 8'd10) : (count + 8'd1));"
        )
    );
}

#[test]
fn comparison_and_mux_type_errors_are_rejected() {
    let comparison_width = "(module bad (ports (input (meta ((width 8)) a)) (output (meta ((width 1)) y))) (assign y (= a (meta ((width 4)) 1))))";
    let multi_bit_condition = "(module bad (ports (input (meta ((width 8)) a)) (output (meta ((width 8)) y))) (assign y (if a 0 1)))";
    let branch_width = "(module bad (ports (input (meta ((width 1)) s)) (output (meta ((width 8)) y))) (assign y (if s (meta ((width 8)) 1) (meta ((width 4)) 1))))";
    let comparison_output = "(module bad (ports (input (meta ((width 8)) a)) (output (meta ((width 8)) y))) (assign y (= a 0)))";
    let signed_comparison = "(module bad (ports (input (meta ((width 8) (signed true)) a)) (output (meta ((width 1)) y))) (assign y (= a 0)))";
    for source in [
        comparison_width,
        multi_bit_condition,
        branch_width,
        comparison_output,
        signed_comparison,
    ] {
        assert!(compile_systemverilog(source).is_err(), "accepted {source}");
    }
}

#[test]
fn verifier_rejects_a_mux_with_a_multi_bit_condition() {
    let byte = HwType {
        width: 8,
        signed: false,
    };
    let design = HwDesign {
        modules: vec![HwModule {
            name: "bad_mux".into(),
            ports: vec![HwPort {
                direction: HwPortDirection::Output,
                name: "y".into(),
                ty: byte,
                properties: Properties::new(),
            }],
            assignments: vec![regatelisp::hardware::HwAssignment {
                destination: HwSignalRef { id: HwSignalId(0) },
                value: HwExpr {
                    kind: HwExprKind::Mux {
                        condition: Box::new(HwExpr {
                            kind: HwExprKind::Constant(1),
                            ty: byte,
                            properties: Properties::new(),
                        }),
                        then_expr: Box::new(HwExpr {
                            kind: HwExprKind::Constant(0),
                            ty: byte,
                            properties: Properties::new(),
                        }),
                        else_expr: Box::new(HwExpr {
                            kind: HwExprKind::Constant(1),
                            ty: byte,
                            properties: Properties::new(),
                        }),
                    },
                    ty: byte,
                    properties: Properties::new(),
                },
                properties: Properties::new(),
            }],
            registers: vec![],
            clocked_blocks: vec![],
            enums: vec![],
            properties: Properties::new(),
        }],
    };
    assert!(verify_hardware_design(&design).is_err());
}

#[test]
fn clocked_blocks_emit_parallel_updates_and_hold_without_self_assignment() {
    let source = "(module swap (ports (input (meta ((width 1)) clk)) (input (meta ((width 1)) enable)) (input (meta ((width 8)) a_in)) (input (meta ((width 8)) b_in)) (output (meta ((width 8)) a)) (output (meta ((width 8)) b))) (register a) (register b) (clocked (clock clk rising) (if enable (do (set a b) (set b a)))))";
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("always_ff @(posedge clk)"));
    assert!(output.contains("if (enable) begin"));
    assert!(output.contains("a <= b;"));
    assert!(output.contains("b <= a;"));
    assert!(!output.contains("a <= a;"));
}

#[test]
fn clocked_driver_conflicts_and_duplicate_path_updates_are_rejected() {
    let duplicate = "(module bad (ports (input (meta ((width 1)) clk)) (output (meta ((width 8)) count))) (register count) (clocked (clock clk rising) (do (set count 1) (set count 2))))";
    let two_blocks = "(module bad (ports (input (meta ((width 1)) clk)) (output (meta ((width 8)) count))) (register count) (clocked (clock clk rising) (set count 1)) (clocked (clock clk falling) (set count 2)))";
    let assign_and_set = "(module bad (ports (input (meta ((width 1)) clk)) (input (meta ((width 8)) value)) (output (meta ((width 8)) count))) (register count) (assign count value) (clocked (clock clk rising) (set count 1)))";
    for source in [duplicate, two_blocks, assign_and_set] {
        assert!(compile_systemverilog(source).is_err(), "accepted {source}");
    }
}

#[test]
fn emits_enum_expression_case_and_statement_case_for_fsm() {
    let source = include_str!("../examples/fsm_sv.lisp");
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("localparam logic [1:0] STATE_IDLE = 2'd0;"));
    assert!(output.contains("localparam logic [1:0] STATE_RUN = 2'd1;"));
    assert!(output.contains("state <= STATE_RUN;"));
    assert!(output.contains("assign status = ((state == STATE_IDLE) ? 2'd0"));
    assert!(output.contains("case (state)"));
    assert!(output.contains("STATE_DONE: begin"));
    assert!(output.contains("default: begin"));
    assert!(output.contains("endcase"));
}

#[test]
fn rejects_duplicate_case_keys_and_same_arm_double_set() {
    let duplicate_key = "(module bad (ports (input (meta ((width 2)) state)) (output (meta ((width 1)) y))) (enum State 2 (IDLE 0)) (assign y (case state (IDLE 0) (0 1) (else 0))))";
    let duplicate_set = "(module bad (ports (input (meta ((width 1)) clk)) (output (meta ((width 2)) state))) (enum State 2 (IDLE 0)) (register state) (clocked (clock clk rising) (case-do state (IDLE (do (set state 0) (set state 1))))))";
    let set_after_case = "(module bad (ports (input (meta ((width 1)) clk)) (output (meta ((width 2)) state))) (enum State 2 (IDLE 0)) (register state) (clocked (clock clk rising) (do (case-do state (IDLE (set state 1))) (set state 0))))";
    assert!(compile_systemverilog(duplicate_key).is_err());
    assert!(compile_systemverilog(duplicate_set).is_err());
    assert!(compile_systemverilog(set_after_case).is_err());
}

#[test]
fn rejects_invalid_enum_declarations_and_case_shapes() {
    let width_zero =
        "(module bad (ports (output (meta ((width 1)) y))) (enum State 0 (IDLE 0)) (assign y 0))";
    let out_of_range =
        "(module bad (ports (output (meta ((width 1)) y))) (enum State 1 (IDLE 2)) (assign y 0))";
    let duplicate_value = "(module bad (ports (output (meta ((width 1)) y))) (enum State 1 (IDLE 0) (RUN 0)) (assign y 0))";
    let missing_else = "(module bad (ports (input (meta ((width 1)) s)) (output (meta ((width 1)) y))) (assign y (case s (0 0))))";
    let misplaced_else = "(module bad (ports (input (meta ((width 1)) s)) (output (meta ((width 1)) y))) (assign y (case s (else 0) (0 1))))";
    for source in [
        width_zero,
        out_of_range,
        duplicate_value,
        missing_else,
        misplaced_else,
    ] {
        assert!(compile_systemverilog(source).is_err(), "accepted {source}");
    }
}

#[test]
fn verifier_rejects_invalid_or_retyped_enum_member_ir() {
    let byte = HwType {
        width: 8,
        signed: false,
    };
    let bit = HwType {
        width: 1,
        signed: false,
    };
    let make_design = |enum_id, expression_type| HwDesign {
        modules: vec![HwModule {
            name: "bad_enum_ir".into(),
            ports: vec![HwPort {
                direction: HwPortDirection::Output,
                name: "y".into(),
                ty: byte,
                properties: Properties::new(),
            }],
            assignments: vec![regatelisp::hardware::HwAssignment {
                destination: HwSignalRef { id: HwSignalId(0) },
                value: HwExpr {
                    kind: HwExprKind::EnumMember(HwEnumMemberId {
                        enum_id: HwEnumId(enum_id),
                        member_index: 0,
                    }),
                    ty: expression_type,
                    properties: Properties::new(),
                },
                properties: Properties::new(),
            }],
            registers: vec![],
            clocked_blocks: vec![],
            enums: vec![HwEnum {
                name: "State".into(),
                ty: bit,
                members: vec![HwEnumMember {
                    name: "IDLE".into(),
                    value: 0,
                    properties: Properties::new(),
                }],
                properties: Properties::new(),
            }],
            properties: Properties::new(),
        }],
    };
    assert!(verify_hardware_design(&make_design(0, byte)).is_err());
    assert!(verify_hardware_design(&make_design(99, byte)).is_err());
}
