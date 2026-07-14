use arborui_layout::{FlexDirection, LayoutStyle};
use arborui_ui::Element;

/// Composes children from left to right.
#[must_use]
pub fn row<'a, Message>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    row_with_gap(children, 0)
}

/// Composes children from left to right with a cell gap between them.
#[must_use]
pub fn row_with_gap<'a, Message>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
    gap: u16,
) -> Element<'a, Message> {
    Element::container(children).layout(LayoutStyle {
        direction: FlexDirection::Row,
        gap,
        ..LayoutStyle::default()
    })
}
