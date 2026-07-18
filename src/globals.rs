//! Global name/value bookkeeping, split into two responsibilities:
//!
//! * `GlobalRegistry` maps source names to stable `GlobalId`s (interning).
//!   The same name always resolves to the same ID, including across
//!   redefinition, so lowering can be done once per top-level form and IDs
//!   stay valid for the lifetime of the `Interpreter`.
//! * `GlobalStore` holds the current runtime `Value` for each `GlobalId`,
//!   distinguishing "not yet defined" from "defined". Reading an unbound
//!   global is where the undefined-symbol error actually surfaces, which is
//!   what lets an `if` branch that is never taken reference a
//!   not-yet-defined global without failing.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::expand::ForConstantSource;
use crate::ids::GlobalId;
use crate::value::Value;

/// Interns global names to stable, source-order `GlobalId`s.
#[derive(Debug, Default)]
pub struct GlobalRegistry {
    names: Vec<String>,
    ids: HashMap<String, GlobalId>,
}

impl GlobalRegistry {
    pub fn new() -> Self {
        GlobalRegistry::default()
    }

    /// Returns the existing ID for `name`, or interns a new one. The same
    /// name always maps to the same ID, so redefining a global (`def`)
    /// reuses its original slot rather than allocating a new one.
    pub fn intern(&mut self, name: &str) -> GlobalId {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = GlobalId(self.names.len() as u32);
        self.names.push(name.to_string());
        self.ids.insert(name.to_string(), id);
        id
    }

    pub fn lookup(&self, name: &str) -> Option<GlobalId> {
        self.ids.get(name).copied()
    }

    pub fn name(&self, id: GlobalId) -> Option<&str> {
        self.names.get(id.index()).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// The runtime value bound to a global, or the absence of one. A slot can
/// be interned (known to the registry) without yet being bound to a value
/// (e.g. a symbol referenced only in an untaken `if` branch, or a `def`
/// whose right-hand side is still being evaluated).
#[derive(Debug, Clone)]
enum GlobalValue {
    Unbound,
    Bound(Value),
}

/// Runtime storage for global values, indexed by `GlobalId`. Uses interior
/// mutability so it can be shared (via `Rc`) between the interpreter and
/// every closure that reads globals, without each closure needing to hold
/// its own mutable handle.
#[derive(Debug, Default)]
pub struct GlobalStore {
    values: RefCell<Vec<GlobalValue>>,
}

impl GlobalStore {
    pub fn new() -> Self {
        GlobalStore::default()
    }

    /// Ensures storage exists for `id`, defaulting new slots to `Unbound`.
    pub fn ensure(&self, id: GlobalId) {
        let mut values = self.values.borrow_mut();
        if values.len() <= id.index() {
            values.resize(id.index() + 1, GlobalValue::Unbound);
        }
    }

    /// Binds `id` to `value`. Used only after a `def` initializer has
    /// already evaluated successfully, so this call itself cannot fail.
    pub fn define(&self, id: GlobalId, value: Value) {
        self.ensure(id);
        self.values.borrow_mut()[id.index()] = GlobalValue::Bound(value);
    }

    /// Reads the current value of `id`, or `None` if it has never been
    /// defined (or was interned but not yet bound). This is where an
    /// unbound-global read becomes an undefined-symbol error, one level up.
    pub fn get(&self, id: GlobalId) -> Option<Value> {
        let values = self.values.borrow();
        match values.get(id.index()) {
            Some(GlobalValue::Bound(v)) => Some(v.clone()),
            _ => None,
        }
    }
}

/// Pairs a `GlobalRegistry` (names) with a `GlobalStore` (runtime values)
/// so `for` expansion can resolve a global name to a known integer
/// constant during normal (executing) interpretation.
pub struct RuntimeConstants<'a> {
    pub registry: &'a GlobalRegistry,
    pub store: &'a GlobalStore,
}

impl ForConstantSource for RuntimeConstants<'_> {
    fn integer_constant(&self, name: &str) -> Option<i64> {
        let id = self.registry.lookup(name)?;
        match self.store.get(id) {
            Some(Value::Int(n)) => Some(n),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_same_name_returns_same_id() {
        let mut reg = GlobalRegistry::new();
        let a = reg.intern("x");
        let b = reg.intern("x");
        assert_eq!(a, b);
    }

    #[test]
    fn redefinition_reuses_the_same_id() {
        let mut reg = GlobalRegistry::new();
        let first = reg.intern("count");
        let store = GlobalStore::new();
        store.define(first, Value::Int(3));
        let second = reg.intern("count");
        assert_eq!(first, second);
        store.define(second, Value::Int(5));
        assert!(matches!(store.get(first), Some(Value::Int(5))));
    }

    #[test]
    fn unbound_global_reads_as_none() {
        let mut reg = GlobalRegistry::new();
        let id = reg.intern("later");
        let store = GlobalStore::new();
        store.ensure(id);
        assert!(store.get(id).is_none());
    }

    #[test]
    fn name_round_trips() {
        let mut reg = GlobalRegistry::new();
        let id = reg.intern("square");
        assert_eq!(reg.name(id), Some("square"));
        assert_eq!(reg.lookup("square"), Some(id));
    }
}
