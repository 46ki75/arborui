use arborui_layout::{FlexDirection, LayoutStyle};
use arborui_ui::{Element, Key};

/// Composes explicitly keyed children from top to bottom.
///
/// The list owns no selection or navigation state. Keys are applied directly
/// to child elements so retained identity follows application-owned items.
#[must_use]
pub fn list<'a, Message, ItemKey>(
    items: impl IntoIterator<Item = (ItemKey, Element<'a, Message>)>,
) -> Element<'a, Message>
where
    ItemKey: Into<Key>,
{
    list_with_gap(items, 0)
}

/// Composes explicitly keyed children with a cell gap between them.
#[must_use]
pub fn list_with_gap<'a, Message, ItemKey>(
    items: impl IntoIterator<Item = (ItemKey, Element<'a, Message>)>,
    gap: u16,
) -> Element<'a, Message>
where
    ItemKey: Into<Key>,
{
    let children = items.into_iter().map(|(key, child)| child.key(key));
    Element::container(children).layout(LayoutStyle {
        direction: FlexDirection::Column,
        gap,
        ..LayoutStyle::default()
    })
}
