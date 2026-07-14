use std::fmt;

use yatui_core::{Point, Rect, Style};
use yatui_text::{WidthPolicy, graphemes};

use crate::{Buffer, BufferError, GraphemeStore, GraphemeStoreError, HyperlinkId};

/// Errors produced by high-level drawing operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DrawError {
    /// A value expected to contain one grapheme did not.
    InvalidGrapheme,
    /// A measured grapheme width cannot be represented by the renderer.
    WidthExceeded(usize),
    /// Grapheme interning failed.
    GraphemeStore(GraphemeStoreError),
    /// An invariant-preserving buffer write failed.
    Buffer(BufferError),
}

impl fmt::Display for DrawError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGrapheme => formatter.write_str("value is not one drawable grapheme"),
            Self::WidthExceeded(width) => {
                write!(formatter, "grapheme width {width} exceeds renderer limits")
            }
            Self::GraphemeStore(error) => error.fmt(formatter),
            Self::Buffer(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DrawError {}

impl From<GraphemeStoreError> for DrawError {
    fn from(error: GraphemeStoreError) -> Self {
        Self::GraphemeStore(error)
    }
}

impl From<BufferError> for DrawError {
    fn from(error: BufferError) -> Self {
        Self::Buffer(error)
    }
}

/// Summary of a text drawing operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextDraw {
    /// Number of graphemes written to the buffer.
    pub graphemes: usize,
    /// Number of terminal cells written to the buffer.
    pub cells: usize,
}

/// A clipped drawing view over a buffer and grapheme store.
pub struct Canvas<'a> {
    buffer: &'a mut Buffer,
    store: &'a mut GraphemeStore,
    clip: Rect,
    origin: Point,
    width_policy: WidthPolicy,
}

impl<'a> Canvas<'a> {
    /// Creates a canvas covering the complete buffer.
    pub fn new(
        buffer: &'a mut Buffer,
        store: &'a mut GraphemeStore,
        width_policy: WidthPolicy,
    ) -> Self {
        let clip = buffer.bounds();
        Self {
            buffer,
            store,
            clip,
            origin: Point::ORIGIN,
            width_policy,
        }
    }

    /// Returns the active clip rectangle in buffer coordinates.
    #[must_use]
    pub const fn clip(&self) -> Rect {
        self.clip
    }

    /// Returns the local-coordinate origin in buffer coordinates.
    #[must_use]
    pub const fn origin(&self) -> Point {
        self.origin
    }

    /// Restricts this canvas to `clip` in buffer coordinates.
    #[must_use]
    pub fn with_clip(mut self, clip: Rect) -> Self {
        self.clip = self.clip.intersection(clip).unwrap_or(Rect::ZERO);
        self
    }

    /// Translates local drawing coordinates.
    #[must_use]
    pub fn with_origin(mut self, origin: Point) -> Self {
        self.origin = origin;
        self
    }

