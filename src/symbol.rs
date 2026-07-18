use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GensymId(pub u64);

#[derive(Debug, Clone)]
pub enum Symbol {
    Interned(String),
    Generated { id: GensymId, hint: Option<String> },
}

impl Symbol {
    pub fn interned(name: impl Into<String>) -> Self {
        Self::Interned(name.into())
    }

    pub fn generated(id: GensymId, hint: Option<String>) -> Self {
        Self::Generated { id, hint }
    }

    pub fn as_interned(&self) -> Option<&str> {
        match self {
            Self::Interned(name) => Some(name),
            Self::Generated { .. } => None,
        }
    }
}

impl PartialEq for Symbol {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Interned(left), Self::Interned(right)) => left == right,
            (Self::Generated { id: left, .. }, Self::Generated { id: right, .. }) => left == right,
            _ => false,
        }
    }
}

impl Eq for Symbol {}

impl Hash for Symbol {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Interned(name) => {
                0_u8.hash(state);
                name.hash(state);
            }
            Self::Generated { id, .. } => {
                1_u8.hash(state);
                id.hash(state);
            }
        }
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interned(name) => f.write_str(name),
            Self::Generated { id, hint } => {
                write!(f, "{}__g{}", hint.as_deref().unwrap_or("g"), id.0)
            }
        }
    }
}
