use std::io::{self, Read, Write};
use std::process::ExitCode;

use regatelisp::compiler::Compiler;
use regatelisp::{Interpreter, Value};

enum Mode {
    Run,
    Quiet,
    DumpReader,
    DumpCore,
    DumpIr,
    Check,
}

fn main() -> ExitCode {
    let mut args = std::env::args();
    let _program_name = args.next();
    let first_arg = args.next();

    let (mode, arg) = match first_arg.as_deref() {
        Some("--quiet" | "-q") => (Mode::Quiet, args.next()),
        Some("--dump-reader") => (Mode::DumpReader, args.next()),
        Some("--dump-core") => (Mode::DumpCore, args.next()),
        Some("--dump-ir") => (Mode::DumpIr, args.next()),
        Some("--check") => (Mode::Check, args.next()),
        other => (Mode::Run, other.map(str::to_string)),
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
        Mode::Run | Mode::Quiet => run(&source, matches!(mode, Mode::Quiet)),
        Mode::DumpReader => dump_reader(&source),
        Mode::DumpCore => dump_core(&source),
        Mode::DumpIr => dump_ir(&source),
        Mode::Check => check(&source),
    }
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
    let exprs = match regatelisp::parse_program(source) {
        Ok(exprs) => exprs,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut compiler = Compiler::new();
    for expr in &exprs {
        match compiler.expand_only(expr) {
            Ok(core) => println!("{core:?}"),
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
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
