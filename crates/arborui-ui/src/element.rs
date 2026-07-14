use arborui_core::{CursorState, Point, Size, Style};
use arborui_layout::LayoutStyle;
use arborui_render::{Canvas, DrawError};
use arborui_text::WidthPolicy;

use crate::{EventContext, EventPhase, Key, UiEvent};

/// Stable category used to determine whether retained state is compatible.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WidgetKind {
    /// A flex container with no intrinsic visual content.
    Container,
    /// Borrowed text content.
    Text,
    /// A third-party widget category.
    Custom(&'static str),
}

#[derive(Clone, Copy, Debug)]
enum Content<'a> {
    Empty,
    Text(&'a str),
}

/// Frame-local declarative UI node.
///
/// Borrowed content is used only during synchronous reconciliation, layout,
/// and painting. It is never copied into the retained tree.
pub struct Element<'a, Message> {
    key: Option<Key>,
    kind: WidgetKind,
    layout: LayoutStyle,
    style: Style,
    content: Content<'a>,
    children: Vec<Self>,
    handlers: Vec<EventHandler<'a, Message>>,
    interactive: bool,
    focusable: bool,
    focus_scope: bool,
    focus_order: Option<i32>,
    cursor: Option<CursorState>,
    dynamic_cursor: Option<Box<CursorCallback<'a>>>,
    cursor_fingerprint: u64,
    paint: Option<Box<PaintCallback<'a>>>,
    paint_fingerprint: u64,
    child_offset: Point,
    dynamic_child_offset: Option<Box<ChildOffsetCallback<'a>>>,
    child_offset_fingerprint: u64,
    fill_background: bool,
}

struct EventHandler<'a, Message> {
    phase: EventPhase,
    callback: Box<HandlerCallback<'a, Message>>,
}

type HandlerCallback<'a, Message> = dyn Fn(&UiEvent, &mut EventContext<'_, Message>) + 'a;
type PaintCallback<'a> = dyn Fn(Size, &mut Canvas<'_>) -> Result<(), DrawError> + 'a;
type CursorCallback<'a> = dyn Fn(WidthPolicy, Size) -> CursorState + 'a;
type ChildOffsetCallback<'a> = dyn Fn(Size, WidthPolicy) -> Point + 'a;

impl<'a, Message> Element<'a, Message> {
    /// Creates an empty container from ordered children.
    #[must_use]
    pub fn container(children: impl IntoIterator<Item = Self>) -> Self {
        Self {
            key: None,
            kind: WidgetKind::Container,
            layout: LayoutStyle::default(),
            style: Style::default(),
            content: Content::Empty,
            children: children.into_iter().collect(),
            handlers: Vec::new(),
            interactive: false,
            focusable: false,
            focus_scope: false,
            focus_order: None,
            cursor: None,
            dynamic_cursor: None,
            cursor_fingerprint: 0,
            paint: None,
            paint_fingerprint: 0,
            child_offset: Point::ORIGIN,
            dynamic_child_offset: None,
            child_offset_fingerprint: 0,
            fill_background: true,
        }
    }

    /// Creates a borrowed text leaf.
    #[must_use]
    pub fn text(text: &'a str) -> Self {
        Self {
            key: None,
            kind: WidgetKind::Text,
            layout: LayoutStyle::default(),
            style: Style::default(),
            content: Content::Text(text),
            children: Vec::new(),
            handlers: Vec::new(),
            interactive: false,
            focusable: false,
            focus_scope: false,
            focus_order: None,
            cursor: None,
            dynamic_cursor: None,
            cursor_fingerprint: 0,
            paint: None,
            paint_fingerprint: 0,
            child_offset: Point::ORIGIN,
            dynamic_child_offset: None,
            child_offset_fingerprint: 0,
            fill_background: true,
        }
    }

    /// Creates a custom node without intrinsic content.
    #[must_use]
    pub fn custom(kind: &'static str, children: impl IntoIterator<Item = Self>) -> Self {
        let mut element = Self::container(children);
        element.kind = WidgetKind::Custom(kind);
        element
    }

