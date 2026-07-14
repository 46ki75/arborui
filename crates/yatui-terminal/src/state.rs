use yatui_core::{CursorState, Point};

/// Terminal screen buffer selection.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ScreenMode {
    /// Use the normal screen and preserve scrollback.
    #[default]
    Main,
    /// Use the alternate screen for a fullscreen application.
    Alternate,
}

/// Mouse reporting state.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum MouseMode {
    /// Do not request mouse events.
    #[default]
    Disabled,
    /// Request button, drag, movement, and scroll events.
    Capture,
}

/// Keyboard protocol state.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum KeyboardMode {
    /// Use legacy terminal key sequences.
    #[default]
    Legacy,
    /// Request progressive keyboard enhancement.
    Enhanced,
}

/// Automatic line-wrap state.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum AutowrapMode {
    /// Leave the terminal's existing setting unchanged.
    #[default]
    Preserve,
    /// Enable automatic line wrapping.
    Enabled,
    /// Disable automatic line wrapping.
    Disabled,
}

/// Complete desired terminal-wide state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalState {
    /// Whether input should use raw mode.
    pub raw_mode: bool,
    /// Main or alternate screen.
    pub screen: ScreenMode,
    /// Desired cursor state.
    pub cursor: CursorState,
    /// Mouse reporting state.
    pub mouse: MouseMode,
    /// Keyboard protocol state.
    pub keyboard: KeyboardMode,
    /// Whether bracketed paste is enabled.
    pub bracketed_paste: bool,
    /// Whether focus changes are reported.
    pub focus_reporting: bool,
    /// Whether frame writes use synchronized updates when supported.
    pub synchronized_updates: bool,
    /// Optional terminal title.
    pub title: Option<String>,
    /// Automatic line-wrap state.
    pub autowrap: AutowrapMode,
}

impl TerminalState {
    /// Creates a conservative fullscreen application state.
    #[must_use]
    pub fn fullscreen() -> Self {
        Self {
            raw_mode: true,
            screen: ScreenMode::Alternate,
            cursor: CursorState::HIDDEN,
            mouse: MouseMode::Disabled,
            keyboard: KeyboardMode::Legacy,
            bracketed_paste: true,
            focus_reporting: true,
            synchronized_updates: true,
            title: None,
            autowrap: AutowrapMode::Disabled,
        }
    }
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            raw_mode: false,
            screen: ScreenMode::Main,
            cursor: CursorState::visible(Point::ORIGIN),
            mouse: MouseMode::Disabled,
            keyboard: KeyboardMode::Legacy,
            bracketed_paste: false,
            focus_reporting: false,
            synchronized_updates: false,
            title: None,
            autowrap: AutowrapMode::Preserve,
        }
    }
}
