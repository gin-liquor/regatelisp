/// A generic S-expression. The parser produces only this shape; special
/// forms such as `fn` and `let` are recognized later, in the evaluator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    String(String),
    Symbol(String),
    List(Vec<Expr>),
}
