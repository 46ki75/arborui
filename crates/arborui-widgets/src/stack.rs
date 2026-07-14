use arborui_layout::Position;
use arborui_ui::Element;

/// Overlays children at the same origin in declaration order.
///
/// Later children paint over earlier children. Each child's existing layout
/// dimensions and constraints are preserved; only later layers become
/// absolute. The first layer determines an automatically sized stack, so put
/// the largest intrinsic layer first or assign the stack an explicit size.
#[must_use]
pub fn stack<'a, Message>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::container(children.into_iter().enumerate().map(|(index, child)| {
        if index == 0 {
            child
        } else {
            let layout = child.layout_style().position(Position::Absolute);
            child.layout(layout)
        }
    }))
}
