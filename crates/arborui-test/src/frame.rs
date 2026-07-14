use std::{fmt, sync::Arc};

use arborui_core::{CursorState, Point, Size, Style};
use arborui_render::{FramePatch, HyperlinkId, PatchCell, PatchCellContent};

/// Resolved content of one cell in a committed test frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TestCellContent {
    /// A visually empty terminal cell.
    Empty,
    /// A complete grapheme in its leading cell.
    Grapheme {
        /// Shared UTF-8 grapheme text.
        text: Arc<str>,
        /// Number of occupied terminal cells.
        width: u16,
    },
    /// A trailing cell occupied by a wide grapheme.
    Continuation {
        /// Cell offset from the leading cell.
        offset: u16,
    },
}

/// Resolved content and styling for one committed terminal cell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestCell {
    /// Resolved cell content.
    pub content: TestCellContent,
    /// Cell style.
    pub style: Style,
    /// Optional hyperlink identity.
    pub hyperlink: Option<HyperlinkId>,
}

impl Default for TestCell {
    fn default() -> Self {
        Self {
            content: TestCellContent::Empty,
            style: Style::default(),
            hyperlink: None,
        }
    }
}

/// Complete resolved frame committed by the in-memory test terminal.
///
/// [`Display`](fmt::Display) produces a character snapshot. The derived
/// [`Debug`](fmt::Debug) representation includes styles and continuation cells
/// for styled-cell snapshots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestFrame {
    size: Size,
    cells: Vec<TestCell>,
    cursor: CursorState,
}

impl TestFrame {
    pub(crate) fn new(size: Size) -> Self {
        Self {
            size,
            cells: vec![TestCell::default(); usize::from(size.width) * usize::from(size.height)],
            cursor: CursorState::default(),
        }
    }

    /// Returns the frame dimensions.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// Returns all cells in row-major order.
    #[must_use]
    pub fn cells(&self) -> &[TestCell] {
        &self.cells
    }

    /// Returns the cell at `point`.
    #[must_use]
    pub fn cell(&self, point: Point) -> Option<&TestCell> {
        self.index(point).and_then(|index| self.cells.get(index))
    }

    /// Returns the committed cursor state.
    #[must_use]
    pub const fn cursor(&self) -> CursorState {
        self.cursor
    }

    /// Returns the visual character snapshot represented by this frame.
    #[must_use]
    pub fn characters(&self) -> String {
        self.to_string()
    }

    pub(crate) fn apply(&mut self, patch: &FramePatch) {
        if self.size != patch.size || patch.full_repaint {
            *self = Self::new(patch.size);
        }
        self.cursor = patch.cursor;

        for run in &patch.runs {
            for (offset, cell) in run.cells.iter().enumerate() {
                let Ok(offset) = i32::try_from(offset) else {
                    continue;
                };
                let point = run.position.translated(offset, 0);
                let Some(index) = self.index(point) else {
                    continue;
                };
                self.cells[index] = TestCell::from(cell);
            }
        }
    }

    fn index(&self, point: Point) -> Option<usize> {
        let x = u16::try_from(point.x).ok()?;
        let y = u16::try_from(point.y).ok()?;
        if x >= self.size.width || y >= self.size.height {
            return None;
        }
        Some(usize::from(y) * usize::from(self.size.width) + usize::from(x))
    }
}

impl From<&PatchCell> for TestCell {
    fn from(cell: &PatchCell) -> Self {
        let content = match &cell.content {
            PatchCellContent::Empty => TestCellContent::Empty,
            PatchCellContent::Grapheme { text, width, .. } => TestCellContent::Grapheme {
                text: Arc::clone(text),
                width: *width,
            },
            PatchCellContent::Continuation { offset, .. } => {
                TestCellContent::Continuation { offset: *offset }
            }
        };
        Self {
            content,
            style: cell.style,
            hyperlink: cell.hyperlink,
        }
    }
}

impl fmt::Display for TestFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.size.height {
            if y != 0 {
                formatter.write_str("\n")?;
            }
            for x in 0..self.size.width {
                let point = Point::new(i32::from(x), i32::from(y));
                let Some(cell) = self.cell(point) else {
                    continue;
                };
                match &cell.content {
                    TestCellContent::Empty => formatter.write_str(" ")?,
                    TestCellContent::Grapheme { text, .. } => formatter.write_str(text)?,
                    TestCellContent::Continuation { .. } => {}
                }
            }
        }
        Ok(())
    }
}
