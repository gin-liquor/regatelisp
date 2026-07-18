use crate::property::{Properties, PropertyValue};

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
