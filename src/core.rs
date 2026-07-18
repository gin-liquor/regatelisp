use crate::datum::Datum;
use crate::property::{Properties, PropertyValue};
use crate::symbol::Symbol;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreExpr {
    kind: CoreExprKind,
    properties: Properties,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreExprKind {
    Int(i64),
    Bool(bool),
    String(String),
    Symbol(String),
    GeneratedSymbol(Symbol),
    List(Vec<CoreExpr>),
    Sequence(Vec<CoreExpr>),
    Quote(Datum),
    QuasiQuote(QuasiDatum),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuasiDatum {
    Datum(Datum),
    List(Vec<QuasiDatum>),
    Evaluate(Box<CoreExpr>),
    Splice(Box<CoreExpr>),
}

impl CoreExpr {
    pub fn new(kind: CoreExprKind) -> Self {
        Self::with_properties(kind, Properties::new())
    }
    pub fn with_properties(kind: CoreExprKind, properties: Properties) -> Self {
        Self { kind, properties }
    }
    pub fn kind(&self) -> &CoreExprKind {
        &self.kind
    }
    pub fn kind_mut(&mut self) -> &mut CoreExprKind {
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
    pub fn set_property(
        &mut self,
        key: impl Into<String>,
        value: PropertyValue,
    ) -> Option<PropertyValue> {
        self.properties.insert(key, value)
    }
    pub fn remove_property(&mut self, key: &str) -> Option<PropertyValue> {
        self.properties.remove(key)
    }
    pub fn int(n: i64) -> Self {
        Self::new(CoreExprKind::Int(n))
    }
    pub fn bool(value: bool) -> Self {
        Self::new(CoreExprKind::Bool(value))
    }
    pub fn string(value: impl Into<String>) -> Self {
        Self::new(CoreExprKind::String(value.into()))
    }
    pub fn symbol(value: impl Into<String>) -> Self {
        Self::new(CoreExprKind::Symbol(value.into()))
    }
    pub fn symbol_value(value: Symbol) -> Self {
        match value {
            Symbol::Interned(name) => Self::symbol(name),
            generated @ Symbol::Generated { .. } => {
                Self::new(CoreExprKind::GeneratedSymbol(generated))
            }
        }
    }
    pub fn list(items: Vec<Self>) -> Self {
        Self::new(CoreExprKind::List(items))
    }
    pub fn sequence(items: Vec<Self>) -> Self {
        Self::new(CoreExprKind::Sequence(items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_constructors_start_with_empty_properties() {
        assert!(
            CoreExpr::sequence(vec![CoreExpr::int(1)])
                .properties()
                .is_empty()
        );
    }
}
