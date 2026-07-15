use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use arborui_text::{WidthPolicy, graphemes};

/// Stable identity for an interned grapheme cluster.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GraphemeId(u32);

impl GraphemeId {
    /// Returns the numeric identity assigned by the store.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[cfg(test)]
    pub(crate) const fn from_test_value(value: u32) -> Self {
        Self(value)
    }
}

/// Errors produced while interning graphemes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphemeStoreError {
    /// The value was not exactly one extended grapheme cluster.
    InvalidGrapheme,
    /// The store exhausted its representable identity space.
    CapacityExceeded,
    /// A cell referenced an identity that is not present in the store.
    UnknownId(GraphemeId),
}

impl fmt::Display for GraphemeStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGrapheme => {
                formatter.write_str("value is not one extended grapheme cluster")
            }
            Self::CapacityExceeded => formatter.write_str("grapheme store capacity exceeded"),
            Self::UnknownId(id) => write!(formatter, "unknown grapheme id {}", id.get()),
        }
    }
}

impl std::error::Error for GraphemeStoreError {}

/// A store of deduplicated grapheme strings.
///
/// Clones retain their entries and share identity allocation, so independently
/// added entries cannot alias while cloned stores coexist.
#[derive(Clone, Debug)]
pub struct GraphemeStore {
    entries: HashMap<GraphemeId, Arc<str>>,
    ids: HashMap<Arc<str>, GraphemeId>,
    next_id: Arc<AtomicU32>,
}

impl Default for GraphemeStore {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            ids: HashMap::new(),
            next_id: Arc::new(AtomicU32::new(0)),
        }
    }
}

impl GraphemeStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of distinct graphemes in the store.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the store contains no graphemes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Interns exactly one extended grapheme cluster.
    pub fn intern(&mut self, value: &str) -> Result<GraphemeId, GraphemeStoreError> {
        let mut clusters = graphemes(value, WidthPolicy::Unicode);
        if clusters.next().is_none() || clusters.next().is_some() {
            return Err(GraphemeStoreError::InvalidGrapheme);
        }

        if let Some(id) = self.ids.get(value) {
            return Ok(*id);
        }

        let id = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                (next < u32::MAX).then(|| next + 1)
            })
            .map(GraphemeId)
            .map_err(|_| GraphemeStoreError::CapacityExceeded)?;
        let value: Arc<str> = Arc::from(value);
        self.entries.insert(id, Arc::clone(&value));
        self.ids.insert(value, id);
        Ok(id)
    }

    /// Resolves an identity to its shared grapheme text.
    pub fn get(&self, id: GraphemeId) -> Result<&Arc<str>, GraphemeStoreError> {
        self.entries
            .get(&id)
            .ok_or(GraphemeStoreError::UnknownId(id))
    }

    pub(crate) fn retain(&mut self, ids: impl IntoIterator<Item = GraphemeId>) {
        let retained = ids.into_iter().collect::<HashSet<_>>();
        self.entries.retain(|id, _| retained.contains(id));
        self.ids.retain(|_, id| retained.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_deduplicates_complete_graphemes() -> Result<(), GraphemeStoreError> {
        let mut store = GraphemeStore::new();
        let first = store.intern("👨‍👩‍👧‍👦")?;
        let second = store.intern("👨‍👩‍👧‍👦")?;

        assert_eq!(first, second);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(first)?.as_ref(), "👨‍👩‍👧‍👦");
        Ok(())
    }

    #[test]
    fn rejects_empty_and_multiple_graphemes() {
        let mut store = GraphemeStore::new();

        assert_eq!(store.intern(""), Err(GraphemeStoreError::InvalidGrapheme));
        assert_eq!(store.intern("ab"), Err(GraphemeStoreError::InvalidGrapheme));
    }

    #[test]
    fn cloned_stores_do_not_alias_independently_interned_ids() -> Result<(), GraphemeStoreError> {
        let mut first = GraphemeStore::new();
        let mut second = first.clone();

        let first_id = first.intern("a")?;
        let second_id = second.intern("b")?;

        assert_ne!(first_id, second_id);
        assert_eq!(first.get(first_id)?.as_ref(), "a");
        assert_eq!(second.get(second_id)?.as_ref(), "b");
        assert_eq!(
            first.get(second_id),
            Err(GraphemeStoreError::UnknownId(second_id))
        );
        assert_eq!(
            second.get(first_id),
            Err(GraphemeStoreError::UnknownId(first_id))
        );
        Ok(())
    }
}
