use arborui_core::{Point, Rect, Size};

/// Renderer-neutral identity associated with an interactive painted cell.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HitId(u64);

impl HitId {
    /// Creates an identity from a caller-owned value.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the caller-owned value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Interactive identity for each cell in one logical frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HitMap {
    size: Size,
    cells: Vec<Option<HitId>>,
}

impl HitMap {
    /// Creates an empty map for `size`.
    #[must_use]
    pub fn new(size: Size) -> Self {
        Self {
            size,
            cells: vec![None; size.area() as usize],
        }
    }

    /// Returns the map dimensions.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// Returns the identity at a viewport position.
    #[must_use]
    pub fn get(&self, point: Point) -> Option<HitId> {
        self.index(point).and_then(|index| self.cells[index])
    }

    /// Associates one in-bounds cell with an identity.
    pub fn set(&mut self, point: Point, hit: HitId) -> bool {
        self.set_option(point, Some(hit))
    }

    /// Replaces one in-bounds cell, including clearing an existing identity.
    pub fn set_option(&mut self, point: Point, hit: Option<HitId>) -> bool {
        let Some(index) = self.index(point) else {
            return false;
        };
        self.cells[index] = hit;
        true
    }

    /// Clears one in-bounds cell.
    pub fn clear(&mut self, point: Point) -> bool {
        self.set_option(point, None)
    }

    /// Associates the in-bounds portion of a rectangle with an identity.
    pub fn fill(&mut self, rect: Rect, hit: HitId) {
        let bounds = Rect::new(0, 0, self.size.width, self.size.height);
        let Some(rect) = rect.intersection(bounds) else {
            return;
        };
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let _ = self.set(Point::new(x, y), hit);
            }
        }
    }

    fn index(&self, point: Point) -> Option<usize> {
        let x = usize::try_from(point.x).ok()?;
        let y = usize::try_from(point.y).ok()?;
        if x >= usize::from(self.size.width) || y >= usize::from(self.size.height) {
            return None;
        }
        Some(y * usize::from(self.size.width) + x)
    }
}

impl Default for HitMap {
    fn default() -> Self {
        Self::new(Size::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_is_clipped_to_map_bounds() {
        let mut map = HitMap::new(Size::new(2, 2));
        map.fill(Rect::new(-1, 1, 3, 2), HitId::new(7));

        assert_eq!(map.get(Point::new(0, 1)), Some(HitId::new(7)));
        assert_eq!(map.get(Point::new(1, 1)), Some(HitId::new(7)));
        assert_eq!(map.get(Point::new(0, 0)), None);
    }
}
