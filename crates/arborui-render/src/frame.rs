use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use arborui_core::{CursorState, Point, Size, Style};
use arborui_text::WidthPolicy;

use crate::{
    Buffer, BufferError, Canvas, Cell, CellContent, DrawError, GraphemeId, GraphemeStore,
    GraphemeStoreError, HitMap, HyperlinkId,
};

/// Resolved content in a terminal-independent frame patch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatchCellContent {
    /// A visually empty cell.
    Empty,
    /// A complete grapheme at its leading cell.
    Grapheme {
        /// Stable identity used for diagnostics and caching.
        id: GraphemeId,
        /// Shared UTF-8 grapheme text.
        text: Arc<str>,
        /// Number of occupied terminal cells.
        width: u16,
    },
    /// A continuation cell for a wide grapheme.
    Continuation {
        /// Stable grapheme identity.
        id: GraphemeId,
        /// Cell offset from the leading cell.
        offset: u16,
    },
}

/// One resolved cell in a frame patch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchCell {
    /// Resolved cell content.
    pub content: PatchCellContent,
    /// Cell style.
    pub style: Style,
    /// Optional hyperlink identity.
    pub hyperlink: Option<HyperlinkId>,
}

/// A contiguous run of changed cells on one row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellRun {
    /// Position of the first cell.
    pub position: Point,
    /// Changed cells in left-to-right order.
    pub cells: Vec<PatchCell>,
}

/// Terminal-independent changes between two logical frames.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FramePatch {
    /// Target frame size.
    pub size: Size,
    /// Contiguous changed cell runs.
    pub runs: Vec<CellRun>,
    /// Desired cursor state after applying the patch.
    pub cursor: CursorState,
    /// Whether the cursor differs from the committed state.
    pub cursor_changed: bool,
    /// Whether this patch describes a complete repaint.
    pub full_repaint: bool,
}

impl FramePatch {
    /// Returns whether applying the patch requires no terminal output.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty() && !self.cursor_changed
    }

    /// Applies this patch to a logical buffer.
    ///
    /// This operation is primarily useful for testing, recording, and remote
    /// transports. It does not apply cursor state.
    pub fn apply_to(&self, buffer: &mut Buffer) -> Result<(), BufferError> {
        if buffer.size() != self.size {
            *buffer = Buffer::new(self.size);
        } else if self.full_repaint {
            buffer.clear(Style::default());
        }

        for run in &self.runs {
            for (offset, cell) in run.cells.iter().enumerate() {
                let point = run.position.translated(offset as i32, 0);
                match cell.content {
                    PatchCellContent::Empty => buffer.set_empty(point, cell.style)?,
                    PatchCellContent::Grapheme { id, width, .. } => {
                        buffer.set_grapheme(point, id, width, cell.style, cell.hyperlink)?;
                    }
                    PatchCellContent::Continuation { .. } => {}
                }
            }
        }
        Ok(())
    }
}

/// A fully painted frame waiting for an output decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedFrame {
    next: Buffer,
    hit_map: HitMap,
    cursor: CursorState,
    patch: FramePatch,
    renderer_id: u64,
    generation: u64,
}

/// Opaque identity for one committed renderer state.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RendererStateId {
    renderer_id: u64,
    generation: u64,
}

/// Failure to commit a frame against the renderer state that prepared it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitError {
    /// The frame was prepared by another renderer instance.
    WrongRenderer,
    /// Renderer state advanced after this frame was prepared.
    StaleFrame,
}

impl fmt::Display for CommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongRenderer => formatter.write_str("frame was prepared by another renderer"),
            Self::StaleFrame => formatter.write_str("renderer advanced after frame preparation"),
        }
    }
}

impl std::error::Error for CommitError {}

impl PreparedFrame {
    /// Returns the terminal-independent patch for this frame.
    #[must_use]
    pub const fn patch(&self) -> &FramePatch {
        &self.patch
    }

    /// Returns the prepared logical buffer.
    #[must_use]
    pub const fn buffer(&self) -> &Buffer {
        &self.next
    }

