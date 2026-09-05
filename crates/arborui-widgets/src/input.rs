use arborui_core::{CursorShape, CursorState, Modifier, Point, Style};
use arborui_layout::{Align, Dimension, LayoutStyle};
use arborui_text::{TextBuffer, TextEdit, TextMovement, WidthPolicy};
#[cfg(not(test))]
use arborui_text::{graphemes, measure};
use arborui_ui::{Element, EventPhase, KeyAction, KeyModifiers, UiEvent, UiKey};

#[cfg(test)]
use tests::{graphemes, measure};

/// Creates a controlled single-line text input builder.
#[must_use]
pub fn text_input<'a, Message>(
    buffer: &'a TextBuffer,
    on_change: impl Fn(TextBuffer) -> Message + 'a,
) -> TextInput<'a, Message>
where
    Message: 'a,
{
    TextInput::new(buffer, on_change)
}

/// Builder for a controlled, grapheme-aware single-line text input.
pub struct TextInput<'a, Message> {
    buffer: &'a TextBuffer,
    on_change: Box<dyn Fn(TextBuffer) -> Message + 'a>,
    on_submit: Option<Box<dyn Fn() -> Message + 'a>>,
    style: Style,
    selection_style: Style,
    layout: LayoutStyle,
    focus_order: Option<i32>,
}

impl<'a, Message: 'a> TextInput<'a, Message> {
    /// Creates an input borrowing application-owned text state.
    #[must_use]
    pub fn new(buffer: &'a TextBuffer, on_change: impl Fn(TextBuffer) -> Message + 'a) -> Self {
        Self {
            buffer,
            on_change: Box::new(on_change),
            on_submit: None,
            style: Style::default(),
            selection_style: Style::new().add_modifiers(Modifier::REVERSED),
            layout: LayoutStyle::default(),
            focus_order: None,
        }
    }

    /// Sets a repeatable message factory for Enter submissions.
    #[must_use]
    pub fn on_submit(mut self, on_submit: impl Fn() -> Message + 'a) -> Self {
        self.on_submit = Some(Box::new(on_submit));
        self
    }

    /// Sets the input and displayed text style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style added to selected text, whether or not the input is focused.
    ///
    /// Defaults to [`Modifier::REVERSED`]. This replaces the selection style, not
    /// the input style: unspecified colors inherit and modifiers combine with
    /// the resolved input style. Choose a contrasting style if the input already
    /// uses reversed colors; inherited modifiers cannot be removed here.
    #[must_use]
    pub const fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = style;
        self
    }

    /// Sets the input layout properties.
    #[must_use]
    pub const fn layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }

    /// Sets explicit focus traversal order.
    #[must_use]
    pub const fn focus_order(mut self, order: i32) -> Self {
        self.focus_order = Some(order);
        self
    }

    /// Builds the declarative text input element.
    #[must_use]
    pub fn build(self) -> Element<'a, Message> {
        let mut layout = self.layout;
        if layout.min_width == Dimension::Auto {
            layout.min_width = Dimension::cells(1);
        }
        if layout.min_height == Dimension::Auto {
            layout.min_height = Dimension::cells(1);
        }
        let buffer = self.buffer;
        let cursor_byte = buffer.cursor().get();
        let on_change = self.on_change;
        let on_submit = self.on_submit;
        let selection = buffer
            .selection()
            .map_or(0..0, |selection| selection.byte_range());
        let text = buffer.text();
        // Keep one outer flex item and stable spans across selection changes.
        // Spans must not shrink or stretch vertically.
        let span_layout = LayoutStyle::new().flex(0, 0);
        let text_content = Element::container([
            Element::text(&text[..selection.start]).layout(span_layout),
            Element::text(&text[selection.clone()])
                .layout(span_layout)
                .style(self.selection_style),
            Element::text(&text[selection.end..]).layout(span_layout),
        ])
        .layout(LayoutStyle {
            align: Align::Start,
            ..LayoutStyle::default()
        })
        .child_offset_with(cursor_byte as u64, move |size, width_policy| {
            let (_, offset) = input_text_geometry(buffer, size.width, width_policy);
            Point::new(offset, 0)
        });
        let mut element = Element::custom(
            "text-input",
            [
                text_content,
                Element::container([])
                    .layout(LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(1))),
            ],
        )
        .layout(layout)
        .style(self.style)
        .focusable(true)
        .cursor_with_child(0, cursor_byte as u64, move |width_policy, size| {
            let (x, _) = input_text_geometry(buffer, size.width, width_policy);
            CursorState::visible(Point::new(x, 0)).with_shape(CursorShape::Bar)
        })
        .child_offset_with_child(0, cursor_byte as u64, move |size, width_policy, text| {
            let (x, _) = input_text_geometry(buffer, text.width, width_policy);
            let cursor = text.origin().translated(x, 0);
            // Scroll only when the resolved caret leaves the content viewport.
            Point::new(
                cursor
                    .x
                    .clamp(0, i32::from(size.width.saturating_sub(1)))
                    .saturating_sub(cursor.x),
                cursor
                    .y
                    .clamp(0, i32::from(size.height.saturating_sub(1)))
                    .saturating_sub(cursor.y),
            )
        })
        .on_event(EventPhase::Target, move |event, context| {
            let Some(action) = input_action(event) else {
                return;
            };
            match action {
                InputAction::Submit => {
                    if let Some(factory) = on_submit.as_ref() {
                        context.emit(factory());
                        context.mark_handled();
                    }
                }
                InputAction::Edit(edit) => {
                    let mut updated = buffer.clone();
                    updated.apply(edit);
                    if updated != *buffer {
                        context.emit(on_change(updated));
                    }
                    context.mark_handled();
                }
                InputAction::InsertCharacter(character) => {
                    let mut encoded = [0; 4];
                    let text = character.encode_utf8(&mut encoded);
                    let mut updated = buffer.clone();
                    updated.apply(TextEdit::Insert(text));
                    if updated != *buffer {
                        context.emit(on_change(updated));
                    }
                    context.mark_handled();
                }
            }
        });
        if let Some(order) = self.focus_order {
            element = element.focus_order(order);
        }
        element
    }
}

