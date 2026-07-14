use std::marker::PhantomData;

use yatui_core::Style;
use yatui_layout::LayoutStyle;

use crate::Key;

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
#[derive(Clone, Debug)]
pub struct Element<'a, Message> {
    key: Option<Key>,
    kind: WidgetKind,
    layout: LayoutStyle,
    style: Style,
    content: Content<'a>,
    children: Vec<Self>,
    message: PhantomData<fn() -> Message>,
}

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
            message: PhantomData,
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
            message: PhantomData,
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

    pub(crate) const fn text_content(&self) -> Option<&'a str> {
        match self.content {
            Content::Empty => None,
            Content::Text(text) => Some(text),
        }
    }
}
