use std::fmt;

use crate::{Invalidation, Key};

/// Reconciliation failures that leave the retained tree unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconcileError {
    /// Two children of one element used the same explicit key.
    DuplicateSiblingKey(Key),
    /// Event dispatch used a view whose identity differs from committed UI state.
    ViewDoesNotMatchCommittedTree,
    /// Event dispatch used a renderer state not committed with this UI tree.
    WrongCommittedRenderer,
}

impl fmt::Display for ReconcileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSiblingKey(key) => {
                write!(formatter, "duplicate explicit sibling key {key:?}")
            }
            Self::ViewDoesNotMatchCommittedTree => {
                formatter.write_str("event view does not match the committed retained tree")
            }
            Self::WrongCommittedRenderer => {
                formatter.write_str("renderer state was not committed with this UI tree")
            }
        }
    }
}

impl std::error::Error for ReconcileError {}

/// Summary of one retained-tree reconciliation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReconcileReport {
    /// Compatible retained nodes reused by the new view.
    pub reused: usize,
    /// New retained nodes allocated.
    pub created: usize,
    /// Obsolete retained nodes removed.
    pub removed: usize,
    /// Most expensive work requested by the resulting tree.
    pub invalidation: Invalidation,
}
