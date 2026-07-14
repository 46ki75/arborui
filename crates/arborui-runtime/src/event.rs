use arborui_terminal::{
    KeyCode, KeyEventKind, KeyModifiers as TerminalModifiers, MouseButton, MouseEventKind,
    TerminalEvent,
};
use arborui_ui::{
    KeyAction, KeyModifiers, PointerButton, PointerEvent, PointerEventKind, UiEvent, UiKey,
    UiKeyEvent,
};

/// Translates a normalized terminal event to the backend-neutral UI vocabulary.
///
#[must_use]
pub fn translate_terminal_event(event: TerminalEvent) -> Option<UiEvent> {
    match event {
        TerminalEvent::Key(event) => Some(UiEvent::Key(UiKeyEvent {
            key: match event.code {
                KeyCode::Backspace => UiKey::Backspace,
                KeyCode::Enter => UiKey::Enter,
                KeyCode::Left => UiKey::Left,
                KeyCode::Right => UiKey::Right,
                KeyCode::Up => UiKey::Up,
                KeyCode::Down => UiKey::Down,
                KeyCode::Home => UiKey::Home,
                KeyCode::End => UiKey::End,
                KeyCode::PageUp => UiKey::PageUp,
                KeyCode::PageDown => UiKey::PageDown,
                KeyCode::Tab | KeyCode::BackTab => UiKey::Tab,
                KeyCode::Delete => UiKey::Delete,
                KeyCode::Insert => UiKey::Insert,
                KeyCode::Function(number) => UiKey::Function(number),
                KeyCode::Escape => UiKey::Escape,
                KeyCode::Character(character) => UiKey::Character(character),
                _ => UiKey::Other,
            },
            modifiers: key_modifiers(event.modifiers, event.code == KeyCode::BackTab),
            action: match event.kind {
                KeyEventKind::Press => KeyAction::Press,
                KeyEventKind::Repeat => KeyAction::Repeat,
                KeyEventKind::Release => KeyAction::Release,
            },
        })),
        TerminalEvent::Mouse(event) => {
            let kind = match event.kind {
                MouseEventKind::Down(button) => PointerEventKind::Down(pointer_button(button)),
                MouseEventKind::Up(button) => PointerEventKind::Up(pointer_button(button)),
                MouseEventKind::Drag(button) => PointerEventKind::Drag(pointer_button(button)),
                MouseEventKind::Moved => PointerEventKind::Moved,
                MouseEventKind::ScrollUp => PointerEventKind::Scroll(-1),
                MouseEventKind::ScrollDown => PointerEventKind::Scroll(1),
                MouseEventKind::ScrollLeft => PointerEventKind::ScrollHorizontal(-1),
                MouseEventKind::ScrollRight => PointerEventKind::ScrollHorizontal(1),
            };
            Some(UiEvent::Pointer(PointerEvent {
                kind,
                position: event.position,
                modifiers: key_modifiers(event.modifiers, false),
            }))
        }
        TerminalEvent::Paste(text) => Some(UiEvent::Paste(text)),
        TerminalEvent::Resize(size) => Some(UiEvent::Resize(size)),
        TerminalEvent::FocusGained => Some(UiEvent::TerminalFocusGained),
        TerminalEvent::FocusLost => Some(UiEvent::TerminalFocusLost),
    }
}

fn key_modifiers(modifiers: TerminalModifiers, reverse_tab: bool) -> KeyModifiers {
    let mut translated = KeyModifiers::NONE;
    if modifiers.contains(TerminalModifiers::SHIFT) || reverse_tab {
        translated |= KeyModifiers::SHIFT;
    }
    if modifiers.contains(TerminalModifiers::CONTROL) {
        translated |= KeyModifiers::CONTROL;
    }
    if modifiers.contains(TerminalModifiers::ALT) {
        translated |= KeyModifiers::ALT;
    }
    if modifiers.contains(TerminalModifiers::SUPER) {
        translated |= KeyModifiers::SUPER;
    }
    if modifiers.contains(TerminalModifiers::HYPER) {
        translated |= KeyModifiers::HYPER;
    }
    if modifiers.contains(TerminalModifiers::META) {
        translated |= KeyModifiers::META;
    }
    translated
}

const fn pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
    }
}

#[cfg(test)]
mod tests {
    use arborui_core::{Point, Size};
    use arborui_terminal::{KeyEvent, KeyEventState, MouseEvent};

    use super::*;

    #[test]
    fn translates_terminal_events_without_backend_types() {
        let key = translate_terminal_event(TerminalEvent::Key(KeyEvent {
            code: KeyCode::BackTab,
            modifiers: TerminalModifiers::CONTROL,
            kind: KeyEventKind::Repeat,
            state: KeyEventState::default(),
        }));
        assert_eq!(
            key,
            Some(UiEvent::Key(UiKeyEvent {
                key: UiKey::Tab,
                modifiers: KeyModifiers::SHIFT | KeyModifiers::CONTROL,
                action: KeyAction::Repeat,
            }))
        );

        let pointer = translate_terminal_event(TerminalEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            position: Point::new(3, 4),
            modifiers: TerminalModifiers::ALT,
        }));
        assert_eq!(
            pointer,
            Some(UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Down(PointerButton::Secondary),
                position: Point::new(3, 4),
                modifiers: KeyModifiers::ALT,
            }))
        );
        assert_eq!(
            translate_terminal_event(TerminalEvent::Resize(Size::new(80, 24))),
            Some(UiEvent::Resize(Size::new(80, 24)))
        );
        assert_eq!(
            translate_terminal_event(TerminalEvent::FocusLost),
            Some(UiEvent::TerminalFocusLost)
        );
    }

    #[test]
    fn translates_horizontal_scroll_without_losing_direction() {
        assert_eq!(
            translate_terminal_event(TerminalEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollLeft,
                position: Point::ORIGIN,
                modifiers: TerminalModifiers::NONE,
            })),
            Some(UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::ScrollHorizontal(-1),
                position: Point::ORIGIN,
                modifiers: KeyModifiers::NONE,
            }))
        );
    }
}
