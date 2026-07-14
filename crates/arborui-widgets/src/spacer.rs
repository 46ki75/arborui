use arborui_layout::{Dimension, LayoutStyle};
use arborui_ui::Element;

/// Creates an empty element with an exact cell size.
#[must_use]
pub fn spacer<Message>(width: u16, height: u16) -> Element<'static, Message> {
    spacer_with_dimensions(Dimension::cells(width), Dimension::cells(height))
}

/// Creates an empty element with caller-provided dimensions.
#[must_use]
pub fn spacer_with_dimensions<Message>(
    width: Dimension,
    height: Dimension,
) -> Element<'static, Message> {
    Element::container([]).layout(LayoutStyle::new().size(width, height))
}

/// Creates an empty element that consumes positive free space.
#[must_use]
pub fn flexible_spacer<Message>() -> Element<'static, Message> {
    Element::container([]).layout(LayoutStyle::new().flex(1, 1))
}
