//! A minimal Lisp interpreter used to demonstrate a compiler-style
//! pipeline: source text -> reader AST -> expansion -> core AST ->
//! resolution/lowering -> structured IR -> verification -> IR evaluation.
//!
//! This crate is independent from GateLisp.

pub mod ast;
pub mod backend;
pub mod builtin;
pub mod capture;
pub mod compiler;
pub mod core;
pub mod error;
pub mod expand;
pub mod format;
pub mod globals;
pub mod ids;
pub mod interpreter;
pub mod ir;
pub mod ir_eval;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod property;
pub mod text_ir;
pub mod value;
pub mod verify;

use crate::expand::ExpansionContext;

pub use ast::{Expr, ExprKind};
pub use core::{CoreExpr, CoreExprKind};
pub use error::{
    BackendError, EvalError, ExpandError, FormatError, LexError, LispError, LowerError, ParseError,
    VerifyError,
};
pub use expand::{ExpansionLimits, ForConstantSource};
pub use globals::{GlobalRegistry, GlobalStore, RuntimeConstants};
pub use interpreter::{FrontendSession, Interpreter};
pub use ir::IrModule;
pub use lexer::{SpannedToken, Token};
pub use property::{Properties, PropertyValue};
pub use value::{Builtin, Closure, Value};

pub fn tokenize(source: &str) -> Result<Vec<SpannedToken>, LispError> {
    Ok(lexer::tokenize(source)?)
}

pub fn parse_one(source: &str) -> Result<Expr, LispError> {
    parser::parse_one(source)
}

pub fn parse_program(source: &str) -> Result<Vec<Expr>, LispError> {
    parser::parse_program(source)
}

/// Expands a single reader expression into a core expression, unrolling any
/// `for` forms it contains. Exposed so the expansion phase can be tested in
/// isolation from lowering and evaluation.
pub fn expand(expr: &Expr, context: &ExpansionContext) -> Result<CoreExpr, LispError> {
    Ok(expand::expand(expr, context)?)
}

/// Lowers a core expression into IR, resolving names against `globals` and
/// appending any functions it defines to `module`.
pub fn lower_top_level(
    core: &CoreExpr,
    globals: &mut GlobalRegistry,
    module: &mut IrModule,
) -> Result<ir::IrTopLevel, LispError> {
    let mut context = lower::LowerContext::new(globals);
    let (top_level, functions) = lower::lower_top_level(core, &mut context)?;
    module.functions.extend(functions);
    Ok(top_level)
}

/// Verifies a lowered top-level form (and every function currently in
/// `module`) before it is executed or handed to a backend.
pub fn verify_top_level(
    top_level: &ir::IrTopLevel,
    module: &IrModule,
    globals: &GlobalRegistry,
) -> Result<(), LispError> {
    verify::verify_top_level(top_level, module, globals)?;
    verify::verify_module(module, globals)?;
    Ok(())
}

/// Formats a whole IR module (its function table) as deterministic,
/// human-readable text.
pub fn format_ir_module(module: &IrModule, globals: &GlobalRegistry) -> String {
    text_ir::format_ir_module(module, globals)
}

/// Formats a single top-level form's IR (a `let`/`if`/call/`def` at top
/// level has no function-table entry of its own, so this is needed
/// alongside `format_ir_module` to see its IR at all).
pub fn format_top_level(
    top_level: &ir::IrTopLevel,
    module: &IrModule,
    globals: &GlobalRegistry,
) -> String {
    text_ir::format_top_level(top_level, module, globals)
}

pub use ir::IrTopLevel;
