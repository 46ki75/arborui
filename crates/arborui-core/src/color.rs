/// A terminal-independent color value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Color {
    /// Restore the terminal's default color.
    Reset,
    /// Black from the terminal's ANSI palette.
    Black,
    /// Red from the terminal's ANSI palette.
    Red,
    /// Green from the terminal's ANSI palette.
    Green,
    /// Yellow from the terminal's ANSI palette.
    Yellow,
    /// Blue from the terminal's ANSI palette.
    Blue,
    /// Magenta from the terminal's ANSI palette.
    Magenta,
    /// Cyan from the terminal's ANSI palette.
    Cyan,
    /// White from the terminal's ANSI palette.
    White,
    /// Bright black from the terminal's ANSI palette.
    BrightBlack,
    /// Bright red from the terminal's ANSI palette.
    BrightRed,
    /// Bright green from the terminal's ANSI palette.
    BrightGreen,
    /// Bright yellow from the terminal's ANSI palette.
    BrightYellow,
    /// Bright blue from the terminal's ANSI palette.
    BrightBlue,
    /// Bright magenta from the terminal's ANSI palette.
    BrightMagenta,
    /// Bright cyan from the terminal's ANSI palette.
    BrightCyan,
    /// Bright white from the terminal's ANSI palette.
    BrightWhite,
    /// A color from the terminal's 256-color palette.
    Indexed(u8),
    /// A 24-bit red, green, and blue color.
    Rgb {
        /// Red channel.
        red: u8,
        /// Green channel.
        green: u8,
        /// Blue channel.
        blue: u8,
    },
}

impl Color {
    /// Creates a 24-bit color from red, green, and blue channels.
    #[must_use]
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::Rgb { red, green, blue }
    }
}
