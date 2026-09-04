use std::fmt;

use arborui_core::{Point, Rect, Style};
use arborui_text::{WidthPolicy, graphemes, is_line_break};

use crate::{
    Buffer, BufferError, CellContent, GraphemeStore, GraphemeStoreError, HitId, HitMap,
    HyperlinkId, ImagePlacement, ImageScene, RgbaImage,
};

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
    /// Native image drawing was requested without a frame image scene.
    ImageSceneUnavailable,
    /// A renderer-created image scene exceeded a safety limit.
    ImageScene(crate::ImageSceneError),
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
            Self::ImageSceneUnavailable => formatter.write_str("canvas has no native image scene"),
            Self::ImageScene(error) => error.fmt(formatter),
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

impl From<crate::ImageSceneError> for DrawError {
    fn from(error: crate::ImageSceneError) -> Self {
        Self::ImageScene(error)
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
    hit_map: Option<&'a mut HitMap>,
    hit: Option<HitId>,
    damage_rows: Option<RowMask<'a>>,
    images: Option<&'a mut ImageScene>,
}

#[derive(Clone, Copy)]
struct RowMask<'a> {
    rows: &'a [bool],
    parent: Option<&'a RowMask<'a>>,
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
            hit_map: None,
            hit: None,
            damage_rows: None,
            images: None,
        }
    }

    pub(crate) fn with_hit_map(
        buffer: &'a mut Buffer,
        store: &'a mut GraphemeStore,
        hit_map: &'a mut HitMap,
        images: &'a mut ImageScene,
        width_policy: WidthPolicy,
    ) -> Self {
        let mut canvas = Self::new(buffer, store, width_policy);
        canvas.hit_map = Some(hit_map);
        canvas.images = Some(images);
        canvas
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

    /// Returns the width policy used for grapheme drawing.
    #[must_use]
    pub const fn width_policy(&self) -> WidthPolicy {
        self.width_policy
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

    /// Associates subsequent successful drawing with `hit`.
    #[must_use]
    pub fn with_hit(mut self, hit: Option<HitId>) -> Self {
        self.hit = hit;
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
            hit_map: self.hit_map.as_deref_mut(),
            hit: self.hit,
            damage_rows: self.damage_rows,
            images: self.images.as_deref_mut(),
        }
    }

    /// Filters writes to selected complete buffer rows.
    ///
    /// The ordinary rectangular clip and local origin remain in effect. Each
    /// entry corresponds to one row from the buffer origin; missing entries are
    /// treated as unselected. Complete rows preserve wide-grapheme spans.
    pub fn with_damage_rows<'b>(&'b mut self, rows: &'b [bool]) -> Canvas<'b> {
        let selected_rows = rows
            .iter()
            .enumerate()
            .map(|(y, selected)| {
                *selected && i32::try_from(y).is_ok_and(|y| self.row_is_selected(y))
            })
            .collect::<Vec<_>>();
        if let Some(images) = self.images.as_deref_mut() {
            images.mark_damaged(&selected_rows, self.clip);
        }
        Canvas {
            buffer: &mut *self.buffer,
            store: &mut *self.store,
            clip: self.clip,
            origin: self.origin,
            width_policy: self.width_policy,
            hit_map: self.hit_map.as_deref_mut(),
            hit: self.hit,
            damage_rows: Some(RowMask {
                rows,
                parent: self.damage_rows.as_ref(),
            }),
            images: self.images.as_deref_mut(),
        }
    }

    /// Draws an image into a terminal-cell rectangle.
    ///
    /// Fully visible native image data is recorded separately from the cell
    /// buffer, allowing unsupported backends to retain ordinary cell fallback
    /// content. A partially clipped image returns `false` and remains
    /// fallback-only because an exact pixel crop requires terminal cell metrics.
    pub fn draw_image(&mut self, rect: Rect, image: &RgbaImage) -> Result<bool, DrawError> {
        let destination = rect.translated(self.origin.x, self.origin.y);
        let Some(visible) = destination
            .intersection(self.clip)
            .and_then(|rect| rect.intersection(self.buffer.bounds()))
        else {
            return Ok(false);
        };
        if !(visible.y..visible.bottom()).any(|y| self.row_is_selected(y)) {
            return Ok(false);
        }
        let Some(placement) = ImagePlacement::clipped(image.clone(), destination, visible) else {
            return Ok(false);
        };

        let Some(images) = self.images.as_deref_mut() else {
            return Err(DrawError::ImageSceneUnavailable);
        };
        images.push(placement)?;

        for y in visible.y..visible.bottom() {
            if !self.row_is_selected(y) {
                continue;
            }
            for x in visible.x..visible.right() {
                if let Some(map) = self.hit_map.as_deref_mut() {
                    let _ = map.set_option(Point::new(x, y), self.hit);
                }
            }
        }
        Ok(true)
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
        if !span_fits(point, width, self.clip)
            || !span_fits(point, width, self.buffer.bounds())
            || !self.row_is_selected(point.y)
        {
            return Ok(false);
        }

        let id = self.store.intern(value)?;
        for offset in 0..width {
            self.clear_hit_span_at(point.translated(i32::from(offset), 0));
        }
        self.buffer
            .set_grapheme(point, id, width, style, hyperlink)?;
        self.mark_hit_span(point, width);
        Ok(true)
    }

    /// Draws text with mandatory Unicode line breaks interpreted as line separators.
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
            if grapheme.text.chars().any(is_line_break) {
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

        self.fill_rect(rect, style)
    }

    fn fill_rect(&mut self, rect: Rect, style: Style) -> Result<usize, DrawError> {
        let mut written = 0;
        for y in rect.y..rect.bottom() {
            if !self.row_is_selected(y) {
                continue;
            }
            for x in rect.x..rect.right() {
                let point = Point::new(x, y);
                self.clear_hit_span_at(point);
                self.buffer.set_empty(point, style)?;
                self.mark_hit_span(point, 1);
                written += 1;
            }
        }
        Ok(written)
    }

    fn row_is_selected(&self, y: i32) -> bool {
        let Some(y) = usize::try_from(y).ok() else {
            return false;
        };
        let mut mask = self.damage_rows.as_ref();
        while let Some(current) = mask {
            if !current.rows.get(y).copied().unwrap_or(false) {
                return false;
            }
            mask = current.parent;
        }
        true
    }

    fn mark_hit_span(&mut self, point: Point, width: u16) {
        let (Some(map), Some(hit)) = (self.hit_map.as_deref_mut(), self.hit) else {
            return;
        };
        for offset in 0..width {
            let _ = map.set(point.translated(i32::from(offset), 0), hit);
        }
    }

    fn clear_hit_span_at(&mut self, point: Point) {
        let Some(map) = self.hit_map.as_deref_mut() else {
            return;
        };
        let span = match self.buffer.get(point).map(|cell| cell.content) {
            Some(CellContent::Grapheme { width, .. }) => Some((point, width)),
            Some(CellContent::Continuation { offset, .. }) => {
                let start = point.translated(-i32::from(offset), 0);
                match self.buffer.get(start).map(|cell| cell.content) {
                    Some(CellContent::Grapheme { width, .. }) => Some((start, width)),
                    _ => None,
                }
            }
            Some(CellContent::Empty) => Some((point, 1)),
            None => None,
        };
        if let Some((start, width)) = span {
            for offset in 0..width {
                let _ = map.clear(start.translated(i32::from(offset), 0));
            }
        }
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
    use arborui_core::{Color, Size};

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
    fn rejects_nonprinting_graphemes() {
        let mut buffer = Buffer::new(Size::new(1, 1));
        let mut store = GraphemeStore::new();
        let mut canvas = Canvas::new(&mut buffer, &mut store, WidthPolicy::Unicode);

        for value in ["\u{1b}", "\u{85}", "\u{2028}", "\u{2029}"] {
            assert_eq!(
                canvas.draw_grapheme(Point::ORIGIN, value, Style::default(), None),
                Err(DrawError::InvalidGrapheme)
            );
        }
    }

    #[test]
    fn draws_mandatory_breaks_and_omits_other_controls() -> Result<(), DrawError> {
        let mut buffer = Buffer::new(Size::new(2, 3));
        let mut store = GraphemeStore::new();
        let mut canvas = Canvas::new(&mut buffer, &mut store, WidthPolicy::Unicode);

        let draw = canvas.draw_text(
            Point::ORIGIN,
            "a\u{1b}b\u{b}c\u{2028}d",
            Style::default(),
            None,
        )?;

        assert_eq!(draw.graphemes, 4);
        let visible = [
            Point::new(0, 0),
            Point::new(1, 0),
            Point::new(0, 1),
            Point::new(0, 2),
        ]
        .map(|point| match buffer.get(point).map(|cell| cell.content) {
            Some(CellContent::Grapheme { id, .. }) => store.get(id).map(AsRef::as_ref),
            _ => Err(crate::GraphemeStoreError::InvalidGrapheme),
        });
        assert_eq!(visible, [Ok("a"), Ok("b"), Ok("c"), Ok("d")]);
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

    #[test]
    fn rejected_image_does_not_change_hit_ownership() -> Result<(), Box<dyn std::error::Error>> {
        let existing = RgbaImage::new(1, 1, vec![0; 4])?;
        let replacement = RgbaImage::new(1, 1, vec![1; 4])?;
        let mut scene = ImageScene::from_placements(
            (0..crate::MAX_IMAGE_PLACEMENTS)
                .map(|_| ImagePlacement::new(existing.clone(), Rect::new(0, 0, 1, 1))),
        );
        let mut buffer = Buffer::new(Size::new(1, 1));
        let mut store = GraphemeStore::new();
        let mut hit_map = HitMap::new(Size::new(1, 1));
        let _ = hit_map.set(Point::ORIGIN, HitId::new(1));

        let error = Canvas::with_hit_map(
            &mut buffer,
            &mut store,
            &mut hit_map,
            &mut scene,
            WidthPolicy::Unicode,
        )
        .with_hit(Some(HitId::new(2)))
        .draw_image(Rect::new(0, 0, 1, 1), &replacement)
        .expect_err("the placement limit must reject another image");

        assert_eq!(
            error,
            DrawError::ImageScene(crate::ImageSceneError::TooManyPlacements {
                maximum: crate::MAX_IMAGE_PLACEMENTS,
            })
        );
        assert_eq!(hit_map.get(Point::ORIGIN), Some(HitId::new(1)));
        Ok(())
    }

    #[test]
    fn damage_rows_filter_fills_and_complete_graphemes() -> Result<(), DrawError> {
        let mut buffer = Buffer::new(Size::new(4, 3));
        let mut store = GraphemeStore::new();
        {
            let mut canvas = Canvas::new(&mut buffer, &mut store, WidthPolicy::Unicode);
            let rows = [true, false, true];
            let mut damaged = canvas.with_damage_rows(&rows);

            assert_eq!(
                damaged.fill(Rect::new(0, 0, 4, 3), Style::new().background(Color::Blue))?,
                8
            );
            assert!(!damaged.draw_grapheme(Point::new(0, 1), "界", Style::default(), None)?);
            assert!(damaged.draw_grapheme(Point::new(0, 2), "界", Style::default(), None)?);
        }

        assert!((0..4).all(|x| buffer.get(Point::new(x, 1)) == Some(&Cell::default())));
        assert!(matches!(
            buffer.get(Point::new(0, 2)).map(|cell| cell.content),
            Some(CellContent::Grapheme { width: 2, .. })
        ));
        Ok(())
    }

    #[test]
    fn nested_damage_rows_cannot_broaden_parent_selection() -> Result<(), DrawError> {
        let mut buffer = Buffer::new(Size::new(2, 2));
        let mut store = GraphemeStore::new();
        {
            let mut canvas = Canvas::new(&mut buffer, &mut store, WidthPolicy::Unicode);
            let outer_rows = [true, false];
            let inner_rows = [true, true];
            let mut outer = canvas.with_damage_rows(&outer_rows);
            let mut inner = outer.with_damage_rows(&inner_rows);

            assert_eq!(
                inner.fill(Rect::new(0, 0, 2, 2), Style::new().background(Color::Blue))?,
                2
            );
        }

        assert!((0..2).all(|x| buffer.get(Point::new(x, 1)) == Some(&Cell::default())));
        Ok(())
    }
}
