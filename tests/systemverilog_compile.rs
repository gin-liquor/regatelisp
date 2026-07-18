use regatelisp::compile_systemverilog;
use regatelisp::hardware::{
    HardwareError, HwDesign, HwEnum, HwEnumId, HwEnumMember, HwEnumMemberId, HwExpr, HwExprKind,
    HwModule, HwPort, HwPortDirection, HwSignalId, HwSignalRef, HwType, lower_hardware_design,
    verify_hardware_design,
};
use regatelisp::property::{Properties, PropertyValue};

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

#[test]
fn emits_bit_slice_concat_and_resize_operations() {
    let source = include_str!("../examples/vector_ops_sv.lisp");
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("assign opcode = word[15:12];"));
    assert!(output.contains("assign flag = word[11];"));
    assert!(output.contains("assign swapped = {word[7:0], word[15:8]};"));
    assert!(output.contains("assign extended = 16'($signed(signed_byte));"));
    assert!(output.contains("captured <= word[7:0];"));
}

#[test]
fn bit_operations_work_inside_control_expressions_and_preserve_signedness() {
    let source = "(module bits (ports (input (meta ((width 1)) clk)) (input (meta ((width 16)) word)) (input (meta ((width 8) (signed true)) signed_byte)) (output (meta ((width 1)) flag)) (output (meta ((width 1)) signed_flag)) (output (meta ((width 4)) high_nibble)) (output (meta ((width 8)) y)) (output (meta ((width 16)) signed_concat)) (output (meta ((width 16) (signed true)) extended)) (output (meta ((width 16)) zero_extended)) (output (meta ((width 8)) saved))) (register saved) (assign flag (if (bit word 0) 1 0)) (assign signed_flag (bit signed_byte 7)) (assign high_nibble (slice signed_byte 7 4)) (assign y (case (slice word 3 2) (0 (slice word 7 0)) (else (concat (bit word 0) (slice word 6 0))))) (assign signed_concat (concat signed_byte signed_byte)) (assign extended (resize signed_byte 16)) (assign zero_extended (resize (slice word 7 0) 16)) (clocked (clock clk rising) (case-do (bit word 0) (0 (set saved (resize (slice word 7 0) 8))) (else (set saved (concat (slice word 3 0) (slice word 7 4)))))))";
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("(word[0] ? 1'd1 : 1'd0)"));
    assert!(output.contains("(word[3:2] == 2'd0)"));
    assert!(output.contains("{word[0], word[6:0]}"));
    assert!(output.contains("assign signed_flag = signed_byte[7];"));
    assert!(output.contains("assign high_nibble = signed_byte[7:4];"));
    assert!(output.contains("assign signed_concat = {signed_byte, signed_byte};"));
    assert!(output.contains("case (word[0])"));
    assert!(output.contains("saved <= 8'($unsigned(word[7:0]));"));
    assert!(output.contains("saved <= {word[3:0], word[7:4]};"));
    assert!(output.contains("assign zero_extended = 16'($unsigned(word[7:0]));"));
}

#[test]
fn rejects_invalid_bit_slice_concat_and_resize_forms() {
    let cases = [
        "(module bad (ports (input (meta ((width 8)) x)) (output (meta ((width 1)) y))) (assign y (bit x 8)))",
        "(module bad (ports (input (meta ((width 8)) x)) (output (meta ((width 1)) y))) (assign y (bit x -1)))",
        "(module bad (ports (input (meta ((width 8)) x)) (output (meta ((width 4)) y))) (assign y (slice x 2 3)))",
        "(module bad (ports (input (meta ((width 8)) x)) (output (meta ((width 4)) y))) (assign y (slice x 8 5)))",
        "(module bad (ports (input (meta ((width 8)) x)) (output (meta ((width 8)) y))) (assign y (concat x)))",
        "(module bad (ports (input (meta ((width 8)) x)) (output (meta ((width 8)) y))) (assign y (concat x 1)))",
        "(module bad (ports (input (meta ((width 8)) x)) (output (meta ((width 8)) y))) (assign y (resize x 0)))",
        "(module bad (ports (input (meta ((width 8)) x)) (output (meta ((width 16)) y))) (assign y x)",
    ];
    for source in cases {
        assert!(compile_systemverilog(source).is_err(), "accepted {source}");
    }
}

