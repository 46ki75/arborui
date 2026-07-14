use yatui_core::Insets;

use crate::Dimension;

/// Main-axis direction for a flex container.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum FlexDirection {
    /// Lay children out from left to right.
    #[default]
    Row,
    /// Lay children out from top to bottom.
    Column,
    /// Lay children out from right to left.
    RowReverse,
    /// Lay children out from bottom to top.
    ColumnReverse,
}

/// Cross-axis alignment of children.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Align {
    /// Align children at the cross-axis start.
    Start,
    /// Center children on the cross axis.
    Center,
    /// Align children at the cross-axis end.
    End,
    /// Stretch auto-sized children across the cross axis.
    #[default]
    Stretch,
}

/// Distribution of children on the main axis.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Justify {
    /// Pack children at the main-axis start.
    #[default]
    Start,
    /// Center children on the main axis.
    Center,
    /// Pack children at the main-axis end.
    End,
    /// Distribute remaining space between children.
    SpaceBetween,
    /// Distribute equal space around children.
    SpaceAround,
    /// Distribute equal space before, between, and after children.
    SpaceEvenly,
}

/// Library-owned flex layout style.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LayoutStyle {
    /// Preferred width.
    pub width: Dimension,
    /// Preferred height.
    pub height: Dimension,
    /// Minimum width.
    pub min_width: Dimension,
    /// Minimum height.
    pub min_height: Dimension,
    /// Maximum width.
    pub max_width: Dimension,
    /// Maximum height.
    pub max_height: Dimension,
    /// Main-axis direction.
    pub direction: FlexDirection,
    /// Cross-axis alignment.
    pub align: Align,
    /// Main-axis distribution.
    pub justify: Justify,
    /// Weight used to consume positive free space.
    pub flex_grow: u16,
    /// Weight used to relinquish space when overflowing.
    pub flex_shrink: u16,
    /// Space between adjacent children.
    pub gap: u16,
    /// Inner spacing around content.
    pub padding: Insets,
    /// Border thickness around padding and content.
    pub border: Insets,
}

impl LayoutStyle {
    /// Creates the default row flex style.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            width: Dimension::Auto,
            height: Dimension::Auto,
            min_width: Dimension::Auto,
            min_height: Dimension::Auto,
            max_width: Dimension::Auto,
            max_height: Dimension::Auto,
            direction: FlexDirection::Row,
            align: Align::Stretch,
            justify: Justify::Start,
            flex_grow: 0,
            flex_shrink: 1,
            gap: 0,
            padding: Insets::all(0),
            border: Insets::all(0),
        }
    }

    /// Sets the preferred width and height.
    #[must_use]
    pub const fn size(mut self, width: Dimension, height: Dimension) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Sets the main-axis direction.
    #[must_use]
    pub const fn direction(mut self, direction: FlexDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Sets padding on every edge.
    #[must_use]
    pub const fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// Sets border thickness on every edge.
    #[must_use]
    pub const fn border(mut self, border: Insets) -> Self {
        self.border = border;
        self
    }

    /// Sets positive and negative free-space weights.
    #[must_use]
    pub const fn flex(mut self, grow: u16, shrink: u16) -> Self {
        self.flex_grow = grow;
        self.flex_shrink = shrink;
        self
    }
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self::new()
    }
}
