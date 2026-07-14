use arborui_core::{Point, Rect};
use arborui_layout::LayoutStyle;

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
    pub(crate) content: Rect,
    pub(crate) layout_style: LayoutStyle,
    pub(crate) visual_style: arborui_core::Style,
    pub(crate) content_fingerprint: u64,
    pub(crate) paint_fingerprint: u64,
    pub(crate) invalidation: Invalidation,
    pub(crate) interactive: bool,
    pub(crate) focusable: bool,
    pub(crate) focus_scope: bool,
    pub(crate) focus_order: Option<i32>,
    pub(crate) cursor_intent: Option<arborui_core::CursorState>,
    pub(crate) cursor_fingerprint: u64,
    pub(crate) dynamic_cursor: bool,
    pub(crate) child_offset: Point,
    pub(crate) child_offset_fingerprint: u64,
    pub(crate) dynamic_child_offset: bool,
    pub(crate) fill_background: bool,
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

    /// Returns the most recently computed content box.
    #[must_use]
    pub const fn content(&self) -> Rect {
        self.content
    }

    /// Returns this node's pending invalidation.
    #[must_use]
    pub const fn invalidation(&self) -> Invalidation {
        self.invalidation
    }

    /// Returns whether this node participates in spatial hit testing.
    #[must_use]
    pub const fn is_interactive(&self) -> bool {
        self.interactive
    }

    /// Returns whether this node accepts keyboard focus.
    #[must_use]
    pub const fn is_focusable(&self) -> bool {
        self.focusable
    }

    /// Returns whether this node establishes a focus scope.
    #[must_use]
    pub const fn is_focus_scope(&self) -> bool {
        self.focus_scope
    }
}
