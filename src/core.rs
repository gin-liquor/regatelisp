/// The evaluator's internal expression form, produced by the expander from
/// a reader `Expr`. Structurally identical to `Expr` except for the added
/// `Sequence` node, which the reader can never produce -- it exists only to
/// represent a `for` loop's unrolled iterations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreExpr {
    Int(i64),
    Bool(bool),
    String(String),
    Symbol(String),
    List(Vec<CoreExpr>),

    /// A run of expressions evaluated left to right, each for effect; the
    /// whole sequence evaluates to `Unit`. Only the expander produces this,
    /// as the unrolled body of a `for` loop.
    Sequence(Vec<CoreExpr>),
}
