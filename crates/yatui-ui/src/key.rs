use std::sync::Arc;

/// Explicit stable identity for a child in a dynamic collection.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Key {
    /// Numeric application identity.
    Integer(u64),
    /// Owned string application identity.
    String(Arc<str>),
}

impl From<u64> for Key {
    fn from(value: u64) -> Self {
        Self::Integer(value)
    }
}

impl From<u32> for Key {
    fn from(value: u32) -> Self {
        Self::Integer(u64::from(value))
    }
}

impl From<usize> for Key {
    fn from(value: usize) -> Self {
        Self::Integer(value as u64)
    }
}

impl From<&str> for Key {
    fn from(value: &str) -> Self {
        Self::String(Arc::from(value))
    }
}

impl From<String> for Key {
    fn from(value: String) -> Self {
        Self::String(Arc::from(value))
    }
}
