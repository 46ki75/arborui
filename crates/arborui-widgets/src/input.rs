use arborui_core::{CursorShape, CursorState, Point, Style};
use arborui_layout::{Dimension, LayoutStyle};
use arborui_text::{TextBuffer, TextEdit, TextMovement, measure};
use arborui_ui::{Element, EventPhase, KeyAction, KeyModifiers, UiEvent, UiKey};

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
        let cursor_base = Point::new(
            i32::from(layout.border.left).saturating_add(i32::from(layout.padding.left)),
            i32::from(layout.border.top).saturating_add(i32::from(layout.padding.top)),
        );
        let on_change = self.on_change;
        let on_submit = self.on_submit;
        let mut element = Element::custom(
            "text-input",
            [Element::text(buffer.text()).style(self.style)],
        )
        .layout(layout)
        .style(self.style)
        .focusable(true)
        .cursor_with(cursor_byte as u64, move |width_policy, size| {
            let cursor_width = measure(&buffer.text()[..cursor_byte], width_policy).width;
            let scroll = horizontal_scroll(cursor_width, size.width, layout);
            CursorState::visible(
                cursor_base.translated(saturating_i32(cursor_width.saturating_sub(scroll)), 0),
            )
            .with_shape(CursorShape::Bar)
        })
        .child_offset_with(cursor_byte as u64, move |size, width_policy| {
            let cursor_width = measure(&buffer.text()[..cursor_byte], width_policy).width;
            Point::new(
                -saturating_i32(horizontal_scroll(cursor_width, size.width, layout)),
                0,
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
                UiKey::Character(character) if !command => {
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

fn horizontal_scroll(cursor_width: usize, border_width: u16, layout: LayoutStyle) -> usize {
    let horizontal_insets = layout
        .border
        .left
        .saturating_add(layout.border.right)
        .saturating_add(layout.padding.left)
        .saturating_add(layout.padding.right);
    let content_width = usize::from(border_width.saturating_sub(horizontal_insets));
    cursor_width.saturating_add(1).saturating_sub(content_width)
}
