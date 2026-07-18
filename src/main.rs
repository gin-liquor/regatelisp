use std::io::{self, Read, Write};
use std::process::ExitCode;

use regatelisp::compiler::Compiler;
use regatelisp::{Interpreter, Value};

enum Mode {
    Run,
    DumpReader,
    DumpCore,
    DumpIr,
    Check,
    EmitSystemVerilog,
}

fn main() -> ExitCode {
    let (mode, arg, quiet) = match parse_cli() {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };

    let source = match arg {
        Some(source) => source,
        None => {
            let mut input = String::new();
            if let Err(err) = io::stdin().read_to_string(&mut input) {
                eprintln!("error: failed to read stdin: {err}");
                return ExitCode::FAILURE;
            }
            input
        }
    };

    match mode {
        Mode::Run => run(&source, quiet),
        Mode::DumpReader => dump_reader(&source),
        Mode::DumpCore => dump_core(&source),
        Mode::DumpIr => dump_ir(&source),
        Mode::Check => check(&source),
        Mode::EmitSystemVerilog => emit_systemverilog(&source),
    }
}

fn parse_cli() -> Result<(Mode, Option<String>, bool), String> {
    let mut mode = Mode::Run;
    let mut source = None;
    let mut quiet = false;
    let mut options = true;

    for arg in std::env::args().skip(1) {
        if options && arg == "--" {
            options = false;
            continue;
        }
        if options {
            match arg.as_str() {
                "--quiet" | "-q" => {
                    quiet = true;
                    continue;
                }
                "--dump-reader" => Mode::DumpReader,
                "--dump-core" => Mode::DumpCore,
                "--dump-ir" => Mode::DumpIr,
                "--check" => Mode::Check,
                "--emit-systemverilog" => Mode::EmitSystemVerilog,
                option if option.starts_with('-') => {
                    return Err(format!("unknown option: {option}"));
                }
                _ => {
                    if source.replace(arg).is_some() {
                        return Err("expected at most one source argument".to_string());
                    }
                    continue;
                }
            };
            if !matches!(mode, Mode::Run) {
                return Err("multiple output modes specified".to_string());
            }
            mode = match arg.as_str() {
                "--dump-reader" => Mode::DumpReader,
                "--dump-core" => Mode::DumpCore,
                "--dump-ir" => Mode::DumpIr,
                "--check" => Mode::Check,
                "--emit-systemverilog" => Mode::EmitSystemVerilog,
                _ => unreachable!("recognized CLI mode"),
            };
        } else if source.replace(arg).is_some() {
            return Err("expected at most one source argument".to_string());
        }
    }

    Ok((mode, source, quiet))
}

fn run(source: &str, quiet: bool) -> ExitCode {
    let mut interpreter = Interpreter::new(io::stdout());
    match interpreter.eval_source(source) {
        Ok(values) => {
            if quiet {
                return ExitCode::SUCCESS;
            }
            for value in values {
                // `print` writes its own output; the CLI must not also
                // print a `()` line for the `Unit` it returns.
                if matches!(value, Value::Unit) {
                    continue;
                }
                if let Err(err) = writeln!(interpreter.output_mut(), "{value}") {
                    eprintln!("error: failed to write output: {err}");
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `--dump-reader`: shows the reader S-expression AST, before any
/// expansion. Does not execute anything.
fn dump_reader(source: &str) -> ExitCode {
    match regatelisp::parse_program(source) {
        Ok(exprs) => {
            for expr in exprs {
                println!("{expr:?}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `--dump-core`: shows the core AST after `for` expansion. Does not
/// execute anything (expansion only inspects source shape and known
/// compile-time integer constants, never runtime values).
fn dump_core(source: &str) -> ExitCode {
    let mut compiler = Compiler::new();
    match compiler.expand_source(source) {
        Ok(expressions) => {
            for core in expressions {
                println!("{core:?}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `--dump-ir`: compiles source to verified IR and prints its deterministic
/// text form -- the function table, then each top-level form's own IR (a
/// `let`/`if`/arithmetic expression at top level defines no function, so
/// without this it would print nothing at all). Uses the compile-only
/// `Compiler`, so no `print`, user function call, or runtime loop ever
/// executes.
fn dump_ir(source: &str) -> ExitCode {
    let mut compiler = Compiler::new();
    match compiler.compile_source(source) {
        Ok(top_levels) => {
            print!(
                "{}",
                regatelisp::format_ir_module(compiler.module(), compiler.globals())
            );
            for top_level in &top_levels {
                print!(
                    "{}",
                    regatelisp::format_top_level(top_level, compiler.module(), compiler.globals())
                );
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `--check`: runs tokenize/parse/expand/lower/verify and reports success
/// or failure only. Never executes user code.
fn check(source: &str) -> ExitCode {
    let mut compiler = Compiler::new();
    match compiler.compile_source(source) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `--emit-systemverilog`: compiles a hardware module and writes only the
/// generated SystemVerilog to standard output.
fn emit_systemverilog(source: &str) -> ExitCode {
    match regatelisp::compile_systemverilog(source) {
        Ok(systemverilog) => {
            print!("{systemverilog}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
