use crate::property::{Properties, PropertyValue};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    kind: ExprKind,
    properties: Properties,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    String(String),
    Symbol(String),
    List(Vec<Expr>),
}

impl Expr {
    pub fn new(kind: ExprKind) -> Self {
        Self::with_properties(kind, Properties::new())
    }
    pub fn with_properties(kind: ExprKind, properties: Properties) -> Self {
        Self { kind, properties }
    }
    pub fn kind(&self) -> &ExprKind {
        &self.kind
    }
    pub fn kind_mut(&mut self) -> &mut ExprKind {
        &mut self.kind
    }
    pub fn properties(&self) -> &Properties {
        &self.properties
    }
    pub fn properties_mut(&mut self) -> &mut Properties {
        &mut self.properties
    }
    pub fn get_property(&self, key: &str) -> Option<&PropertyValue> {
        self.properties.get(key)
    }
    pub fn property(&self, key: &str) -> Option<&PropertyValue> {
        self.get_property(key)
    }
    pub fn has_property(&self, key: &str) -> bool {
        self.get_property(key).is_some()
    }
    pub fn remove_property(&mut self, key: &str) -> Option<PropertyValue> {
        self.properties.remove(key)
    }
    pub fn set_property(
        &mut self,
        key: impl Into<String>,
        value: PropertyValue,
    ) -> Option<PropertyValue> {
        self.properties.insert(key, value)
    }
    pub fn with_property(mut self, key: impl Into<String>, value: PropertyValue) -> Self {
        self.set_property(key, value);
        self
    }
    pub fn int(n: i64) -> Self {
        Self::new(ExprKind::Int(n))
    }
    pub fn bool(value: bool) -> Self {
        Self::new(ExprKind::Bool(value))
    }
    pub fn string(value: impl Into<String>) -> Self {
        Self::new(ExprKind::String(value.into()))
    }
    pub fn symbol(value: impl Into<String>) -> Self {
        Self::new(ExprKind::Symbol(value.into()))
    }
    pub fn list(items: Vec<Self>) -> Self {
        Self::new(ExprKind::List(items))
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.properties.is_empty() {
            write!(f, "(meta (")?;
            for (index, (key, value)) in self.properties.iter().enumerate() {
                if index != 0 {
                    write!(f, " ")?;
                }
                write!(f, "({key} {value})")?;
            }
            write!(f, ") {})", self.kind_display())
        } else {
            write!(f, "{}", self.kind_display())
        }
    }
}

impl Expr {
    fn kind_display(&self) -> ExprKindDisplay<'_> {
        ExprKindDisplay(&self.kind)
    }
}

struct ExprKindDisplay<'a>(&'a ExprKind);

impl fmt::Display for ExprKindDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ExprKind::Int(value) => write!(f, "{value}"),
            ExprKind::Bool(value) => write!(f, "{value}"),
            ExprKind::String(value) => write!(f, "{}", PropertyValue::String(value.clone())),
            ExprKind::Symbol(value) => write!(f, "{value}"),
            ExprKind::List(items) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_constructors_start_with_independent_empty_properties() {
        let mut parent = Expr::list(vec![Expr::int(1)]);
        parent.set_property("width", PropertyValue::Int(8));
        let ExprKind::List(children) = parent.kind() else {
            panic!("expected list");
        };
        assert_eq!(parent.get_property("width"), Some(&PropertyValue::Int(8)));
        assert!(children[0].properties().is_empty());
    }
}
