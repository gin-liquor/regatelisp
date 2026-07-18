use std::io::Write;
use std::process::{Command, Stdio};

const COUNTER: &str = "(module counter (ports (input (meta ((width 1)) clk)) (input (meta ((width 1)) reset)) (input (meta ((width 1)) enable)) (output (meta ((width 8)) count))) (register count (clock clk rising) (reset reset (meta ((width 8)) 0)) (enable enable) (next (+ count (meta ((width 8)) 1)))))";

#[test]
fn emits_systemverilog_from_standard_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_regatelisp"))
        .arg("--emit-systemverilog")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(COUNTER.as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr).unwrap().is_empty());

    let systemverilog = String::from_utf8(output.stdout).unwrap();
    assert!(systemverilog.contains("module counter"));
    assert!(systemverilog.contains("output logic [7:0] count"));
    assert!(systemverilog.contains("always_ff @(posedge clk)"));
    assert!(systemverilog.contains("count <= (count + 8'd1);"));
}

#[test]
fn quiet_does_not_suppress_systemverilog_output() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_regatelisp"))
        .args(["--quiet", "--emit-systemverilog"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(COUNTER.as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("module counter")
    );
}

#[test]
fn rejects_unknown_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_regatelisp"))
        .arg("--unknown")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("error: unknown option: --unknown")
    );
}
