use arborui_core::Style;

use crate::GraphemeId;

/// Stable identity for a hyperlink stored by a future terminal layer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HyperlinkId(u32);

impl HyperlinkId {
    /// Creates an identity from its numeric representation.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the numeric representation.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Content occupying one terminal cell.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CellContent {
    /// A visually empty cell.
    #[default]
    Empty,
    /// The leading cell of a grapheme cluster.
    Grapheme {
        /// Interned grapheme identity.
        id: GraphemeId,
        /// Number of cells occupied by the grapheme.
        width: u16,
    },
    /// A non-leading cell occupied by a wider grapheme.
    Continuation {
        /// Interned grapheme identity.
        id: GraphemeId,
        /// Cell offset from the leading cell.
        offset: u16,
    },
}

/// Visual state for one terminal cell.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Cell {
    /// Content occupying the cell.
    pub content: CellContent,
    /// Cell style.
    pub style: Style,
    /// Optional hyperlink identity.
    pub hyperlink: Option<HyperlinkId>,
}

impl Cell {
    /// Creates an empty cell with `style`.
    #[must_use]
    pub const fn empty(style: Style) -> Self {
        Self {
            content: CellContent::Empty,
            style,
            hyperlink: None,
        }
    }
}
