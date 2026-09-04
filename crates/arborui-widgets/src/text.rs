use arborui_ui::Element;

/// Creates a borrowed text element.
///
/// Use [`Element::style`] and [`Element::layout`] on the result to configure
/// its visual and layout properties. Mandatory Unicode line breaks create new
/// rows; tabs and other control characters are omitted from terminal output.
#[must_use]
pub fn text<Message>(content: &str) -> Element<'_, Message> {
    Element::text(content)
}
