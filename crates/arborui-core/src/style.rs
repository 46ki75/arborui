use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

use crate::Color;

/// A set of terminal text modifiers.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Modifier(u16);

impl Modifier {
    /// No modifiers.
    pub const EMPTY: Self = Self(0);
    /// Increased text intensity.
    pub const BOLD: Self = Self(1 << 0);
    /// Decreased text intensity.
    pub const DIM: Self = Self(1 << 1);
    /// Italic text.
    pub const ITALIC: Self = Self(1 << 2);
    /// Underlined text.
    pub const UNDERLINED: Self = Self(1 << 3);
    /// Slowly blinking text.
    pub const SLOW_BLINK: Self = Self(1 << 4);
    /// Rapidly blinking text.
    pub const RAPID_BLINK: Self = Self(1 << 5);
    /// Reversed foreground and background colors.
    pub const REVERSED: Self = Self(1 << 6);
    /// Hidden text.
    pub const HIDDEN: Self = Self(1 << 7);
    /// Crossed-out text.
    pub const CROSSED_OUT: Self = Self(1 << 8);

    /// Returns whether this set contains every modifier in `other`.
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

impl BitOr for Modifier {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Modifier {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for Modifier {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for Modifier {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl Not for Modifier {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

/// Visual styling applied to terminal cells.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Style {
    /// Foreground color, or `None` to inherit the current value.
    pub foreground: Option<Color>,
    /// Background color, or `None` to inherit the current value.
    pub background: Option<Color>,
    /// Underline color, or `None` to inherit the current value.
    pub underline_color: Option<Color>,
    /// Enabled text modifiers.
    pub modifiers: Modifier,
}

impl Style {
    /// A style that inherits every value.
    pub const DEFAULT: Self = Self {
        foreground: None,
        background: None,
        underline_color: None,
        modifiers: Modifier::EMPTY,
    };

    /// Creates a style that inherits every value.
    #[must_use]
    pub const fn new() -> Self {
        Self::DEFAULT
    }

    /// Returns this style with a foreground color.
    #[must_use]
    pub const fn foreground(mut self, color: Color) -> Self {
        self.foreground = Some(color);
        self
    }

    /// Returns this style with a background color.
    #[must_use]
    pub const fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Returns this style with an underline color.
    #[must_use]
    pub const fn underline_color(mut self, color: Color) -> Self {
        self.underline_color = Some(color);
        self
    }

    /// Returns this style with the provided modifiers enabled.
    #[must_use]
    pub fn add_modifiers(mut self, modifiers: Modifier) -> Self {
        self.modifiers |= modifiers;
        self
    }

    /// Returns this style with the provided modifiers disabled.
    #[must_use]
    pub fn remove_modifiers(mut self, modifiers: Modifier) -> Self {
        self.modifiers &= !modifiers;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_builder_preserves_existing_values() {
        let style = Style::new()
            .foreground(Color::BrightCyan)
            .background(Color::rgb(1, 2, 3))
            .add_modifiers(Modifier::BOLD | Modifier::UNDERLINED)
            .remove_modifiers(Modifier::BOLD);

        assert_eq!(style.foreground, Some(Color::BrightCyan));
        assert_eq!(style.background, Some(Color::rgb(1, 2, 3)));
        assert!(!style.modifiers.contains(Modifier::BOLD));
        assert!(style.modifiers.contains(Modifier::UNDERLINED));
    }
}
