//! Common interface future code generators (Rust, C, VHDL, bytecode, ...)
//! would implement. Only `TextIrBackend` -- rendering IR back to text -- is
//! implemented today; the trait exists so verified IR has one obvious
//! extension point rather than each future backend inventing its own entry
//! function.

use crate::error::BackendError;
use crate::globals::GlobalRegistry;
use crate::ir::IrModule;
use crate::text_ir;

pub trait IrBackend {
    type Output;

    fn emit(
        &mut self,
        module: &IrModule,
        globals: &GlobalRegistry,
    ) -> Result<Self::Output, BackendError>;
}

/// Renders a verified `IrModule` to its deterministic text form. Mainly
/// useful as the reference implementation of `IrBackend` and as the engine
/// behind the CLI's `--dump-ir`.
#[derive(Debug, Default)]
pub struct TextIrBackend;

impl IrBackend for TextIrBackend {
    type Output = String;

    fn emit(
        &mut self,
        module: &IrModule,
        globals: &GlobalRegistry,
    ) -> Result<String, BackendError> {
        Ok(text_ir::format_ir_module(module, globals))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_backend_emits_nonempty_output_for_a_function() {
        use crate::error::Span;
        use crate::ids::FunctionId;
        use crate::ir::{IrConst, IrExpr, IrExprKind, IrFunction};

        let mut module = IrModule::new();
        module.functions.push(IrFunction {
            id: FunctionId(0),
            name_hint: None,
            parameter_count: 0,
            capture_count: 0,
            local_count: 0,
            body: IrExpr::new(Span::new(0, 0), IrExprKind::Const(IrConst::Int(1))),
            span: Span::new(0, 0),
        });
        let globals = GlobalRegistry::new();
        let mut backend = TextIrBackend;
        let output = backend.emit(&module, &globals).unwrap();
        assert!(output.contains("fn#0"));
    }
}