#[test]
fn reports_specific_vector_operation_lowering_errors() {
    let wrap = |expression: &str, width| {
        format!(
            "(module bad (ports (input (meta ((width 8)) x)) (output (meta ((width {width})) y))) (assign y {expression}))"
        )
    };
    assert_eq!(
        compile_systemverilog(&wrap("(bit x)", 1)).unwrap_err(),
        HardwareError::InvalidBitSelect
    );
    assert_eq!(
        compile_systemverilog(&wrap("(bit x x)", 1)).unwrap_err(),
        HardwareError::InvalidBitSelect
    );
    assert_eq!(
        compile_systemverilog(&wrap("(bit x 8)", 1)).unwrap_err(),
        HardwareError::IndexOutOfRange { index: 8, width: 8 }
    );
    assert_eq!(
        compile_systemverilog(&wrap("(slice x 3)", 4)).unwrap_err(),
        HardwareError::InvalidSlice
    );
    assert_eq!(
        compile_systemverilog(&wrap("(slice x 2 3)", 1)).unwrap_err(),
        HardwareError::InvalidSlice
    );
    assert_eq!(
        compile_systemverilog(&wrap("(concat x)", 8)).unwrap_err(),
        HardwareError::InvalidConcat
    );
    assert_eq!(
        compile_systemverilog(&wrap("(resize x x)", 8)).unwrap_err(),
        HardwareError::InvalidResize
    );
    assert_eq!(
        compile_systemverilog(&wrap("(resize x 0)", 8)).unwrap_err(),
        HardwareError::InvalidWidth("resize".into())
    );
    let overflow = "(module bad (ports (input (meta ((width 4294967295)) x)) (output (meta ((width 1)) y))) (assign y (concat x x)))";
    assert_eq!(
        compile_systemverilog(overflow).unwrap_err(),
        HardwareError::InvalidWidth("concat".into())
    );
}

#[test]
fn handles_vector_operation_boundaries_enum_members_and_deep_nesting() {
    let source = "(module vectors (ports (input (meta ((width 8)) x)) (input (meta ((width 8) (signed true)) sx)) (output (meta ((width 8)) full)) (output (meta ((width 1)) one)) (output (meta ((width 24)) three)) (output (meta ((width 16)) symbolic)) (output (meta ((width 16)) typed_literal)) (output (meta ((width 4)) shrunk)) (output (meta ((width 4) (signed true)) signed_shrunk)) (output (meta ((width 8)) same)) (output (meta ((width 8)) nested))) (enum Byte 8 (MAGIC 165)) (assign full (slice x 7 0)) (assign one (slice sx 0 0)) (assign three (concat x sx MAGIC)) (assign symbolic (concat MAGIC x)) (assign typed_literal (concat (meta ((width 8)) 1) x)) (assign shrunk (resize x 4)) (assign signed_shrunk (resize sx 4)) (assign same (resize x 8)) (assign nested (slice (concat (slice x 3 0) (slice x 7 4)) 7 0)))";
    let output = compile_systemverilog(source).unwrap();
    assert!(output.contains("assign full = x[7:0];"));
    assert!(output.contains("assign one = sx[0:0];"));
    assert!(output.contains("assign three = {x, sx, MAGIC};"));
    assert!(output.contains("assign symbolic = {MAGIC, x};"));
    assert!(output.contains("assign typed_literal = {8'd1, x};"));
    assert!(output.contains("assign shrunk = 4'($unsigned(x));"));
    assert!(output.contains("assign signed_shrunk = 4'($signed(sx));"));
    assert!(output.contains("assign same = 8'($unsigned(x));"));
    assert!(output.contains("assign nested = ({x[3:0], x[7:4]})[7:0];"));
}

#[test]
fn vector_operation_ir_preserves_source_properties() {
    let source = "(module props (ports (input (meta ((width 8)) x)) (output (meta ((width 1)) b)) (output (meta ((width 4)) s)) (output (meta ((width 16)) c)) (output (meta ((width 16)) r))) (assign b (meta ((operation bit)) (bit x 0))) (assign s (meta ((operation slice)) (slice x 3 0))) (assign c (meta ((operation concat)) (concat x x))) (assign r (meta ((operation resize)) (resize x 16))))";
    let expressions = regatelisp::parse_program(source).unwrap();
    let design = lower_hardware_design(&expressions).unwrap();
    let expected = ["bit", "slice", "concat", "resize"];
    for (assignment, expected) in design.modules[0].assignments.iter().zip(expected) {
        assert_eq!(
            assignment.value.properties.get("operation"),
            Some(&PropertyValue::Symbol(expected.into()))
        );
    }
}

