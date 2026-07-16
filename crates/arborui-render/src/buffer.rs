use std::fmt;

use arborui_core::{Point, Rect, Size, Style};

use crate::{Cell, CellContent, FramePatchValidationError, GraphemeId, HyperlinkId};

/// Errors produced by invariant-preserving buffer writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferError {
    /// A frame patch violated its public cell-run contract.
    InvalidPatch(FramePatchValidationError),
    /// The requested point is outside the buffer.
    OutOfBounds(Point),
    /// A grapheme width was zero.
    ZeroWidth,
    /// The grapheme would extend beyond the current row.
    GraphemeDoesNotFit {
        /// Leading cell requested by the caller.
        point: Point,
        /// Requested display width.
        width: u16,
    },
    /// A parallel buffer or map has incompatible dimensions.
    SizeMismatch {
        /// Required dimensions.
        expected: Size,
        /// Supplied dimensions.
        actual: Size,
    },
}

impl fmt::Display for BufferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPatch(error) => error.fmt(formatter),
            Self::OutOfBounds(point) => write!(
                formatter,
                "point ({}, {}) is outside the buffer",
                point.x, point.y
            ),
            Self::ZeroWidth => {
                formatter.write_str("a rendered grapheme must occupy at least one cell")
            }
            Self::GraphemeDoesNotFit { point, width } => write!(
                formatter,
                "grapheme of width {width} does not fit at ({}, {})",
                point.x, point.y
            ),
            Self::SizeMismatch { expected, actual } => write!(
                formatter,
                "size {}x{} does not match required size {}x{}",
                actual.width, actual.height, expected.width, expected.height
            ),
        }
    }
}

impl std::error::Error for BufferError {}

/// A rectangular grid of grapheme-aware terminal cells.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Buffer {
    size: Size,
    cells: Vec<Cell>,
}

impl Buffer {
    /// Creates an empty buffer of `size`.
    #[must_use]
    pub fn new(size: Size) -> Self {
        Self {
            size,
            cells: vec![Cell::default(); size.area() as usize],
        }
    }

    /// Returns the buffer size.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// Returns buffer bounds in local coordinates.
    #[must_use]
    pub const fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.size.width, self.size.height)
    }

    /// Returns all cells in row-major order.
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Returns the cell at `point`.
    #[must_use]
    pub fn get(&self, point: Point) -> Option<&Cell> {
        self.index(point).map(|index| &self.cells[index])
    }

    /// Clears every cell to an empty cell using `style`.
    pub fn clear(&mut self, style: Style) {
        self.cells.fill(Cell::empty(style));
    }

    /// Sets one cell to empty and clears any grapheme span it intersects.
    pub fn set_empty(&mut self, point: Point, style: Style) -> Result<(), BufferError> {
        let index = self.index(point).ok_or(BufferError::OutOfBounds(point))?;
        self.clear_span_at(point);
        self.cells[index] = Cell::empty(style);
        Ok(())
    }

    /// Writes a complete grapheme span.
    pub fn set_grapheme(
        &mut self,
        point: Point,
        id: GraphemeId,
        width: u16,
        style: Style,
        hyperlink: Option<HyperlinkId>,
    ) -> Result<(), BufferError> {
        if width == 0 {
            return Err(BufferError::ZeroWidth);
        }

        let row_width = i64::from(self.size.width);
        let start = i64::from(point.x);
        let end = start + i64::from(width);
        if point.y < 0 || i64::from(point.y) >= i64::from(self.size.height) || start < 0 {
            return Err(BufferError::OutOfBounds(point));
        }
        if end > row_width {
            return Err(BufferError::GraphemeDoesNotFit { point, width });
        }

        for offset in 0..width {
            self.clear_span_at(Point::new(point.x + i32::from(offset), point.y));
        }

        let start_index = self.index(point).ok_or(BufferError::OutOfBounds(point))?;
        self.cells[start_index] = Cell {
            content: CellContent::Grapheme { id, width },
            style,
            hyperlink,
        };

        for offset in 1..width {
            let continuation = Point::new(point.x + i32::from(offset), point.y);
            let index = self
                .index(continuation)
                .ok_or(BufferError::OutOfBounds(continuation))?;
            self.cells[index] = Cell {
                content: CellContent::Continuation { id, offset },
                style,
                hyperlink,
            };
        }

        Ok(())
    }

    fn index(&self, point: Point) -> Option<usize> {
        let x = u16::try_from(point.x).ok()?;
        let y = u16::try_from(point.y).ok()?;
        if x >= self.size.width || y >= self.size.height {
            return None;
        }
        Some(usize::from(y) * usize::from(self.size.width) + usize::from(x))
    }

    fn clear_span_at(&mut self, point: Point) {
        let Some(index) = self.index(point) else {
            return;
        };

        let (start_x, id, width) = match self.cells[index].content {
            CellContent::Empty => return,
            CellContent::Grapheme { id, width } => (point.x, id, width),
            CellContent::Continuation { id, offset } => {
                let start_x = point.x - i32::from(offset);
                let Some(start) = self.get(Point::new(start_x, point.y)) else {
                    self.cells[index] = Cell::default();
                    return;
                };
                match start.content {
                    CellContent::Grapheme {
                        id: start_id,
                        width,
                    } if start_id == id => (start_x, id, width),
                    _ => {
                        self.cells[index] = Cell::default();
                        return;
                    }
                }
            }
        };

        for offset in 0..width {
            let span_point = Point::new(start_x + i32::from(offset), point.y);
            let Some(span_index) = self.index(span_point) else {
                break;
            };
            let belongs_to_span = match self.cells[span_index].content {
                CellContent::Grapheme {
                    id: span_id,
                    width: span_width,
                } => offset == 0 && span_id == id && span_width == width,
                CellContent::Continuation {
                    id: span_id,
                    offset: span_offset,
                } => span_id == id && span_offset == offset,
                CellContent::Empty => false,
            };
            if belongs_to_span {
                self.cells[span_index] = Cell::default();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwriting_a_continuation_clears_the_complete_span() -> Result<(), BufferError> {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let wide = GraphemeId::from_test_value(1);
        buffer.set_grapheme(Point::new(1, 0), wide, 2, Style::default(), None)?;
        buffer.set_empty(Point::new(2, 0), Style::default())?;

        assert_eq!(buffer.get(Point::new(1, 0)), Some(&Cell::default()));
        assert_eq!(buffer.get(Point::new(2, 0)), Some(&Cell::default()));
        Ok(())
    }
}