fn input_text_geometry(buffer: &TextBuffer, width: u16, policy: WidthPolicy) -> (i32, i32) {
    let cursor = saturating_i32(measure(&buffer.text()[..buffer.cursor().get()], policy).width);
    if cursor <= i32::from(width.saturating_sub(1)) {
        return (cursor, 0);
    }
    // A column-stretched group can be narrower than its intrinsic text. Scroll
    // the spans inside that clip instead of moving the entire clip offscreen.
    // The prefix proves overflow past the right edge. At the exact edge, keep
    // the outer spare-caret cell unless a positive-width suffix proves overflow.
    if cursor == i32::from(width)
        && !graphemes(&buffer.text()[buffer.cursor().get()..], policy)
            .any(|grapheme| grapheme.width > 0)
    {
        return (cursor, 0);
    }
    let offset = i32::from(width.saturating_sub(1))
        .saturating_sub(cursor)
        .min(0);
    (cursor.saturating_add(offset), offset)
}

enum InputAction<'a> {
    Edit(TextEdit<'a>),
    InsertCharacter(char),
    Submit,
}

fn input_action(event: &UiEvent) -> Option<InputAction<'_>> {
    match event {
        UiEvent::Text(text) => Some(InputAction::Edit(TextEdit::Insert(text))),
        UiEvent::Paste(text) => Some(InputAction::Edit(TextEdit::Insert(text))),
        UiEvent::Key(key) if key.action != KeyAction::Release => {
            let extend_selection = key.modifiers.contains(KeyModifiers::SHIFT);
            let control_shortcut = key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT);
            let command = control_shortcut
                || key.modifiers.contains(KeyModifiers::META)
                || key.modifiers.contains(KeyModifiers::SUPER)
                || key.modifiers.contains(KeyModifiers::HYPER);
            let alt_shortcut = key.modifiers.contains(KeyModifiers::ALT)
                && !key.modifiers.contains(KeyModifiers::CONTROL);
            match key.key {
                UiKey::Backspace => Some(InputAction::Edit(TextEdit::Backspace)),
                UiKey::Delete => Some(InputAction::Edit(TextEdit::Delete)),
                UiKey::Left => Some(InputAction::Edit(TextEdit::Move {
                    movement: TextMovement::Left,
                    extend_selection,
                })),
                UiKey::Right => Some(InputAction::Edit(TextEdit::Move {
                    movement: TextMovement::Right,
                    extend_selection,
                })),
                UiKey::Home => Some(InputAction::Edit(TextEdit::Move {
                    movement: TextMovement::Home,
                    extend_selection,
                })),
                UiKey::End => Some(InputAction::Edit(TextEdit::Move {
                    movement: TextMovement::End,
                    extend_selection,
                })),
                UiKey::Character(character) if command && character.eq_ignore_ascii_case(&'a') => {
                    Some(InputAction::Edit(TextEdit::SelectAll))
                }
                UiKey::Character(character) if !command && !alt_shortcut => {
                    Some(InputAction::InsertCharacter(character))
                }
                UiKey::Enter if key.action == KeyAction::Press => Some(InputAction::Submit),
                _ => None,
            }
        }
        _ => None,
    }
}

