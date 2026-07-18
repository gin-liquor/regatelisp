//! The structured, high-level intermediate representation. Unlike the Core
//! AST, names have already been resolved to slots/IDs and closures carry
//! explicit capture lists -- but control flow is still structured (no basic
//! blocks, no SSA), which keeps this IR easy to interpret directly and easy
//! to eventually translate to other backends without a separate
//! flattening pass.

use crate::datum::Datum;
use crate::error::Span;
use crate::ids::{CaptureSlot, FunctionId, GlobalId, LocalSlot, LoopId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrConst {
    Int(i64),
    Bool(bool),
    String(String),
    Unit,
}

/// Where a closure's capture slot gets its value from at `MakeClosure`
/// time: either a local slot in the *enclosing* function, or a capture
/// slot the enclosing function already holds (re-capturing a value the
/// enclosing closure itself captured, for multi-level closures).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrCaptureSource {
    Local(LocalSlot),
    Capture(CaptureSlot),
}

#[derive(Debug, Clone)]
pub struct IrLetBinding {
    pub target: LocalSlot,
    pub initializer: IrExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct IrStateBinding {
    pub target: LocalSlot,
    pub initializer: IrExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct IrStateUpdate {
    pub target: LocalSlot,
    pub value: IrExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct IrExpr {
    pub span: Span,
    pub kind: IrExprKind,
}

impl IrExpr {
    pub fn new(span: Span, kind: IrExprKind) -> Self {
        IrExpr { span, kind }
    }
}

#[derive(Debug, Clone)]
pub enum IrExprKind {
    Const(IrConst),
    Quote(Datum),
    QuasiQuote(IrQuasiDatum),
    Gensym {
        prefix: Option<Box<IrExpr>>,
    },

    LoadLocal(LocalSlot),
    LoadCapture(CaptureSlot),
    LoadGlobal(GlobalId),

    Let {
        bindings: Vec<IrLetBinding>,
        body: Box<IrExpr>,
    },

    MakeClosure {
        function: FunctionId,
        captures: Vec<IrCaptureSource>,
    },

    Call {
        callee: Box<IrExpr>,
        arguments: Vec<IrExpr>,
    },

    If {
        condition: Box<IrExpr>,
        then_expr: Box<IrExpr>,
        else_expr: Box<IrExpr>,
    },

    RangeLoop {
        loop_id: LoopId,
        variable: LocalSlot,
        start: Box<IrExpr>,
        end: Box<IrExpr>,
        step: Box<IrExpr>,
        body: Box<IrExpr>,
    },

    StateLoop {
        loop_id: LoopId,
        states: Vec<IrStateBinding>,
        condition: Box<IrExpr>,
        updates: Vec<IrStateUpdate>,
        body: Box<IrExpr>,
    },

    Break {
        target: LoopId,
        value: Option<Box<IrExpr>>,
    },

    Sequence(Vec<IrExpr>),
    Do(Vec<IrExpr>),
}

#[derive(Debug, Clone)]
pub enum IrQuasiDatum {
    Datum(Datum),
    List(Vec<IrQuasiDatum>),
    Evaluate(Box<IrExpr>),
    Splice(Box<IrExpr>),
}

/// The body of a function or a top-level expression: a `local_count`-sized
/// frame is all it needs at runtime.
#[derive(Debug, Clone)]
pub struct IrBody {
    pub local_count: u32,
    pub expr: IrExpr,
}

#[derive(Debug, Clone)]
pub struct IrFunction {
    pub id: FunctionId,
    pub name_hint: Option<String>,
    pub parameter_count: u32,
    pub capture_count: u32,
    pub local_count: u32,
    pub body: IrExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum IrTopLevel {
    Expr {
        body: IrBody,
        span: Span,
    },

    Define {
        target: GlobalId,
        initializer: IrBody,
        span: Span,
    },
}

#[derive(Debug, Clone, Default)]
pub struct IrModule {
    pub functions: Vec<IrFunction>,
}

impl IrModule {
    pub fn new() -> Self {
        IrModule::default()
    }

    pub fn function(&self, id: FunctionId) -> Option<&IrFunction> {
        self.functions.get(id.index())
    }
}
