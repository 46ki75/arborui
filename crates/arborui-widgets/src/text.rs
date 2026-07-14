use arborui_ui::Element;

/// Creates a borrowed text element.
///
/// Use [`Element::style`] and [`Element::layout`] on the result to configure
/// its visual and layout properties.
#[must_use]
pub fn text<Message>(content: &str) -> Element<'_, Message> {
    Element::text(content)
}
