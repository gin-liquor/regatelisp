use std::collections::BTreeMap;
use std::fmt;

use crate::ast::{Expr, ExprKind};
use crate::symbol::Symbol;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyValue {
    Int(i64),
    Bool(bool),
    String(String),
    Symbol(String),
    GeneratedSymbol(Symbol),
    List(Vec<PropertyValue>),
}

impl PropertyValue {
    /// Converts reader syntax into inert property data; lists are never evaluated.
    pub fn from_expr(expr: &Expr) -> Self {
        match expr.kind() {
            ExprKind::Int(value) => Self::Int(*value),
            ExprKind::Bool(value) => Self::Bool(*value),
            ExprKind::String(value) => Self::String(value.clone()),
            ExprKind::Symbol(value) => Self::Symbol(value.clone()),
            ExprKind::GeneratedSymbol(value) => Self::GeneratedSymbol(value.clone()),
            ExprKind::List(items) => Self::List(items.iter().map(Self::from_expr).collect()),
        }
    }
}

impl fmt::Display for PropertyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => write!(f, "{value}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::String(value) => write!(f, "\"{}\"", escape_string(value)),
            Self::Symbol(value) => write!(f, "{value}"),
            Self::GeneratedSymbol(value) => write!(f, "{value}"),
            Self::List(items) => {
                write!(f, "(")?;
                for (index, item) in items.iter().enumerate() {
                    if index != 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, ")")
            }
        }
    }
}

fn escape_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Properties {
    entries: BTreeMap<String, PropertyValue>,
}

impl Properties {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn get(&self, key: &str) -> Option<&PropertyValue> {
        self.entries.get(key)
    }
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: PropertyValue,
    ) -> Option<PropertyValue> {
        self.entries.insert(key.into(), value)
    }
    pub fn remove(&mut self, key: &str) -> Option<PropertyValue> {
        self.entries.remove(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &PropertyValue)> {
        self.entries.iter()
    }
    pub fn with(mut self, key: impl Into<String>, value: PropertyValue) -> Self {
        self.insert(key, value);
        self
    }

    /// Adds only keys not already present, preserving explicit inner metadata.
    pub fn merge_missing_from(&mut self, outer: &Self) {
        for (key, value) in outer.iter() {
            self.entries
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_are_deterministic_and_replace_by_key() {
        let mut properties = Properties::new().with("width", PropertyValue::Int(8));
        assert!(!properties.is_empty());
        assert_eq!(
            properties.insert("width", PropertyValue::Int(16)),
            Some(PropertyValue::Int(8))
        );
        assert_eq!(properties.get("width"), Some(&PropertyValue::Int(16)));
        assert_eq!(
            properties
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            vec!["width"]
        );
        assert_eq!(properties.remove("width"), Some(PropertyValue::Int(16)));
        assert!(properties.is_empty());
    }

    #[test]
    fn properties_clone_and_equality_include_nested_values() {
        let properties = Properties::new().with(
            "shape",
            PropertyValue::List(vec![PropertyValue::Symbol("wire".to_string())]),
        );
        assert_eq!(properties, properties.clone());
    }
}
