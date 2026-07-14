use std::ops::{BitOr, BitOrAssign};

use arborui_core::{Point, Size};

/// A normalized keyboard key.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KeyCode {
    /// Backspace.
    Backspace,
    /// Enter or return.
    Enter,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Tab.
    Tab,
    /// Reverse tab.
    BackTab,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Function key.
    Function(u8),
    /// Unicode character.
    Character(char),
    /// Null key code.
    Null,
    /// Escape.
    Escape,
    /// Caps Lock.
    CapsLock,
    /// Scroll Lock.
    ScrollLock,
    /// Num Lock.
    NumLock,
    /// Print Screen.
    PrintScreen,
    /// Pause.
    Pause,
    /// Menu or application key.
    Menu,
    /// Keypad begin or center key.
    KeypadBegin,
    /// A backend key without a portable representation.
    Unknown,
}

/// Modifier keys active for a keyboard or mouse event.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// Shift modifier.
    pub const SHIFT: Self = Self(1 << 0);
    /// Control modifier.
    pub const CONTROL: Self = Self(1 << 1);
    /// Alt modifier.
    pub const ALT: Self = Self(1 << 2);
    /// Super modifier.
    pub const SUPER: Self = Self(1 << 3);
    /// Hyper modifier.
    pub const HYPER: Self = Self(1 << 4);
    /// Meta modifier.
    pub const META: Self = Self(1 << 5);

    /// Returns whether every modifier in `other` is present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns whether no modifiers are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for KeyModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Keyboard event phase.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum KeyEventKind {
    /// Initial key press.
    #[default]
    Press,
    /// Automatic or protocol-reported repeat.
    Repeat,
    /// Key release.
    Release,
}

/// Additional keyboard state reported by enhanced protocols.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct KeyEventState {
    /// The event came from the numeric keypad.
    pub keypad: bool,
    /// Caps Lock was active.
    pub caps_lock: bool,
    /// Num Lock was active.
    pub num_lock: bool,
}

/// A normalized keyboard event.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct KeyEvent {
    /// Key code.
    pub code: KeyCode,
    /// Active modifier keys.
    pub modifiers: KeyModifiers,
    /// Press, repeat, or release phase.
    pub kind: KeyEventKind,
    /// Additional enhanced-protocol state.
    pub state: KeyEventState,
}

/// A normalized mouse button.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Right button.
    Right,
    /// Middle button.
    Middle,
}

/// A normalized mouse action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MouseEventKind {
    /// Button press.
    Down(MouseButton),
    /// Button release.
    Up(MouseButton),
    /// Pointer movement with a pressed button.
    Drag(MouseButton),
    /// Pointer movement without a pressed button.
    Moved,
    /// Vertical scroll away from the user.
    ScrollUp,
    /// Vertical scroll toward the user.
    ScrollDown,
    /// Horizontal scroll left.
    ScrollLeft,
    /// Horizontal scroll right.
    ScrollRight,
}

/// A normalized mouse event.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MouseEvent {
    /// Mouse action.
    pub kind: MouseEventKind,
    /// Pointer position in viewport coordinates.
    pub position: Point,
    /// Active modifier keys.
    pub modifiers: KeyModifiers,
}

/// Input or lifecycle event emitted by a terminal backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalEvent {
    /// Keyboard event.
    Key(KeyEvent),
    /// Mouse event.
    Mouse(MouseEvent),
    /// Complete bracketed-paste payload.
    Paste(String),
    /// Viewport resize.
    Resize(Size),
    /// Terminal window gained focus.
    FocusGained,
    /// Terminal window lost focus.
    FocusLost,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_modifiers_compose_without_backend_types() {
        let modifiers = KeyModifiers::CONTROL | KeyModifiers::SHIFT;

        assert!(modifiers.contains(KeyModifiers::CONTROL));
        assert!(modifiers.contains(KeyModifiers::SHIFT));
        assert!(!modifiers.contains(KeyModifiers::ALT));
    }
}