#[test]
fn verifier_rejects_malformed_bit_operation_ir() {
    let byte = HwType {
        width: 8,
        signed: false,
    };
    let bit = HwType {
        width: 1,
        signed: false,
    };
    let signed_bit = HwType {
        width: 1,
        signed: true,
    };
    let nibble = HwType {
        width: 4,
        signed: false,
    };
    let signed_byte = HwType {
        width: 8,
        signed: true,
    };
    let signed_nibble = HwType {
        width: 4,
        signed: true,
    };
    let signed_word = HwType {
        width: 16,
        signed: true,
    };
    let malformed = |kind, ty| HwDesign {
        modules: vec![HwModule {
            name: "bad_bit_ir".into(),
            ports: vec![
                HwPort {
                    direction: HwPortDirection::Input,
                    name: "x".into(),
                    ty: byte,
                    properties: Properties::new(),
                },
                HwPort {
                    direction: HwPortDirection::Output,
                    name: "y".into(),
                    ty,
                    properties: Properties::new(),
                },
            ],
            assignments: vec![regatelisp::hardware::HwAssignment {
                destination: HwSignalRef { id: HwSignalId(1) },
                value: HwExpr {
                    kind,
                    ty,
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
    let reference = || HwExpr {
        kind: HwExprKind::Reference(HwSignalRef { id: HwSignalId(0) }),
        ty: byte,
        properties: Properties::new(),
    };
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::BitSelect {
                value: Box::new(reference()),
                index: 8
            },
            bit
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::Slice {
                value: Box::new(reference()),
                high: 3,
                low: 0
            },
            signed_nibble
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::BitSelect {
                value: Box::new(reference()),
                index: 0
            },
            signed_bit
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::Slice {
                value: Box::new(reference()),
                high: 3,
                low: 4
            },
            nibble
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::Slice {
                value: Box::new(reference()),
                high: 8,
                low: 5
            },
            nibble
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::Slice {
                value: Box::new(reference()),
                high: 3,
                low: 0
            },
            byte
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::Concat {
                values: vec![reference()]
            },
            byte
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::Concat {
                values: vec![reference(), reference()]
            },
            signed_word
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::Resize {
                value: Box::new(reference()),
                new_width: 0
            },
            byte
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::Resize {
                value: Box::new(reference()),
                new_width: 4
            },
            byte
        ))
        .is_err()
    );
    assert!(
        verify_hardware_design(&malformed(
            HwExprKind::Resize {
                value: Box::new(reference()),
                new_width: 8
            },
            signed_byte
        ))
        .is_err()
    );
}

#[test]
fn sample_systemverilog_passes_an_available_external_compiler() {
    let systemverilog =
        compile_systemverilog(include_str!("../examples/vector_ops_sv.lisp")).unwrap();
    let temp = std::env::temp_dir();
    let stem = format!("regatelisp_vector_ops_{}", std::process::id());
    let source_path = temp.join(format!("{stem}.sv"));
    let output_path = temp.join(format!("{stem}.out"));
    std::fs::write(&source_path, systemverilog).unwrap();

    let iverilog_available = std::process::Command::new("iverilog")
        .arg("-V")
        .output()
        .is_ok();
    if iverilog_available {
        let status = std::process::Command::new("iverilog")
            .args(["-g2012", "-s", "vector_ops", "-o"])
            .arg(&output_path)
            .arg(&source_path)
            .status()
            .unwrap();
        assert!(status.success(), "Icarus Verilog rejected generated output");
    }

    let verilator_available = std::process::Command::new("verilator")
        .arg("--version")
        .output()
        .is_ok();
    if verilator_available {
        let status = std::process::Command::new("verilator")
            .args(["--lint-only", "--top-module", "vector_ops"])
            .arg(&source_path)
            .current_dir(&temp)
            .status()
            .unwrap();
        assert!(status.success(), "Verilator rejected generated output");
    }

    let _ = std::fs::remove_file(source_path);
    let _ = std::fs::remove_file(output_path);
}
