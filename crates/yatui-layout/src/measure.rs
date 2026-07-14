/// Space available to a measured leaf along one axis.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AvailableSpace {
    /// An exact upper bound in terminal cells.
    Definite(u16),
    /// The minimum size that does not overflow avoidably.
    MinContent,
    /// The preferred unconstrained size.
    MaxContent,
}

/// Constraints passed to a leaf measurement callback.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MeasureInput {
    /// Width fixed by layout, when known.
    pub known_width: Option<u16>,
    /// Height fixed by layout, when known.
    pub known_height: Option<u16>,
    /// Width available to the leaf.
    pub available_width: AvailableSpace,
    /// Height available to the leaf.
    pub available_height: AvailableSpace,
}
