use crossterm::event as ct;
use yatui_core::{Point, Size};
use yatui_terminal::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind, TerminalEvent,
};

pub(crate) fn translate_event(event: ct::Event) -> TerminalEvent {
    match event {
        ct::Event::FocusGained => TerminalEvent::FocusGained,
        ct::Event::FocusLost => TerminalEvent::FocusLost,
        ct::Event::Key(event) => TerminalEvent::Key(KeyEvent {
            code: translate_key_code(event.code),
            modifiers: translate_modifiers(event.modifiers),
            kind: match event.kind {
                ct::KeyEventKind::Press => KeyEventKind::Press,
                ct::KeyEventKind::Repeat => KeyEventKind::Repeat,
                ct::KeyEventKind::Release => KeyEventKind::Release,
            },
            state: KeyEventState {
                keypad: event.state.contains(ct::KeyEventState::KEYPAD),
                caps_lock: event.state.contains(ct::KeyEventState::CAPS_LOCK),
                num_lock: event.state.contains(ct::KeyEventState::NUM_LOCK),
            },
        }),
        ct::Event::Mouse(event) => TerminalEvent::Mouse(MouseEvent {
            kind: match event.kind {
                ct::MouseEventKind::Down(button) => {
                    MouseEventKind::Down(translate_mouse_button(button))
                }
                ct::MouseEventKind::Up(button) => {
                    MouseEventKind::Up(translate_mouse_button(button))
                }
                ct::MouseEventKind::Drag(button) => {
                    MouseEventKind::Drag(translate_mouse_button(button))
                }
                ct::MouseEventKind::Moved => MouseEventKind::Moved,
                ct::MouseEventKind::ScrollDown => MouseEventKind::ScrollDown,
                ct::MouseEventKind::ScrollUp => MouseEventKind::ScrollUp,
                ct::MouseEventKind::ScrollLeft => MouseEventKind::ScrollLeft,
                ct::MouseEventKind::ScrollRight => MouseEventKind::ScrollRight,
            },
            position: Point::new(i32::from(event.column), i32::from(event.row)),
            modifiers: translate_modifiers(event.modifiers),
        }),
        ct::Event::Paste(text) => TerminalEvent::Paste(text),
        ct::Event::Resize(width, height) => TerminalEvent::Resize(Size::new(width, height)),
    }
}

fn translate_key_code(code: ct::KeyCode) -> KeyCode {
    match code {
        ct::KeyCode::Backspace => KeyCode::Backspace,
        ct::KeyCode::Enter => KeyCode::Enter,
        ct::KeyCode::Left => KeyCode::Left,
        ct::KeyCode::Right => KeyCode::Right,
        ct::KeyCode::Up => KeyCode::Up,
        ct::KeyCode::Down => KeyCode::Down,
        ct::KeyCode::Home => KeyCode::Home,
        ct::KeyCode::End => KeyCode::End,
        ct::KeyCode::PageUp => KeyCode::PageUp,
        ct::KeyCode::PageDown => KeyCode::PageDown,
        ct::KeyCode::Tab => KeyCode::Tab,
        ct::KeyCode::BackTab => KeyCode::BackTab,
        ct::KeyCode::Delete => KeyCode::Delete,
        ct::KeyCode::Insert => KeyCode::Insert,
        ct::KeyCode::F(number) => KeyCode::Function(number),
        ct::KeyCode::Char(character) => KeyCode::Character(character),
        ct::KeyCode::Null => KeyCode::Null,
        ct::KeyCode::Esc => KeyCode::Escape,
        ct::KeyCode::CapsLock => KeyCode::CapsLock,
        ct::KeyCode::ScrollLock => KeyCode::ScrollLock,
        ct::KeyCode::NumLock => KeyCode::NumLock,
        ct::KeyCode::PrintScreen => KeyCode::PrintScreen,
        ct::KeyCode::Pause => KeyCode::Pause,
        ct::KeyCode::Menu => KeyCode::Menu,
        ct::KeyCode::KeypadBegin => KeyCode::KeypadBegin,
        ct::KeyCode::Media(_) | ct::KeyCode::Modifier(_) => KeyCode::Unknown,
    }
}

fn translate_modifiers(modifiers: ct::KeyModifiers) -> KeyModifiers {
    let mut translated = KeyModifiers::NONE;
    if modifiers.contains(ct::KeyModifiers::SHIFT) {
        translated |= KeyModifiers::SHIFT;
    }
    if modifiers.contains(ct::KeyModifiers::CONTROL) {
        translated |= KeyModifiers::CONTROL;
    }
    if modifiers.contains(ct::KeyModifiers::ALT) {
        translated |= KeyModifiers::ALT;
    }
    if modifiers.contains(ct::KeyModifiers::SUPER) {
        translated |= KeyModifiers::SUPER;
    }
    if modifiers.contains(ct::KeyModifiers::HYPER) {
        translated |= KeyModifiers::HYPER;
    }
    if modifiers.contains(ct::KeyModifiers::META) {
        translated |= KeyModifiers::META;
    }
    translated
}

const fn translate_mouse_button(button: ct::MouseButton) -> MouseButton {
    match button {
        ct::MouseButton::Left => MouseButton::Left,
        ct::MouseButton::Right => MouseButton::Right,
        ct::MouseButton::Middle => MouseButton::Middle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_enhanced_key_details() {
        let event = ct::KeyEvent {
            code: ct::KeyCode::Char('x'),
            modifiers: ct::KeyModifiers::CONTROL | ct::KeyModifiers::SHIFT,
            kind: ct::KeyEventKind::Repeat,
            state: ct::KeyEventState::CAPS_LOCK,
        };

        let TerminalEvent::Key(event) = translate_event(ct::Event::Key(event)) else {
            panic!("expected key event");
        };
        assert_eq!(event.code, KeyCode::Character('x'));
        assert_eq!(event.kind, KeyEventKind::Repeat);
        assert!(event.modifiers.contains(KeyModifiers::CONTROL));
        assert!(event.modifiers.contains(KeyModifiers::SHIFT));
        assert!(event.state.caps_lock);
    }

    #[test]
    fn translates_mouse_coordinates_and_action() {
        let event = ct::MouseEvent {
            kind: ct::MouseEventKind::Drag(ct::MouseButton::Left),
            column: 12,
            row: 4,
            modifiers: ct::KeyModifiers::ALT,
        };

        let TerminalEvent::Mouse(event) = translate_event(ct::Event::Mouse(event)) else {
            panic!("expected mouse event");
        };
        assert_eq!(event.position, Point::new(12, 4));
        assert_eq!(event.kind, MouseEventKind::Drag(MouseButton::Left));
        assert!(event.modifiers.contains(KeyModifiers::ALT));
    }
}
