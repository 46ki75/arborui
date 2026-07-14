use arborui_text::WidthPolicy;

/// Terminal color depth.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ColorCapability {
    /// The standard 16 ANSI colors.
    #[default]
    Ansi16,
    /// The indexed 256-color palette.
    Ansi256,
    /// 24-bit red, green, and blue colors.
    TrueColor,
}

/// Keyboard protocol capability.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum KeyboardCapability {
    /// Legacy terminal key sequences only.
    #[default]
    Legacy,
    /// Kitty progressive keyboard enhancement is supported.
    Enhanced,
}

/// Mouse protocol capability.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum MouseCapability {
    /// Mouse reporting is unavailable.
    None,
    /// SGR mouse reporting is available.
    #[default]
    Sgr,
}

/// Terminal features detected or configured by a backend.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Capabilities {
    /// Supported color depth.
    pub color: ColorCapability,
    /// Supported keyboard protocol.
    pub keyboard: KeyboardCapability,
    /// Supported mouse protocol.
    pub mouse: MouseCapability,
    /// Whether synchronized output updates are supported.
    pub synchronized_updates: bool,
    /// Whether bracketed paste is supported.
    pub bracketed_paste: bool,
    /// Whether terminal focus reporting is supported.
    pub focus_reporting: bool,
    /// Whether OSC 8 hyperlinks are supported.
    pub hyperlinks: bool,
    /// Whether explicit grapheme width sequences are supported.
    pub explicit_width: bool,
    /// Width policy selected for this terminal.
    pub width_policy: WidthPolicy,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            color: ColorCapability::Ansi16,
            keyboard: KeyboardCapability::Legacy,
            mouse: MouseCapability::Sgr,
            synchronized_updates: false,
            bracketed_paste: true,
            focus_reporting: true,
            hyperlinks: false,
            explicit_width: false,
            width_policy: WidthPolicy::Unicode,
        }
    }
}
