use yatui_core::Rect;
use yatui_layout::LayoutStyle;

use crate::{Invalidation, Key, WidgetKind};

/// Stable identity for a retained UI node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NodeId(pub(crate) u64);

/// Owned metadata retained between declarative views.
#[derive(Clone, Debug)]
pub struct RetainedNode {
    pub(crate) key: Option<Key>,
    pub(crate) kind: WidgetKind,
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    pub(crate) layout: Rect,
    pub(crate) layout_style: LayoutStyle,
    pub(crate) visual_style: yatui_core::Style,
    pub(crate) content_fingerprint: u64,
    pub(crate) invalidation: Invalidation,
}

impl RetainedNode {
    /// Returns explicit declarative identity, if present.
    #[must_use]
    pub const fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    /// Returns the widget category.
    #[must_use]
    pub const fn kind(&self) -> WidgetKind {
        self.kind
    }

    /// Returns the parent identity.
    #[must_use]
    pub const fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// Returns ordered child identities.
    #[must_use]
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    /// Returns the most recently computed border box.
    #[must_use]
    pub const fn layout(&self) -> Rect {
        self.layout
    }

    /// Returns this node's pending invalidation.
    #[must_use]
    pub const fn invalidation(&self) -> Invalidation {
        self.invalidation
    }
}
