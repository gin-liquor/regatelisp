//! Deterministic, human-readable text rendering of an `IrModule`. This is
//! not `Debug` output -- it is a dedicated formatter so the shape stays
//! stable regardless of internal struct layout, and so identifiers use the
//! short, consistent notation documented in the README (`@n` for globals,
//! `fn#n` for functions, `%n` for locals, `^n` for captures, `loop#n` for
//! loop IDs).

use std::fmt::Write as _;

use crate::globals::GlobalRegistry;
use crate::ir::{
    IrCaptureSource, IrConst, IrExpr, IrExprKind, IrFunction, IrModule, IrQuasiDatum, IrTopLevel,
};

pub fn format_ir_module(module: &IrModule, globals: &GlobalRegistry) -> String {
    let mut out = String::new();
    for function in &module.functions {
        format_function(&mut out, function, module, globals);
    }
    out
}

/// Formats one top-level form's IR, given the module it was lowered into
/// (so `MakeClosure` references inside it can be resolved for display).
pub fn format_top_level(
    top_level: &IrTopLevel,
    module: &IrModule,
    globals: &GlobalRegistry,
) -> String {
    let mut out = String::new();
    match top_level {
        IrTopLevel::Expr { body, .. } => {
            let _ = writeln!(out, "top.expr(locals={}) {{", body.local_count);
            format_expr_line(&mut out, &body.expr, module, globals, 1);
            let _ = writeln!(out, "}}");
        }
        IrTopLevel::Define {
            target,
            initializer,
            ..
        } => {
            let name = globals.name(*target).unwrap_or("?");
            let _ = writeln!(
                out,
                "top.define @{} \"{name}\" (locals={}) {{",
                target.0, initializer.local_count
            );
            format_expr_line(&mut out, &initializer.expr, module, globals, 1);
            let _ = writeln!(out, "}}");
        }
    }
    out
}