    /// Creates a shorter-lived canvas with an additional clip and local origin.
    pub fn scoped(&mut self, clip: Rect, origin: Point) -> Canvas<'_> {
        Canvas {
            buffer: &mut *self.buffer,
            store: &mut *self.store,
            clip: self.clip.intersection(clip).unwrap_or(Rect::ZERO),
            origin,
            width_policy: self.width_policy,
        }
    }

    /// Draws exactly one grapheme, returning whether it was fully visible.
    pub fn draw_grapheme(
        &mut self,
        point: Point,
        value: &str,
        style: Style,
        hyperlink: Option<HyperlinkId>,
    ) -> Result<bool, DrawError> {
        let mut clusters = graphemes(value, self.width_policy);
        let Some(grapheme) = clusters.next() else {
            return Err(DrawError::InvalidGrapheme);
        };
        if clusters.next().is_some() || grapheme.width == 0 {
            return Err(DrawError::InvalidGrapheme);
        }

        let width =
            u16::try_from(grapheme.width).map_err(|_| DrawError::WidthExceeded(grapheme.width))?;
        let point = self.origin.translated(point.x, point.y);
        if !span_fits(point, width, self.clip) || !span_fits(point, width, self.buffer.bounds()) {
            return Ok(false);
        }

        let id = self.store.intern(value)?;
        self.buffer
            .set_grapheme(point, id, width, style, hyperlink)?;
        Ok(true)
    }

    /// Draws text with CR, LF, and CRLF interpreted as line separators.
    pub fn draw_text(
        &mut self,
        point: Point,
        text: &str,
        style: Style,
        hyperlink: Option<HyperlinkId>,
    ) -> Result<TextDraw, DrawError> {
        let mut cursor = point;
        let line_start = point.x;
        let mut draw = TextDraw::default();

        for grapheme in graphemes(text, self.width_policy) {
            if grapheme
                .text
                .chars()
                .any(|character| matches!(character, '\r' | '\n'))
            {
                cursor.x = line_start;
                cursor.y = cursor.y.saturating_add(1);
                continue;
            }
            if grapheme.width == 0 {
                continue;
            }

            let width = u16::try_from(grapheme.width)
                .map_err(|_| DrawError::WidthExceeded(grapheme.width))?;
            if self.draw_grapheme(cursor, grapheme.text, style, hyperlink)? {
                draw.graphemes += 1;
                draw.cells += usize::from(width);
            }
            cursor.x = cursor.x.saturating_add(i32::from(width));
        }

        Ok(draw)
    }

    /// Fills the visible part of `rect` with styled empty cells.
    pub fn fill(&mut self, rect: Rect, style: Style) -> Result<usize, DrawError> {
        let rect = rect.translated(self.origin.x, self.origin.y);
        let Some(rect) = rect
            .intersection(self.clip)
            .and_then(|rect| rect.intersection(self.buffer.bounds()))
        else {
            return Ok(0);
        };

        let mut written = 0;
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                self.buffer.set_empty(Point::new(x, y), style)?;
                written += 1;
            }
        }
        Ok(written)
    }
}

fn span_fits(point: Point, width: u16, rect: Rect) -> bool {
    if width == 0 || !rect.contains(point) {
        return false;
    }
    let end = point.x.saturating_add(i32::from(width) - 1);
    rect.contains(Point::new(end, point.y))
}

#[cfg(test)]
mod tests {
    use yatui_core::{Color, Size};

    use super::*;
    use crate::{Cell, CellContent};

    #[test]
    fn draws_wide_grapheme_as_start_and_continuation() -> Result<(), DrawError> {
        let mut buffer = Buffer::new(Size::new(4, 1));
        let mut store = GraphemeStore::new();
        let mut canvas = Canvas::new(&mut buffer, &mut store, WidthPolicy::Unicode);

        assert!(canvas.draw_grapheme(Point::ORIGIN, "界", Style::default(), None)?);
        assert!(matches!(
            buffer.get(Point::new(0, 0)).map(|cell| cell.content),
            Some(CellContent::Grapheme { width: 2, .. })
        ));
        assert!(matches!(
            buffer.get(Point::new(1, 0)).map(|cell| cell.content),
            Some(CellContent::Continuation { offset: 1, .. })
        ));
        Ok(())
    }

    #[test]
    fn clipping_never_draws_half_a_grapheme() -> Result<(), DrawError> {
        let mut buffer = Buffer::new(Size::new(3, 1));
        let mut store = GraphemeStore::new();
        let mut canvas = Canvas::new(&mut buffer, &mut store, WidthPolicy::Unicode)
            .with_clip(Rect::new(0, 0, 1, 1));

        assert!(!canvas.draw_grapheme(Point::ORIGIN, "界", Style::default(), None)?);
        assert!(buffer.cells().iter().all(|cell| *cell == Cell::default()));
        Ok(())
    }

    #[test]
    fn fill_clears_intersecting_wide_spans() -> Result<(), DrawError> {
        let mut buffer = Buffer::new(Size::new(3, 1));
        let mut store = GraphemeStore::new();
        let mut canvas = Canvas::new(&mut buffer, &mut store, WidthPolicy::Unicode);
        canvas.draw_grapheme(Point::ORIGIN, "界", Style::default(), None)?;
        canvas.fill(Rect::new(1, 0, 1, 1), Style::new().background(Color::Blue))?;

        assert_eq!(buffer.get(Point::ORIGIN), Some(&Cell::default()));
        assert_eq!(
            buffer
                .get(Point::new(1, 0))
                .map(|cell| cell.style.background),
            Some(Some(Color::Blue))
        );
        Ok(())
    }
}
