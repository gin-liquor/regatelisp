use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyValue {
    Int(i64),
    Bool(bool),
    String(String),
    Symbol(String),
    List(Vec<PropertyValue>),
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
