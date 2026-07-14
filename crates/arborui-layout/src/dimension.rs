/// A size along one layout axis.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Dimension {
    /// Let content and the layout algorithm determine the size.
    #[default]
    Auto,
    /// An exact number of terminal cells.
    Cells(u16),
    /// A percentage of the containing block, where `100` is the full size.
    Percent(u16),
}

impl Dimension {
    /// Creates an exact cell dimension.
    #[must_use]
    pub const fn cells(value: u16) -> Self {
        Self::Cells(value)
    }

    /// Creates a percentage dimension.
    #[must_use]
    pub const fn percent(value: u16) -> Self {
        Self::Percent(value)
    }
}