    /// Returns the interactive map prepared with the logical frame.
    #[must_use]
    pub const fn hit_map(&self) -> &HitMap {
        &self.hit_map
    }
}

/// Errors produced while preparing a frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderError {
    /// Painting failed.
    Draw(DrawError),
    /// A cell referred to an unknown grapheme.
    GraphemeStore(GraphemeStoreError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Draw(error) => error.fmt(formatter),
            Self::GraphemeStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<DrawError> for RenderError {
    fn from(error: DrawError) -> Self {
        Self::Draw(error)
    }
}

impl From<GraphemeStoreError> for RenderError {
    fn from(error: GraphemeStoreError) -> Self {
        Self::GraphemeStore(error)
    }
}

/// Stateful renderer with transactional frame preparation and commit.
#[derive(Debug)]
pub struct Renderer {
    id: u64,
    generation: u64,
    current: Buffer,
    hit_map: HitMap,
    cursor: CursorState,
    graphemes: GraphemeStore,
    width_policy: WidthPolicy,
    force_full_repaint: bool,
}

static NEXT_RENDERER_ID: AtomicU64 = AtomicU64::new(1);

impl Clone for Renderer {
    fn clone(&self) -> Self {
        Self {
            id: next_renderer_id(),
            generation: self.generation,
            current: self.current.clone(),
            hit_map: self.hit_map.clone(),
            cursor: self.cursor,
            graphemes: self.graphemes.clone(),
            width_policy: self.width_policy,
            force_full_repaint: self.force_full_repaint,
        }
    }
}

impl Renderer {
    /// Creates a renderer with an empty committed frame.
    #[must_use]
    pub fn new(size: Size, width_policy: WidthPolicy) -> Self {
        Self {
            id: next_renderer_id(),
            generation: 0,
            current: Buffer::new(size),
            hit_map: HitMap::new(size),
            cursor: CursorState::default(),
            graphemes: GraphemeStore::new(),
            width_policy,
            force_full_repaint: true,
        }
    }

    /// Returns the committed frame.
    #[must_use]
    pub const fn current(&self) -> &Buffer {
        &self.current
    }

    /// Returns the hit map committed with the current logical frame.
    #[must_use]
    pub const fn hit_map(&self) -> &HitMap {
        &self.hit_map
    }

    /// Returns an opaque identity for the currently committed renderer state.
    #[must_use]
    pub const fn state_id(&self) -> RendererStateId {
        RendererStateId {
            renderer_id: self.id,
            generation: self.generation,
        }
    }

    /// Returns the active width policy.
    #[must_use]
    pub const fn width_policy(&self) -> WidthPolicy {
        self.width_policy
    }

    /// Changes the width policy and invalidates the committed physical state.
    pub fn set_width_policy(&mut self, width_policy: WidthPolicy) {
        if self.width_policy != width_policy {
            self.width_policy = width_policy;
            self.invalidate();
        }
    }

    /// Paints and diffs a complete logical frame without committing it.
    pub fn prepare<F>(
        &mut self,
        size: Size,
        cursor: CursorState,
        paint: F,
    ) -> Result<PreparedFrame, RenderError>
    where
        F: FnOnce(&mut Canvas<'_>) -> Result<(), DrawError>,
    {
        let mut next = Buffer::new(size);
        let mut hit_map = HitMap::new(size);
        let mut canvas = Canvas::with_hit_map(
            &mut next,
            &mut self.graphemes,
            &mut hit_map,
            self.width_policy,
        );
        paint(&mut canvas)?;

        let full_repaint = self.force_full_repaint || self.current.size() != size;
        let patch = diff(
            &self.current,
            &next,
            self.cursor,
            cursor,
            full_repaint,
            &self.graphemes,
        )?;
        Ok(PreparedFrame {
            next,
            hit_map,
            cursor,
            patch,
            renderer_id: self.id,
            generation: self.generation,
        })
    }

    /// Commits a prepared frame after its patch has been accepted for output.
    pub fn commit(&mut self, prepared: PreparedFrame) -> Result<(), CommitError> {
        if prepared.renderer_id != self.id {
            return Err(CommitError::WrongRenderer);
        }
        if prepared.generation != self.generation {
            return Err(CommitError::StaleFrame);
        }
        self.current = prepared.next;
        self.hit_map = prepared.hit_map;
        self.cursor = prepared.cursor;
        self.force_full_repaint = false;
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    /// Discards a prepared frame without changing committed state.
    pub fn discard(&mut self, _prepared: PreparedFrame) {}

    /// Marks physical terminal state as unknown so the next patch repaints all cells.
    pub fn invalidate(&mut self) {
        self.force_full_repaint = true;
        self.generation = self.generation.wrapping_add(1);
    }
}

fn next_renderer_id() -> u64 {
    NEXT_RENDERER_ID.fetch_add(1, Ordering::Relaxed)
}

fn diff(
    current: &Buffer,
    next: &Buffer,
    current_cursor: CursorState,
    next_cursor: CursorState,
    full_repaint: bool,
    store: &GraphemeStore,
) -> Result<FramePatch, GraphemeStoreError> {
    let size = next.size();
    let mut runs = Vec::new();

    for y in 0..size.height {
        let mut x = 0;
        while x < size.width {
            let point = Point::new(i32::from(x), i32::from(y));
            if !full_repaint && current.get(point) == next.get(point) {
                x += 1;
                continue;
            }

            let start = x;
            let mut cells = Vec::new();
            while x < size.width {
                let point = Point::new(i32::from(x), i32::from(y));
                if !full_repaint && current.get(point) == next.get(point) {
                    break;
                }
                if let Some(cell) = next.get(point) {
                    cells.push(resolve_cell(*cell, store)?);
                }
                x += 1;
            }
            runs.push(CellRun {
                position: Point::new(i32::from(start), i32::from(y)),
                cells,
            });
        }
    }

    Ok(FramePatch {
        size,
        runs,
        cursor: next_cursor,
        cursor_changed: current_cursor != next_cursor,
        full_repaint,
    })
}

fn resolve_cell(cell: Cell, store: &GraphemeStore) -> Result<PatchCell, GraphemeStoreError> {
    let content = match cell.content {
        CellContent::Empty => PatchCellContent::Empty,
        CellContent::Grapheme { id, width } => PatchCellContent::Grapheme {
            id,
            text: Arc::clone(store.get(id)?),
            width,
        },
        CellContent::Continuation { id, offset } => PatchCellContent::Continuation { id, offset },
    };
    Ok(PatchCell {
        content,
        style: cell.style,
        hyperlink: cell.hyperlink,
    })
}

#[cfg(test)]
mod tests {
    use arborui_core::{Color, Point, Rect, Style};

    use super::*;

    #[test]
    fn committed_identical_frame_produces_no_patch() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(3, 1), WidthPolicy::Unicode);
        let first = renderer.prepare(Size::new(3, 1), CursorState::default(), |canvas| {
            canvas.draw_text(Point::ORIGIN, "abc", Style::default(), None)?;
            Ok(())
        })?;
        assert!(first.patch().full_repaint);
        assert_eq!(renderer.commit(first), Ok(()));

        let second = renderer.prepare(Size::new(3, 1), CursorState::default(), |canvas| {
            canvas.draw_text(Point::ORIGIN, "abc", Style::default(), None)?;
            Ok(())
        })?;

        assert!(second.patch().is_empty());
        assert!(!second.patch().full_repaint);
        Ok(())
    }

    #[test]
    fn changed_cells_are_grouped_into_runs() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(4, 1), WidthPolicy::Unicode);
        let initial = renderer.prepare(Size::new(4, 1), CursorState::default(), |_| Ok(()))?;
        assert_eq!(renderer.commit(initial), Ok(()));

        let changed = renderer.prepare(Size::new(4, 1), CursorState::default(), |canvas| {
            canvas.draw_text(
                Point::new(1, 0),
                "ab",
                Style::new().foreground(Color::Green),
                None,
            )?;
            Ok(())
        })?;

        assert_eq!(changed.patch().runs.len(), 1);
        assert_eq!(changed.patch().runs[0].position, Point::new(1, 0));
        assert_eq!(changed.patch().runs[0].cells.len(), 2);
        let mut replay = renderer.current().clone();
        assert_eq!(changed.patch().apply_to(&mut replay), Ok(()));
        assert_eq!(&replay, changed.buffer());
        Ok(())
    }

    #[test]
    fn discarded_frame_does_not_advance_committed_state() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let initial = renderer.prepare(Size::new(1, 1), CursorState::default(), |_| Ok(()))?;
        assert_eq!(renderer.commit(initial), Ok(()));
        let changed = renderer.prepare(Size::new(1, 1), CursorState::default(), |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;
        renderer.discard(changed);

        let retry = renderer.prepare(Size::new(1, 1), CursorState::default(), |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;
        assert!(!retry.patch().is_empty());
        Ok(())
    }

    #[test]
    fn hit_maps_commit_and_discard_with_their_frames() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(2, 1), WidthPolicy::Unicode);
        let prepared = renderer.prepare(Size::new(2, 1), CursorState::default(), |canvas| {
            let mut canvas = canvas
                .scoped(Rect::new(0, 0, 2, 1), Point::ORIGIN)
                .with_hit(Some(crate::HitId::new(9)));
            canvas.draw_text(Point::ORIGIN, "界", Style::default(), None)?;
            Ok(())
        })?;

        assert_eq!(
            prepared.hit_map().get(Point::new(0, 0)),
            Some(crate::HitId::new(9))
        );
        assert_eq!(
            prepared.hit_map().get(Point::new(1, 0)),
            Some(crate::HitId::new(9))
        );
        assert_eq!(renderer.hit_map().get(Point::ORIGIN), None);
        renderer.discard(prepared);
        assert_eq!(renderer.hit_map().get(Point::ORIGIN), None);

        let committed = renderer.prepare(Size::new(2, 1), CursorState::default(), |canvas| {
            let mut canvas = canvas
                .scoped(Rect::new(0, 0, 2, 1), Point::ORIGIN)
                .with_hit(Some(crate::HitId::new(4)));
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;
        assert_eq!(renderer.commit(committed), Ok(()));
        assert_eq!(
            renderer.hit_map().get(Point::ORIGIN),
            Some(crate::HitId::new(4))
        );
        Ok(())
    }

    #[test]
    fn overwriting_wide_continuation_clears_the_complete_hit_span() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(2, 1), WidthPolicy::Unicode);
        let prepared = renderer.prepare(Size::new(2, 1), CursorState::default(), |canvas| {
            {
                let mut wide = canvas
                    .scoped(Rect::new(0, 0, 2, 1), Point::ORIGIN)
                    .with_hit(Some(crate::HitId::new(1)));
                wide.draw_text(Point::ORIGIN, "界", Style::default(), None)?;
            }
            let mut replacement = canvas
                .scoped(Rect::new(1, 0, 1, 1), Point::ORIGIN)
                .with_hit(Some(crate::HitId::new(2)));
            replacement.fill(Rect::new(1, 0, 1, 1), Style::default())?;
            Ok(())
        })?;

        assert_eq!(prepared.hit_map().get(Point::ORIGIN), None);
        assert_eq!(
            prepared.hit_map().get(Point::new(1, 0)),
            Some(crate::HitId::new(2))
        );
        Ok(())
    }

    #[test]
    fn rejects_cross_renderer_and_stale_commits() -> Result<(), RenderError> {
        let mut first = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let mut second = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let wrong_renderer = first.prepare(Size::new(1, 1), CursorState::default(), |_| Ok(()))?;
        assert_eq!(
            second.commit(wrong_renderer),
            Err(CommitError::WrongRenderer)
        );

        let current = first.prepare(Size::new(1, 1), CursorState::default(), |_| Ok(()))?;
        let stale = first.prepare(Size::new(1, 1), CursorState::default(), |_| Ok(()))?;
        assert_eq!(first.commit(current), Ok(()));
        assert_eq!(first.commit(stale), Err(CommitError::StaleFrame));
        Ok(())
    }
}
