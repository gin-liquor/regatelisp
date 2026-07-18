use std::fmt;

use crate::ast::{Expr, ExprKind};
use crate::symbol::Symbol;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Datum {
    Integer(i64),
    Bool(bool),
    String(String),
    Symbol(Symbol),
    List(Vec<Datum>),
}

pub fn expr_to_datum(expr: &Expr) -> Datum {
    match expr.kind() {
        ExprKind::Int(value) => Datum::Integer(*value),
        ExprKind::Bool(value) => Datum::Bool(*value),
        ExprKind::String(value) => Datum::String(value.clone()),
        ExprKind::Symbol(symbol) => Datum::Symbol(Symbol::interned(symbol.clone())),
        ExprKind::GeneratedSymbol(symbol) => Datum::Symbol(symbol.clone()),
        ExprKind::List(items) => Datum::List(items.iter().map(expr_to_datum).collect()),
    }
}

pub fn datum_to_expr(datum: &Datum) -> Expr {
    match datum {
        Datum::Integer(value) => Expr::int(*value),
        Datum::Bool(value) => Expr::bool(*value),
        Datum::String(value) => Expr::string(value.clone()),
        Datum::Symbol(symbol) => Expr::symbol_value(symbol.clone()),
        Datum::List(items) => Expr::list(items.iter().map(datum_to_expr).collect()),
    }
}

impl fmt::Display for Datum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => write!(f, "{value}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::String(value) => write!(f, "{value:?}"),
            Self::Symbol(symbol) => write!(f, "{symbol}"),
            Self::List(items) => {
                f.write_str("(")?;
                for (index, item) in items.iter().enumerate() {
                    if index != 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str(")")
            }
        }
    }
}
