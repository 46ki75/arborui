/// A point in terminal-cell coordinates.
///
/// Signed coordinates permit partially off-screen surfaces without requiring
/// lossy conversions at rendering boundaries.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: i32,
    /// Vertical coordinate.
    pub y: i32,
}

impl Point {
    /// The coordinate-system origin.
    pub const ORIGIN: Self = Self { x: 0, y: 0 };

    /// Creates a point.
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Translates the point, saturating at the coordinate limits.
    #[must_use]
    pub const fn translated(self, x: i32, y: i32) -> Self {
        Self {
            x: self.x.saturating_add(x),
            y: self.y.saturating_add(y),
        }
    }
}

/// A width and height measured in terminal cells.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Size {
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
}

impl Size {
    /// A size with no area.
    pub const ZERO: Self = Self {
        width: 0,
        height: 0,
    };

    /// Creates a size.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    /// Returns whether either dimension is zero.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Returns the number of cells in the size.
    #[must_use]
    pub const fn area(self) -> u32 {
        self.width as u32 * self.height as u32
    }
}

/// Insets applied to the four edges of a rectangle.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Insets {
    /// Top inset.
    pub top: u16,
    /// Right inset.
    pub right: u16,
    /// Bottom inset.
    pub bottom: u16,
    /// Left inset.
    pub left: u16,
}

impl Insets {
    /// Creates insets in top, right, bottom, left order.
    #[must_use]
    pub const fn new(top: u16, right: u16, bottom: u16, left: u16) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Creates equal insets on every edge.
    #[must_use]
    pub const fn all(value: u16) -> Self {
        Self::new(value, value, value, value)
    }

    /// Creates vertical and horizontal insets.
    #[must_use]
    pub const fn symmetric(vertical: u16, horizontal: u16) -> Self {
        Self::new(vertical, horizontal, vertical, horizontal)
    }
}

/// A rectangular region in terminal-cell coordinates.
///
/// Rectangles are half-open: the right and bottom edges are excluded.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Rect {
    /// Horizontal coordinate of the leading edge.
    pub x: i32,
    /// Vertical coordinate of the top edge.
    pub y: i32,
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
}

impl Rect {
    /// An empty rectangle at the origin.
    pub const ZERO: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    /// Creates a rectangle.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Creates a rectangle from an origin and size.
    #[must_use]
    pub const fn from_origin_size(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.width, size.height)
    }

    /// Returns the rectangle origin.
    #[must_use]
    pub const fn origin(self) -> Point {
        Point::new(self.x, self.y)
    }

    /// Returns the rectangle size.
    #[must_use]
    pub const fn size(self) -> Size {
        Size::new(self.width, self.height)
    }

    /// Returns whether the rectangle has no area.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Returns the excluded right edge, saturating at the coordinate limit.
    #[must_use]
    pub const fn right(self) -> i32 {
        self.x.saturating_add(self.width as i32)
    }

    /// Returns the excluded bottom edge, saturating at the coordinate limit.
    #[must_use]
    pub const fn bottom(self) -> i32 {
        self.y.saturating_add(self.height as i32)
    }

    /// Returns whether the point is inside the half-open rectangle.
    #[must_use]
    pub fn contains(self, point: Point) -> bool {
        let x = i64::from(point.x);
        let y = i64::from(point.y);
        let left = i64::from(self.x);
        let top = i64::from(self.y);
        let right = left + i64::from(self.width);
        let bottom = top + i64::from(self.height);

        x >= left && x < right && y >= top && y < bottom
    }

    /// Returns the overlapping region of two non-empty rectangles.
    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let left = i64::from(self.x).max(i64::from(other.x));
        let top = i64::from(self.y).max(i64::from(other.y));
        let right = (i64::from(self.x) + i64::from(self.width))
            .min(i64::from(other.x) + i64::from(other.width));
        let bottom = (i64::from(self.y) + i64::from(self.height))
            .min(i64::from(other.y) + i64::from(other.height));

        if left >= right || top >= bottom {
            return None;
        }

        Some(Self::new(
            saturating_i64_to_i32(left),
            saturating_i64_to_i32(top),
            (right - left) as u16,
            (bottom - top) as u16,
        ))
    }

    /// Returns whether this rectangle overlaps `other` with non-zero area.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.intersection(other).is_some()
    }

    /// Translates the rectangle, saturating at the coordinate limits.
    #[must_use]
    pub const fn translated(self, x: i32, y: i32) -> Self {
        Self::new(
            self.x.saturating_add(x),
            self.y.saturating_add(y),
            self.width,
            self.height,
        )
    }

    /// Returns the region left after applying `insets` without underflow.
    #[must_use]
    pub const fn inner(self, insets: Insets) -> Self {
        let left = min_u16(insets.left, self.width);
        let remaining_width = self.width - left;
        let right = min_u16(insets.right, remaining_width);
        let top = min_u16(insets.top, self.height);
        let remaining_height = self.height - top;
        let bottom = min_u16(insets.bottom, remaining_height);

        Self::new(
            self.x.saturating_add(left as i32),
            self.y.saturating_add(top as i32),
            remaining_width - right,
            remaining_height - bottom,
        )
    }
}

const fn min_u16(left: u16, right: u16) -> u16 {
    if left < right { left } else { right }
}

fn saturating_i64_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_area_uses_a_wider_integer() {
        assert_eq!(Size::new(u16::MAX, u16::MAX).area(), 4_294_836_225);
    }

    #[test]
    fn rectangle_contains_only_half_open_region() {
        let rect = Rect::new(-2, 3, 4, 2);

        assert!(rect.contains(Point::new(-2, 3)));
        assert!(rect.contains(Point::new(1, 4)));
        assert!(!rect.contains(Point::new(2, 4)));
        assert!(!rect.contains(Point::new(1, 5)));
    }

    #[test]
    fn rectangle_intersection_excludes_touching_edges() {
        let left = Rect::new(0, 0, 3, 3);

        assert_eq!(
            left.intersection(Rect::new(2, 1, 3, 3)),
            Some(Rect::new(2, 1, 1, 2))
        );
        assert_eq!(left.intersection(Rect::new(3, 0, 2, 2)), None);
    }

    #[test]
    fn inner_rectangle_saturates_oversized_insets() {
        let rect = Rect::new(5, 7, 4, 3);

        assert_eq!(rect.inner(Insets::new(2, 4, 5, 3)), Rect::new(8, 9, 0, 0));
    }
}
