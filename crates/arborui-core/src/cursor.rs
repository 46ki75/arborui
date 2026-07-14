use crate::Point;

/// The shape used to display a visible terminal cursor.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CursorShape {
    /// A cursor covering the current cell.
    #[default]
    Block,
    /// A horizontal line at the bottom of the current cell.
    Underline,
    /// A vertical line at the leading edge of the current cell.
    Bar,
}

/// Whether the terminal cursor is visible.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CursorVisibility {
    /// The cursor is hidden.
    #[default]
    Hidden,
    /// The cursor is visible.
    Visible,
}

/// Desired visual state for the terminal cursor.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct CursorState {
    /// Cursor position in viewport coordinates.
    pub position: Point,
    /// Cursor visibility.
    pub visibility: CursorVisibility,
    /// Cursor shape.
    pub shape: CursorShape,
    /// Whether the cursor should blink when supported.
    pub blinking: bool,
}

impl CursorState {
    /// A hidden cursor at the origin.
    pub const HIDDEN: Self = Self {
        position: Point::ORIGIN,
        visibility: CursorVisibility::Hidden,
        shape: CursorShape::Block,
        blinking: false,
    };

    /// Creates a visible, non-blinking block cursor at `position`.
    #[must_use]
    pub const fn visible(position: Point) -> Self {
        Self {
            position,
            visibility: CursorVisibility::Visible,
            shape: CursorShape::Block,
            blinking: false,
        }
    }

    /// Returns this state with the requested cursor shape.
    #[must_use]
    pub const fn with_shape(mut self, shape: CursorShape) -> Self {
        self.shape = shape;
        self
    }

    /// Returns this state with blinking enabled or disabled.
    #[must_use]
    pub const fn with_blinking(mut self, blinking: bool) -> Self {
        self.blinking = blinking;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_is_hidden_by_default() {
        assert_eq!(CursorState::default(), CursorState::HIDDEN);
    }

    #[test]
    fn visible_cursor_can_be_configured() {
        let cursor = CursorState::visible(Point::new(2, 3))
            .with_shape(CursorShape::Bar)
            .with_blinking(true);

        assert_eq!(cursor.visibility, CursorVisibility::Visible);
        assert_eq!(cursor.shape, CursorShape::Bar);
        assert!(cursor.blinking);
    }
}