fn format_function(
    out: &mut String,
    function: &IrFunction,
    module: &IrModule,
    globals: &GlobalRegistry,
) {
    let _ = writeln!(
        out,
        "fn#{}(params={} captures={} locals={}) {{",
        function.id.0, function.parameter_count, function.capture_count, function.local_count
    );
    format_expr_line(out, &function.body, module, globals, 1);
    let _ = writeln!(out, "}}");
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn format_expr_line(
    out: &mut String,
    expr: &IrExpr,
    module: &IrModule,
    globals: &GlobalRegistry,
    depth: usize,
) {
    indent(out, depth);
    format_expr_inline(out, expr, module, globals, depth);
    out.push('\n');
}

fn format_expr_inline(
    out: &mut String,
    expr: &IrExpr,
    module: &IrModule,
    globals: &GlobalRegistry,
    depth: usize,
) {
    match &expr.kind {
        IrExprKind::Const(c) => format_const(out, c),
        IrExprKind::Quote(datum) => {
            let _ = write!(out, "quote {datum}");
        }
        IrExprKind::QuasiQuote(template) => {
            out.push_str("quasiquote ");
            format_quasi_datum(out, template, module, globals, depth);
        }
        IrExprKind::Gensym { prefix } => {
            out.push_str("gensym");
            if let Some(prefix) = prefix {
                out.push('(');
                format_expr_inline(out, prefix, module, globals, depth);
                out.push(')');
            }
        }
        IrExprKind::LoadLocal(slot) => {
            let _ = write!(out, "local %{}", slot.0);
        }
        IrExprKind::LoadCapture(slot) => {
            let _ = write!(out, "capture ^{}", slot.0);
        }
        IrExprKind::LoadGlobal(id) => {
            let name = globals.name(*id).unwrap_or("?");
            let _ = write!(out, "global @{} \"{name}\"", id.0);
        }
        IrExprKind::Let { bindings, body } => {
            let _ = write!(out, "let(");
            for (i, binding) in bindings.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "%{} = ", binding.target.0);
                format_expr_inline(out, &binding.initializer, module, globals, depth);
            }
            out.push_str(") {\n");
            format_expr_line(out, body, module, globals, depth + 1);
            indent(out, depth);
            out.push('}');
        }
        IrExprKind::MakeClosure { function, captures } => {
            let _ = write!(out, "closure fn#{} captures=[", function.0);
            for (i, source) in captures.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                match source {
                    IrCaptureSource::Local(slot) => {
                        let _ = write!(out, "local %{}", slot.0);
                    }
                    IrCaptureSource::Capture(slot) => {
                        let _ = write!(out, "capture ^{}", slot.0);
                    }
                }
            }
            out.push(']');
        }
        IrExprKind::Call { callee, arguments } => {
            out.push_str("call(\n");
            indent(out, depth + 1);
            format_expr_inline(out, callee, module, globals, depth + 1);
            out.push('\n');
            for arg in arguments {
                indent(out, depth + 1);
                format_expr_inline(out, arg, module, globals, depth + 1);
                out.push('\n');
            }
            indent(out, depth);
            out.push(')');
        }
        IrExprKind::If {
            condition,
            then_expr,
            else_expr,
        } => {
            out.push_str("if(\n");
            indent(out, depth + 1);
            format_expr_inline(out, condition, module, globals, depth + 1);
            out.push('\n');
            indent(out, depth);
            out.push_str(") then {\n");
            format_expr_line(out, then_expr, module, globals, depth + 1);
            indent(out, depth);
            out.push_str("} else {\n");
            format_expr_line(out, else_expr, module, globals, depth + 1);
            indent(out, depth);
            out.push('}');
        }
        IrExprKind::RangeLoop {
            loop_id,
            variable,
            start,
            end,
            step,
            body,
        } => {
            let _ = write!(out, "range_loop loop#{} %{} start=", loop_id.0, variable.0);
            format_expr_inline(out, start, module, globals, depth);
            out.push_str(" end=");
            format_expr_inline(out, end, module, globals, depth);
            out.push_str(" step=");
            format_expr_inline(out, step, module, globals, depth);
            out.push_str(" {\n");
            format_expr_line(out, body, module, globals, depth + 1);
            indent(out, depth);
            out.push('}');
        }
        IrExprKind::StateLoop {
            loop_id,
            states,
            condition,
            updates,
            body,
        } => {
            let _ = write!(out, "state_loop loop#{} states=[", loop_id.0);
            for (i, state) in states.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "%{} = ", state.target.0);
                format_expr_inline(out, &state.initializer, module, globals, depth);
            }
            out.push_str("] while(\n");
            indent(out, depth + 1);
            format_expr_inline(out, condition, module, globals, depth + 1);
            out.push('\n');
            indent(out, depth);
            out.push_str(") do {\n");
            format_expr_line(out, body, module, globals, depth + 1);
            indent(out, depth);
            out.push_str("} next=[");
            for (i, update) in updates.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "%{} = ", update.target.0);
                format_expr_inline(out, &update.value, module, globals, depth);
            }
            out.push(']');
        }
        IrExprKind::Break { target, value } => {
            let _ = write!(out, "break loop#{}", target.0);
            if let Some(value) = value {
                out.push_str(" value=");
                format_expr_inline(out, value, module, globals, depth);
            }
        }
        IrExprKind::Sequence(items) => {
            out.push_str("sequence {\n");
            for item in items {
                format_expr_line(out, item, module, globals, depth + 1);
            }
            indent(out, depth);
            out.push('}');
        }
        IrExprKind::Do(items) => {
            out.push_str("do {");
            if !items.is_empty() {
                out.push('\n');
                for item in items {
                    format_expr_line(out, item, module, globals, depth + 1);
                }
                indent(out, depth);
            }
            out.push('}');
        }
    }
}

fn format_quasi_datum(
    out: &mut String,
    template: &IrQuasiDatum,
    module: &IrModule,
    globals: &GlobalRegistry,
    depth: usize,
) {
    match template {
        IrQuasiDatum::Datum(datum) => {
            let _ = write!(out, "{datum}");
        }
        IrQuasiDatum::List(items) => {
            out.push('(');
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    out.push(' ');
                }
                format_quasi_datum(out, item, module, globals, depth);
            }
            out.push(')');
        }
        IrQuasiDatum::Evaluate(expression) => {
            out.push_str("(evaluate ");
            format_expr_inline(out, expression, module, globals, depth);
            out.push(')');
        }
        IrQuasiDatum::Splice(expression) => {
            out.push_str("(splice ");
            format_expr_inline(out, expression, module, globals, depth);
            out.push(')');
        }
    }
}

fn format_const(out: &mut String, c: &IrConst) {
    match c {
        IrConst::Int(n) => {
            let _ = write!(out, "const(int {n})");
        }
        IrConst::Bool(b) => {
            let _ = write!(out, "const(bool {b})");
        }
        IrConst::String(s) => {
            let _ = write!(out, "const(string {s:?})");
        }
        IrConst::Unit => out.push_str("const(unit)"),
    }
}
