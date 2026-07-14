//! Borrowed declarative elements and retained UI identity.
//!
//! Element data is consumed synchronously. The retained tree stores only owned
//! identity, geometry, fingerprints, and invalidation state.

mod element;
mod invalidation;
mod key;
mod node;
mod reconcile;
mod tree;

pub use element::{Element, WidgetKind};
pub use invalidation::Invalidation;
pub use key::Key;
pub use node::{NodeId, RetainedNode};
pub use reconcile::{ReconcileError, ReconcileReport};
pub use tree::{UiError, UiTree};