    /// Assigns explicit stable identity.
    #[must_use]
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Assigns layout behavior.
    #[must_use]
    pub const fn layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }

    /// Assigns visual cell styling.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Registers an ephemeral handler for one dispatch phase.
    #[must_use]
    pub fn on_event(
        mut self,
        phase: EventPhase,
        handler: impl Fn(&UiEvent, &mut EventContext<'_, Message>) + 'a,
    ) -> Self {
        self.handlers.push(EventHandler {
            phase,
            callback: Box::new(handler),
        });
        self.interactive = true;
        self
    }

    /// Enables or disables spatial hit testing for this element.
    #[must_use]
    pub const fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    /// Enables or disables keyboard focus for this element.
    #[must_use]
    pub const fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        if focusable {
            self.interactive = true;
        }
        self
    }

    /// Marks this element as a focus scope, such as an overlay or dialog.
    #[must_use]
    pub const fn focus_scope(mut self, focus_scope: bool) -> Self {
        self.focus_scope = focus_scope;
        self
    }

    /// Sets explicit traversal order within a focus scope.
    #[must_use]
    pub const fn focus_order(mut self, order: i32) -> Self {
        self.focus_order = Some(order);
        self
    }

    /// Sets a terminal cursor intent local to this element's border box.
    #[must_use]
    pub fn cursor(mut self, cursor: CursorState) -> Self {
        self.cursor = Some(cursor);
        self.dynamic_cursor = None;
        self
    }

    /// Computes terminal cursor intent with the renderer's active width policy.
    #[must_use]
    pub fn cursor_with(
        mut self,
        fingerprint: u64,
        cursor: impl Fn(WidthPolicy, Size) -> CursorState + 'a,
    ) -> Self {
        self.cursor = None;
        self.dynamic_cursor = Some(Box::new(cursor));
        self.cursor_fingerprint = fingerprint;
        self
    }

    /// Adds frame-local custom painting after intrinsic content and before children.
    ///
    /// `fingerprint` must change whenever captured visual data changes so retained
    /// invalidation can detect the update.
    #[must_use]
    pub fn paint(
        mut self,
        fingerprint: u64,
        painter: impl Fn(Size, &mut Canvas<'_>) -> Result<(), DrawError> + 'a,
    ) -> Self {
        self.paint = Some(Box::new(painter));
        self.paint_fingerprint = fingerprint;
        self
    }

    /// Translates all descendants while retaining this node as their clip viewport.
    #[must_use]
    pub fn child_offset(mut self, offset: Point) -> Self {
        self.child_offset = offset;
        self.dynamic_child_offset = None;
        self
    }

    /// Computes descendant translation from resolved size and width policy.
    #[must_use]
    pub fn child_offset_with(
        mut self,
        fingerprint: u64,
        offset: impl Fn(Size, WidthPolicy) -> Point + 'a,
    ) -> Self {
        self.child_offset = Point::ORIGIN;
        self.dynamic_child_offset = Some(Box::new(offset));
        self.child_offset_fingerprint = fingerprint;
        self
    }

    /// Enables or disables filling this node's complete border box.
    ///
    /// Disable filling for sparse overlays that should preserve lower visual
    /// cells and hit identities where they do not paint.
    #[must_use]
    pub const fn fill_background(mut self, fill_background: bool) -> Self {
        self.fill_background = fill_background;
        self
    }

    /// Returns explicit identity, if present.
    #[must_use]
    pub const fn explicit_key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    /// Returns the widget category.
    #[must_use]
    pub const fn kind(&self) -> WidgetKind {
        self.kind
    }

    /// Returns the layout style.
    #[must_use]
    pub const fn layout_style(&self) -> LayoutStyle {
        self.layout
    }

    /// Returns the visual style.
    #[must_use]
    pub const fn visual_style(&self) -> Style {
        self.style
    }

    /// Returns ordered child declarations.
    #[must_use]
    pub fn children(&self) -> &[Self] {
        &self.children
    }

    pub(crate) fn handlers(
        &self,
        phase: EventPhase,
    ) -> impl Iterator<Item = &HandlerCallback<'a, Message>> {
        self.handlers
            .iter()
            .filter(move |handler| handler.phase == phase)
            .map(|handler| handler.callback.as_ref())
    }

    pub(crate) const fn is_interactive(&self) -> bool {
        self.interactive
    }

    pub(crate) const fn is_focusable(&self) -> bool {
        self.focusable
    }

    pub(crate) const fn is_focus_scope(&self) -> bool {
        self.focus_scope
    }

    pub(crate) const fn explicit_focus_order(&self) -> Option<i32> {
        self.focus_order
    }

    pub(crate) fn cursor_intent(
        &self,
        width_policy: WidthPolicy,
        size: Size,
    ) -> Option<CursorState> {
        self.dynamic_cursor
            .as_ref()
            .map(|cursor| cursor(width_policy, size))
            .or(self.cursor)
    }

    pub(crate) const fn fixed_cursor_intent(&self) -> Option<CursorState> {
        self.cursor
    }

    pub(crate) const fn cursor_fingerprint(&self) -> u64 {
        self.cursor_fingerprint
    }

    pub(crate) const fn has_dynamic_cursor(&self) -> bool {
        self.dynamic_cursor.is_some()
    }

    pub(crate) fn paint_content(
        &self,
        size: Size,
        canvas: &mut Canvas<'_>,
    ) -> Result<(), DrawError> {
        self.paint
            .as_ref()
            .map_or(Ok(()), |painter| painter(size, canvas))
    }

    pub(crate) const fn paint_fingerprint(&self) -> u64 {
        self.paint_fingerprint
    }

    pub(crate) const fn fixed_children_offset(&self) -> Point {
        self.child_offset
    }

    pub(crate) fn children_offset(&self, size: Size, width_policy: WidthPolicy) -> Point {
        self.dynamic_child_offset
            .as_ref()
            .map_or(self.child_offset, |offset| offset(size, width_policy))
    }

    pub(crate) const fn child_offset_fingerprint(&self) -> u64 {
        self.child_offset_fingerprint
    }

    pub(crate) const fn has_dynamic_child_offset(&self) -> bool {
        self.dynamic_child_offset.is_some()
    }

    pub(crate) const fn fills_background(&self) -> bool {
        self.fill_background
    }

    pub(crate) const fn text_content(&self) -> Option<&'a str> {
        match self.content {
            Content::Empty => None,
            Content::Text(text) => Some(text),
        }
    }
}