fn saturating_i32(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static MEASURED_BYTES: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn measure(text: &str, policy: WidthPolicy) -> arborui_text::TextMetrics {
        MEASURED_BYTES.with(|bytes| bytes.set(bytes.get() + text.len()));
        arborui_text::measure(text, policy)
    }

    pub(super) fn graphemes(
        text: &str,
        policy: WidthPolicy,
    ) -> impl Iterator<Item = arborui_text::Grapheme<'_>> {
        // Count fully measured bytes and yielded suffix bytes, not wall-clock time.
        arborui_text::graphemes(text, policy).inspect(|grapheme| {
            MEASURED_BYTES.with(|bytes| bytes.set(bytes.get() + grapheme.text.len()));
        })
    }

    fn assert_bounded_geometry_work(cluster: &str) {
        for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
            for length in [1 << 20, 64] {
                let mut buffer = TextBuffer::new(cluster.repeat(length));
                buffer.apply(TextEdit::Move {
                    movement: TextMovement::Home,
                    extend_selection: false,
                });
                for position in 0..=41 {
                    if matches!(position, 0 | 1 | 39 | 40 | 41) {
                        MEASURED_BYTES.with(|bytes| bytes.set(0));
                        let (cursor, offset) = input_text_geometry(&buffer, 40, policy);
                        assert_eq!(cursor, position.min(39));
                        assert_eq!(offset, cursor - position);
                        let expected =
                            (position as usize + usize::from(position == 40)) * cluster.len();
                        assert_eq!(
                            MEASURED_BYTES.with(Cell::get),
                            expected,
                            "{policy:?}, cluster={cluster:?}, length={length}, position={position}"
                        );
                    }
                    buffer.apply(TextEdit::Move {
                        movement: TextMovement::Right,
                        extend_selection: false,
                    });
                }
            }
        }
    }

    #[test]
    fn input_ascii_geometry_has_bounded_measurement_work() {
        assert_bounded_geometry_work("a");
    }

    #[test]
    fn input_combining_geometry_has_bounded_measurement_work() {
        assert_bounded_geometry_work("a\u{301}");
    }

    #[test]
    fn input_geometry_right_edge_preserves_the_spare_caret_cell() {
        for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
            for prefix in ["abcd", "a\u{301}\u{b7}\u{1f469}\u{200d}\u{1f4bb}z"] {
                for (suffix, overflow, suffix_bytes) in [
                    ("", false, 0),
                    ("\u{200b}", false, 3),
                    ("\u{200b}xyz", true, 4),
                ] {
                    let width = arborui_text::measure(prefix, policy).width as u16;
                    let mut buffer = TextBuffer::new(format!("{prefix}{suffix}"));
                    buffer.apply(TextEdit::Move {
                        movement: TextMovement::Home,
                        extend_selection: false,
                    });
                    for _ in arborui_text::graphemes(prefix, policy) {
                        buffer.apply(TextEdit::Move {
                            movement: TextMovement::Right,
                            extend_selection: false,
                        });
                    }
                    assert_eq!(buffer.cursor().get(), prefix.len());
                    MEASURED_BYTES.with(|bytes| bytes.set(0));
                    let expected = if overflow {
                        (i32::from(width) - 1, -1)
                    } else {
                        (i32::from(width), 0)
                    };
                    assert_eq!(input_text_geometry(&buffer, width, policy), expected);
                    assert_eq!(MEASURED_BYTES.with(Cell::get), prefix.len() + suffix_bytes);
                }
            }
        }
    }
}
