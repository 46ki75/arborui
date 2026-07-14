use arborui_layout::{FlexDirection, LayoutStyle};
use arborui_ui::Element;

/// Composes children from top to bottom.
#[must_use]
pub fn column<'a, Message>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    column_with_gap(children, 0)
}

/// Composes children from top to bottom with a cell gap between them.
#[must_use]
pub fn column_with_gap<'a, Message>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
    gap: u16,
) -> Element<'a, Message> {
    Element::container(children).layout(LayoutStyle {
        direction: FlexDirection::Column,
        gap,
        ..LayoutStyle::default()
    })
}
