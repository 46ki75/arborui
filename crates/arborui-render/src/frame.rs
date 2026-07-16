use std::{
    collections::{HashMap, hash_map::Entry},
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use arborui_core::{CursorState, Point, Size, Style};
use arborui_text::{WidthPolicy, graphemes};

use crate::{
    Buffer, BufferError, Canvas, Cell, CellContent, DrawError, GraphemeId, GraphemeStore,
    GraphemeStoreError, HitMap, HyperlinkId,
};

/// Resolved content in a terminal-independent frame patch.
///
/// Renderer-generated grapheme IDs remain associated with the same text within
/// that renderer's patch stream. Manually constructed patch streams must
/// preserve the same identity-to-text mapping across patches.
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
    ///
    /// Within a [`CellRun`], this is always covered by the preceding matching
    /// [`PatchCellContent::Grapheme`]. Backends emit the leading grapheme and
    /// skip its continuations.
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
///
/// Wide grapheme spans are atomic: a `Grapheme` with width `n` is immediately
/// followed by its `n - 1` matching `Continuation` cells in this same run.
/// Matching continuations have the same grapheme identity, style, and
/// hyperlink as the leading cell, and offsets `1..n`. A run therefore never
/// starts with a continuation. Backends should emit only the leading grapheme
/// and skip the covered continuation cells.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellRun {
    /// Position of the first cell.
    pub position: Point,
    /// Changed cells in left-to-right order.
    pub cells: Vec<PatchCell>,
}

/// Terminal-independent changes between two logical frames.
///
/// Runs are row-major and non-overlapping. A full repaint of a nonempty frame
/// contains exactly one complete run per row.
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

/// A violation of the public [`FramePatch`] cell-run contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FramePatchValidationError {
    /// A run contains no cells.
    EmptyRun {
        /// Index of the invalid run.
        run: usize,
    },
    /// A run starts outside the target frame.
    RunOutOfBounds {
        /// Index of the invalid run.
        run: usize,
        /// Invalid run origin.
        position: Point,
    },
    /// A run extends beyond the target row.
    RunDoesNotFit {
        /// Index of the invalid run.
        run: usize,
        /// Number of cells in the run.
        cells: usize,
    },
    /// A run overlaps the preceding run on the same row.
    OverlappingRuns {
        /// Index of the preceding run.
        previous_run: usize,
        /// Index of the overlapping run.
        run: usize,
    },
    /// A run appears before the preceding run in row-major order.
    RunsOutOfOrder {
        /// Index of the preceding run.
        previous_run: usize,
        /// Index of the out-of-order run.
        run: usize,
    },
    /// A nonempty full repaint does not cover every cell exactly once.
    IncompleteFullRepaint,
    /// A leading grapheme has zero width.
    ZeroWidthGrapheme {
        /// Index of the invalid run.
        run: usize,
        /// Index of the invalid cell within the run.
        cell: usize,
    },
    /// A leading grapheme is not followed by its complete continuation span.
    IncompleteGrapheme {
        /// Index of the invalid run.
        run: usize,
        /// Index of the leading cell within the run.
        cell: usize,
        /// Declared grapheme width.
        width: u16,
    },
    /// A continuation is isolated or does not match its leading grapheme.
    InvalidContinuation {
        /// Index of the invalid run.
        run: usize,
        /// Index of the continuation within the run.
        cell: usize,
    },
    /// Grapheme text is not exactly one printable extended grapheme cluster.
    InvalidGraphemeText {
        /// Index of the invalid run.
        run: usize,
        /// Index of the invalid cell within the run.
        cell: usize,
    },
    /// A grapheme's declared width differs from its measured width.
    GraphemeWidthMismatch {
        /// Index of the invalid run.
        run: usize,
        /// Index of the invalid cell within the run.
        cell: usize,
        /// Width declared by the patch.
        declared: u16,
        /// Width measured under the selected policy.
        actual: usize,
    },
    /// One grapheme identity is associated with different text in this patch.
    ConflictingGraphemeId {
        /// Conflicting grapheme identity.
        id: GraphemeId,
        /// Index of the first run containing the identity.
        first_run: usize,
        /// Index of the first cell containing the identity.
        first_cell: usize,
        /// Index of the run containing the conflicting text.
        run: usize,
        /// Index of the cell containing the conflicting text.
        cell: usize,
    },
}

impl fmt::Display for FramePatchValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRun { run } => write!(formatter, "frame patch run {run} is empty"),
            Self::RunOutOfBounds { run, position } => write!(
                formatter,
                "frame patch run {run} starts outside the frame at ({}, {})",
                position.x, position.y
            ),
            Self::RunDoesNotFit { run, cells } => write!(
                formatter,
                "frame patch run {run} with {cells} cells extends beyond its row"
            ),
            Self::OverlappingRuns { previous_run, run } => write!(
                formatter,
                "frame patch run {run} overlaps preceding run {previous_run}"
            ),
            Self::RunsOutOfOrder { previous_run, run } => write!(
                formatter,
                "frame patch run {run} appears before preceding run {previous_run}"
            ),
            Self::IncompleteFullRepaint => formatter.write_str(
                "a full repaint of a nonempty frame must contain one complete run per row",
            ),
            Self::ZeroWidthGrapheme { run, cell } => write!(
                formatter,
                "frame patch run {run} cell {cell} contains a zero-width grapheme"
            ),
            Self::IncompleteGrapheme { run, cell, width } => write!(
                formatter,
                "frame patch run {run} cell {cell} does not contain the complete width-{width} grapheme span"
            ),
            Self::InvalidContinuation { run, cell } => write!(
                formatter,
                "frame patch run {run} cell {cell} is not covered by a matching grapheme"
            ),
            Self::InvalidGraphemeText { run, cell } => write!(
                formatter,
                "frame patch run {run} cell {cell} is not exactly one printable grapheme"
            ),
            Self::GraphemeWidthMismatch {
                run,
                cell,
                declared,
                actual,
            } => write!(
                formatter,
                "frame patch run {run} cell {cell} declares width {declared} but measures {actual}"
            ),
            Self::ConflictingGraphemeId {
                id,
                first_run,
                first_cell,
                run,
                cell,
            } => write!(
                formatter,
                "frame patch run {run} cell {cell} maps grapheme id {} to different text than run {first_run} cell {first_cell}",
                id.get()
            ),
        }
    }
}

impl std::error::Error for FramePatchValidationError {}

impl FramePatch {
    /// Returns whether applying the patch requires no terminal output.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let nonempty_full_repaint =
            self.full_repaint && self.size.width != 0 && self.size.height != 0;
        self.runs.is_empty() && !self.cursor_changed && !nonempty_full_repaint
    }

    /// Validates run geometry and the atomic wide-grapheme contract.
    ///
    /// Renderer-generated patches always satisfy this contract. This check is
    /// sufficient for policy-independent consumers such as
    /// [`FramePatch::apply_to`]. Terminal backends should additionally call
    /// [`FramePatch::validate_for_width_policy`] before producing output.
    pub fn validate(&self) -> Result<(), FramePatchValidationError> {
        let mut previous_run = None;
        for (run_index, run) in self.runs.iter().enumerate() {
            if run.cells.is_empty() {
                return Err(FramePatchValidationError::EmptyRun { run: run_index });
            }
            let Ok(x) = u16::try_from(run.position.x) else {
                return Err(FramePatchValidationError::RunOutOfBounds {
                    run: run_index,
                    position: run.position,
                });
            };
            let Ok(y) = u16::try_from(run.position.y) else {
                return Err(FramePatchValidationError::RunOutOfBounds {
                    run: run_index,
                    position: run.position,
                });
            };
            if x >= self.size.width || y >= self.size.height {
                return Err(FramePatchValidationError::RunOutOfBounds {
                    run: run_index,
                    position: run.position,
                });
            }
            if run.cells.len() > usize::from(self.size.width - x) {
                return Err(FramePatchValidationError::RunDoesNotFit {
                    run: run_index,
                    cells: run.cells.len(),
                });
            }

            let start = usize::from(x);
            let end = start + run.cells.len();
            if let Some((previous_index, previous_x, previous_end, previous_y)) = previous_run {
                if y == previous_y && start < previous_end && end > previous_x {
                    return Err(FramePatchValidationError::OverlappingRuns {
                        previous_run: previous_index,
                        run: run_index,
                    });
                }
                if y < previous_y || (y == previous_y && start < previous_end) {
                    return Err(FramePatchValidationError::RunsOutOfOrder {
                        previous_run: previous_index,
                        run: run_index,
                    });
                }
            }
            previous_run = Some((run_index, start, end, y));

            let mut cell_index = 0;
            while cell_index < run.cells.len() {
                let leading = &run.cells[cell_index];
                match leading.content {
                    PatchCellContent::Empty => cell_index += 1,
                    PatchCellContent::Continuation { .. } => {
                        return Err(FramePatchValidationError::InvalidContinuation {
                            run: run_index,
                            cell: cell_index,
                        });
                    }
                    PatchCellContent::Grapheme { id, width, .. } => {
                        if width == 0 {
                            return Err(FramePatchValidationError::ZeroWidthGrapheme {
                                run: run_index,
                                cell: cell_index,
                            });
                        }
                        let end = cell_index.saturating_add(usize::from(width));
                        if end > run.cells.len() {
                            return Err(FramePatchValidationError::IncompleteGrapheme {
                                run: run_index,
                                cell: cell_index,
                                width,
                            });
                        }
                        for offset in 1..width {
                            let continuation_index = cell_index + usize::from(offset);
                            let continuation = &run.cells[continuation_index];
                            let matches = matches!(
                                continuation.content,
                                PatchCellContent::Continuation {
                                    id: continuation_id,
                                    offset: continuation_offset,
                                } if continuation_id == id && continuation_offset == offset
                            ) && continuation.style == leading.style
                                && continuation.hyperlink == leading.hyperlink;
                            if !matches {
                                return Err(FramePatchValidationError::InvalidContinuation {
                                    run: run_index,
                                    cell: continuation_index,
                                });
                            }
                        }
                        cell_index = end;
                    }
                }
            }
        }

        if self.full_repaint && self.size.width != 0 && self.size.height != 0 {
            let complete = self.runs.len() == usize::from(self.size.height)
                && self.runs.iter().zip(0..self.size.height).all(|(run, y)| {
                    run.position == Point::new(0, i32::from(y))
                        && run.cells.len() == usize::from(self.size.width)
                });
            if !complete {
                return Err(FramePatchValidationError::IncompleteFullRepaint);
            }
        }
        Ok(())
    }

    /// Validates the structural contract and terminal-policy-dependent text.
    ///
    /// In addition to [`FramePatch::validate`], this requires every grapheme
    /// value to contain exactly one printable extended grapheme cluster whose
    /// measured width matches its declared width, and every grapheme identity
    /// visible in this patch to map to only one text value. This method cannot
    /// detect identity conflicts across separate manually constructed patches;
    /// their producer must preserve that mapping across the patch stream.
    /// Terminal backends should use this method before producing any output.
    pub fn validate_for_width_policy(
        &self,
        width_policy: WidthPolicy,
    ) -> Result<(), FramePatchValidationError> {
        self.validate()?;
        let mut grapheme_texts: HashMap<GraphemeId, (&str, usize, usize)> = HashMap::new();
        for (run_index, run) in self.runs.iter().enumerate() {
            for (cell_index, cell) in run.cells.iter().enumerate() {
                let PatchCellContent::Grapheme { id, text, width } = &cell.content else {
                    continue;
                };
                if text.chars().any(char::is_control) {
                    return Err(FramePatchValidationError::InvalidGraphemeText {
                        run: run_index,
                        cell: cell_index,
                    });
                }
                let mut clusters = graphemes(text, width_policy);
                let Some(grapheme) = clusters.next() else {
                    return Err(FramePatchValidationError::InvalidGraphemeText {
                        run: run_index,
                        cell: cell_index,
                    });
                };
                if grapheme.text != text.as_ref() || clusters.next().is_some() {
                    return Err(FramePatchValidationError::InvalidGraphemeText {
                        run: run_index,
                        cell: cell_index,
                    });
                }
                if grapheme.width != usize::from(*width) {
                    return Err(FramePatchValidationError::GraphemeWidthMismatch {
                        run: run_index,
                        cell: cell_index,
                        declared: *width,
                        actual: grapheme.width,
                    });
                }
                match grapheme_texts.entry(*id) {
                    Entry::Occupied(entry) if entry.get().0 != text.as_ref() => {
                        let &(_, first_run, first_cell) = entry.get();
                        return Err(FramePatchValidationError::ConflictingGraphemeId {
                            id: *id,
                            first_run,
                            first_cell,
                            run: run_index,
                            cell: cell_index,
                        });
                    }
                    Entry::Vacant(entry) => {
                        entry.insert((text.as_ref(), run_index, cell_index));
                    }
                    Entry::Occupied(_) => {}
                }
            }
        }
        Ok(())
    }

    /// Applies this patch to a logical buffer.
    ///
    /// This operation is primarily useful for testing, recording, and remote
    /// transports. It does not apply cursor state.
    pub fn apply_to(&self, buffer: &mut Buffer) -> Result<(), BufferError> {
        self.validate().map_err(BufferError::InvalidPatch)?;
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
#[derive(Clone, Debug)]
pub struct PreparedFrame {
    next: Buffer,
    hit_map: HitMap,
    cursor: CursorState,
    patch: FramePatch,
    graphemes: GraphemeStore,
    renderer_id: u64,
    generation: u64,
}

impl PartialEq for PreparedFrame {
    fn eq(&self, other: &Self) -> bool {
        self.next == other.next
            && self.hit_map == other.hit_map
            && self.cursor == other.cursor
            && self.patch == other.patch
            && self.renderer_id == other.renderer_id
            && self.generation == other.generation
    }
}

impl Eq for PreparedFrame {}

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
        let mut graphemes = self.graphemes.clone();
        let mut canvas =
            Canvas::with_hit_map(&mut next, &mut graphemes, &mut hit_map, self.width_policy);
        paint(&mut canvas)?;

        let full_repaint = self.force_full_repaint || self.current.size() != size;
        let patch = diff(
            &self.current,
            &next,
            self.cursor,
            cursor,
            full_repaint,
            &graphemes,
            self.width_policy,
        )?;
        Ok(PreparedFrame {
            next,
            hit_map,
            cursor,
            patch,
            graphemes,
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
        let mut graphemes = prepared.graphemes;
        graphemes.retain(
            prepared
                .next
                .cells()
                .iter()
                .filter_map(|cell| match cell.content {
                    CellContent::Grapheme { id, .. } | CellContent::Continuation { id, .. } => {
                        Some(id)
                    }
                    CellContent::Empty => None,
                }),
        );
        self.current = prepared.next;
        self.hit_map = prepared.hit_map;
        self.cursor = prepared.cursor;
        self.graphemes = graphemes;
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
    width_policy: WidthPolicy,
) -> Result<FramePatch, GraphemeStoreError> {
    let size = next.size();
    let mut runs = Vec::new();

    for y in 0..size.height {
        let mut changed = vec![full_repaint; usize::from(size.width)];
        if !full_repaint {
            for x in 0..size.width {
                let point = Point::new(i32::from(x), i32::from(y));
                changed[usize::from(x)] = current.get(point) != next.get(point);
            }
            for x in 0..size.width {
                if changed[usize::from(x)] {
                    let point = Point::new(i32::from(x), i32::from(y));
                    include_grapheme_span(current, point, &mut changed);
                    include_grapheme_span(next, point, &mut changed);
                }
            }
        }

        let mut x = 0;
        while x < size.width {
            if !changed[usize::from(x)] {
                x += 1;
                continue;
            }

            let start = x;
            let mut cells = Vec::new();
            while x < size.width {
                let point = Point::new(i32::from(x), i32::from(y));
                if !changed[usize::from(x)] {
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

    let patch = FramePatch {
        size,
        runs,
        cursor: next_cursor,
        cursor_changed: current_cursor != next_cursor,
        full_repaint,
    };
    debug_assert_eq!(patch.validate_for_width_policy(width_policy), Ok(()));
    Ok(patch)
}

fn include_grapheme_span(buffer: &Buffer, point: Point, changed: &mut [bool]) {
    let Some(cell) = buffer.get(point) else {
        return;
    };
    let (start_x, width) = match cell.content {
        CellContent::Empty => return,
        CellContent::Grapheme { width, .. } => (point.x, width),
        CellContent::Continuation { id, offset } => {
            let start_x = point.x - i32::from(offset);
            match buffer
                .get(Point::new(start_x, point.y))
                .map(|cell| cell.content)
            {
                Some(CellContent::Grapheme {
                    id: leading_id,
                    width,
                }) if leading_id == id => (start_x, width),
                _ => return,
            }
        }
    };
    for offset in 0..width {
        let x = start_x + i32::from(offset);
        if let Ok(x) = usize::try_from(x) {
            if let Some(cell_changed) = changed.get_mut(x) {
                *cell_changed = true;
            }
        }
    }
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
    use crate::TextDraw;

    fn empty_full_patch(size: Size) -> Result<FramePatch, RenderError> {
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
        Ok(renderer
            .prepare(size, CursorState::HIDDEN, |_| Ok(()))?
            .patch()
            .clone())
    }

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
    fn wide_grapheme_insertion_is_one_complete_valid_span() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(4, 1), WidthPolicy::Unicode);
        let initial = renderer.prepare(Size::new(4, 1), CursorState::HIDDEN, |_| Ok(()))?;
        assert_eq!(renderer.commit(initial), Ok(()));

        let inserted = renderer.prepare(Size::new(4, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::new(1, 0), "界", Style::default(), None)?;
            Ok(())
        })?;

        assert_eq!(inserted.patch().validate(), Ok(()));
        assert_eq!(inserted.patch().runs.len(), 1);
        let run = &inserted.patch().runs[0];
        assert_eq!(run.position, Point::new(1, 0));
        assert_eq!(run.cells.len(), 2);
        let PatchCellContent::Grapheme { id, width: 2, .. } = run.cells[0].content else {
            panic!("wide span must start with its grapheme");
        };
        assert!(matches!(
            run.cells[1].content,
            PatchCellContent::Continuation {
                id: continuation_id,
                offset: 1,
            } if continuation_id == id
        ));
        Ok(())
    }

    #[test]
    fn replacing_wide_grapheme_with_narrow_clears_trailing_cell() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(4, 1), WidthPolicy::Unicode);
        let initial = renderer.prepare(Size::new(4, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::new(1, 0), "界", Style::default(), None)?;
            Ok(())
        })?;
        assert_eq!(renderer.commit(initial), Ok(()));

        let replacement = renderer.prepare(Size::new(4, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::new(1, 0), "x", Style::default(), None)?;
            Ok(())
        })?;
        let mut replay = renderer.current().clone();

        assert_eq!(replacement.patch().validate(), Ok(()));
        assert_eq!(replacement.patch().runs[0].position, Point::new(1, 0));
        assert_eq!(replacement.patch().runs[0].cells.len(), 2);
        assert!(matches!(
            &replacement.patch().runs[0].cells[0].content,
            PatchCellContent::Grapheme { text, width: 1, .. } if text.as_ref() == "x"
        ));
        assert_eq!(
            replacement.patch().runs[0].cells[1].content,
            PatchCellContent::Empty
        );
        assert_eq!(replacement.patch().apply_to(&mut replay), Ok(()));
        assert_eq!(&replay, replacement.buffer());
        Ok(())
    }

    #[test]
    fn clearing_wide_grapheme_covers_its_complete_old_span() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(4, 1), WidthPolicy::Unicode);
        let initial = renderer.prepare(Size::new(4, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::new(1, 0), "界", Style::default(), None)?;
            Ok(())
        })?;
        assert_eq!(renderer.commit(initial), Ok(()));

        let cleared = renderer.prepare(Size::new(4, 1), CursorState::HIDDEN, |_| Ok(()))?;

        assert_eq!(cleared.patch().validate(), Ok(()));
        assert_eq!(cleared.patch().runs.len(), 1);
        assert_eq!(cleared.patch().runs[0].position, Point::new(1, 0));
        assert_eq!(cleared.patch().runs[0].cells.len(), 2);
        assert!(
            cleared.patch().runs[0]
                .cells
                .iter()
                .all(|cell| cell.content == PatchCellContent::Empty)
        );
        Ok(())
    }

    #[test]
    fn clipped_wide_grapheme_at_last_column_leaves_no_continuation() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(3, 1), WidthPolicy::Unicode);
        let initial = renderer.prepare(Size::new(3, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::new(2, 0), "x", Style::default(), None)?;
            Ok(())
        })?;
        assert_eq!(renderer.commit(initial), Ok(()));

        let clipped = renderer.prepare(Size::new(3, 1), CursorState::HIDDEN, |canvas| {
            assert_eq!(
                canvas.draw_text(Point::new(2, 0), "界", Style::default(), None)?,
                TextDraw::default()
            );
            Ok(())
        })?;

        assert_eq!(clipped.patch().validate(), Ok(()));
        assert_eq!(clipped.patch().runs.len(), 1);
        assert_eq!(clipped.patch().runs[0].position, Point::new(2, 0));
        assert_eq!(
            clipped.patch().runs[0].cells[0].content,
            PatchCellContent::Empty
        );
        Ok(())
    }

    #[test]
    fn malformed_manual_continuations_are_rejected_before_apply() {
        let mut store = GraphemeStore::new();
        let id = store.intern("界").expect("test grapheme must intern");
        let mut patch = FramePatch {
            size: Size::new(2, 1),
            runs: vec![CellRun {
                position: Point::ORIGIN,
                cells: vec![PatchCell {
                    content: PatchCellContent::Continuation { id, offset: 1 },
                    style: Style::default(),
                    hyperlink: None,
                }],
            }],
            cursor: CursorState::HIDDEN,
            cursor_changed: false,
            full_repaint: false,
        };
        let mut buffer = Buffer::new(patch.size);
        let original = buffer.clone();

        assert_eq!(
            patch.validate(),
            Err(FramePatchValidationError::InvalidContinuation { run: 0, cell: 0 })
        );
        assert_eq!(
            patch.apply_to(&mut buffer),
            Err(BufferError::InvalidPatch(
                FramePatchValidationError::InvalidContinuation { run: 0, cell: 0 }
            ))
        );
        assert_eq!(buffer, original);

        patch.runs[0].cells.insert(
            0,
            PatchCell {
                content: PatchCellContent::Grapheme {
                    id,
                    text: Arc::from("界"),
                    width: 2,
                },
                style: Style::new().foreground(Color::Blue),
                hyperlink: None,
            },
        );
        assert_eq!(
            patch.validate(),
            Err(FramePatchValidationError::InvalidContinuation { run: 0, cell: 1 })
        );
    }

    #[test]
    fn nonempty_full_repaint_without_runs_is_not_empty_and_cannot_mutate_buffer()
    -> Result<(), RenderError> {
        let mut patch = empty_full_patch(Size::new(2, 2))?;
        patch.runs.clear();
        let mut buffer = Buffer::new(patch.size);
        buffer
            .set_empty(Point::ORIGIN, Style::new().background(Color::Blue))
            .expect("test point must be in bounds");
        let original = buffer.clone();

        assert!(!patch.is_empty());
        assert_eq!(
            patch.validate(),
            Err(FramePatchValidationError::IncompleteFullRepaint)
        );
        assert_eq!(
            patch.apply_to(&mut buffer),
            Err(BufferError::InvalidPatch(
                FramePatchValidationError::IncompleteFullRepaint
            ))
        );
        assert_eq!(buffer, original);
        Ok(())
    }

    #[test]
    fn full_repaint_requires_complete_rows() -> Result<(), RenderError> {
        let mut patch = empty_full_patch(Size::new(2, 2))?;
        patch.runs[0].cells.pop();

        assert_eq!(
            patch.validate(),
            Err(FramePatchValidationError::IncompleteFullRepaint)
        );
        Ok(())
    }

    #[test]
    fn overlapping_and_out_of_order_runs_are_rejected() -> Result<(), RenderError> {
        let patch = empty_full_patch(Size::new(2, 2))?;
        let mut overlapping = patch.clone();
        overlapping.runs.insert(1, overlapping.runs[0].clone());
        assert_eq!(
            overlapping.validate(),
            Err(FramePatchValidationError::OverlappingRuns {
                previous_run: 0,
                run: 1,
            })
        );

        let mut out_of_order = patch;
        out_of_order.runs.reverse();
        assert_eq!(
            out_of_order.validate(),
            Err(FramePatchValidationError::RunsOutOfOrder {
                previous_run: 0,
                run: 1,
            })
        );
        Ok(())
    }

    #[test]
    fn zero_area_full_repaint_remains_valid_and_empty() -> Result<(), RenderError> {
        let patch = empty_full_patch(Size::new(0, 2))?;

        assert_eq!(patch.validate(), Ok(()));
        assert!(patch.is_empty());
        Ok(())
    }

    #[test]
    fn width_policy_validation_rejects_malformed_grapheme_text() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;
        let mut patch = frame.patch().clone();
        if let PatchCellContent::Grapheme { text, .. } = &mut patch.runs[0].cells[0].content {
            *text = Arc::from("界");
        } else {
            panic!("test patch must contain a grapheme");
        }
        assert_eq!(patch.validate(), Ok(()));
        assert_eq!(
            patch.validate_for_width_policy(WidthPolicy::Unicode),
            Err(FramePatchValidationError::GraphemeWidthMismatch {
                run: 0,
                cell: 0,
                declared: 1,
                actual: 2,
            })
        );

        if let PatchCellContent::Grapheme { text, .. } = &mut patch.runs[0].cells[0].content {
            *text = Arc::from("ab");
        }
        assert_eq!(
            patch.validate_for_width_policy(WidthPolicy::Unicode),
            Err(FramePatchValidationError::InvalidGraphemeText { run: 0, cell: 0 })
        );

        if let PatchCellContent::Grapheme { text, .. } = &mut patch.runs[0].cells[0].content {
            *text = Arc::from("\n");
        }
        assert_eq!(
            patch.validate_for_width_policy(WidthPolicy::Unicode),
            Err(FramePatchValidationError::InvalidGraphemeText { run: 0, cell: 0 })
        );
        Ok(())
    }

    #[test]
    fn width_policy_validation_rejects_conflicting_grapheme_ids() {
        let mut store = GraphemeStore::new();
        let id = store.intern("a").expect("test grapheme must intern");
        let mut patch = FramePatch {
            size: Size::new(3, 1),
            runs: vec![
                CellRun {
                    position: Point::ORIGIN,
                    cells: vec![PatchCell {
                        content: PatchCellContent::Grapheme {
                            id,
                            text: Arc::from("a"),
                            width: 1,
                        },
                        style: Style::default(),
                        hyperlink: None,
                    }],
                },
                CellRun {
                    position: Point::new(2, 0),
                    cells: vec![PatchCell {
                        content: PatchCellContent::Grapheme {
                            id,
                            text: Arc::from("b"),
                            width: 1,
                        },
                        style: Style::default(),
                        hyperlink: None,
                    }],
                },
            ],
            cursor: CursorState::HIDDEN,
            cursor_changed: false,
            full_repaint: false,
        };

        assert_eq!(patch.validate(), Ok(()));
        assert_eq!(
            patch.validate_for_width_policy(WidthPolicy::Unicode),
            Err(FramePatchValidationError::ConflictingGraphemeId {
                id,
                first_run: 0,
                first_cell: 0,
                run: 1,
                cell: 0,
            })
        );

        let PatchCellContent::Grapheme { text, .. } = &mut patch.runs[1].cells[0].content else {
            panic!("test patch must contain a grapheme");
        };
        *text = Arc::from("a");
        assert_eq!(
            patch.validate_for_width_policy(WidthPolicy::Unicode),
            Ok(())
        );
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
    fn discarded_frames_do_not_retain_graphemes() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let retained_before = renderer.graphemes.len();

        for grapheme in ["a", "b", "c", "d"] {
            let prepared = renderer.prepare(Size::new(1, 1), CursorState::default(), |canvas| {
                canvas.draw_text(Point::ORIGIN, grapheme, Style::default(), None)?;
                Ok(())
            })?;
            renderer.discard(prepared);
        }

        assert_eq!(
            renderer.graphemes.len(),
            retained_before,
            "discarded frames must not retain their graphemes"
        );
        Ok(())
    }

    #[test]
    fn failed_frames_do_not_retain_graphemes() {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let retained_before = renderer.graphemes.len();

        let result = renderer.prepare(Size::new(1, 1), CursorState::default(), |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Err(DrawError::InvalidGrapheme)
        });

        assert_eq!(result, Err(RenderError::Draw(DrawError::InvalidGrapheme)));
        assert_eq!(renderer.graphemes.len(), retained_before);
    }

    #[test]
    fn committed_frames_release_replaced_graphemes() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);

        for grapheme in ["a", "b", "c", "d"] {
            let prepared = renderer.prepare(Size::new(1, 1), CursorState::default(), |canvas| {
                canvas.draw_text(Point::ORIGIN, grapheme, Style::default(), None)?;
                Ok(())
            })?;
            assert_eq!(renderer.commit(prepared), Ok(()));
            assert_eq!(renderer.graphemes.len(), 1);
        }
        Ok(())
    }

    #[test]
    fn concurrent_prepared_frames_keep_distinct_grapheme_ids() -> Result<(), RenderError> {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let first = renderer.prepare(Size::new(1, 1), CursorState::default(), |canvas| {
            canvas.draw_text(Point::ORIGIN, "a", Style::default(), None)?;
            Ok(())
        })?;
        let second = renderer.prepare(Size::new(1, 1), CursorState::default(), |canvas| {
            canvas.draw_text(Point::ORIGIN, "b", Style::default(), None)?;
            Ok(())
        })?;

        let first_content = &first.patch().runs[0].cells[0].content;
        let second_content = &second.patch().runs[0].cells[0].content;
        let (
            PatchCellContent::Grapheme {
                id: first_id,
                text: first_text,
                ..
            },
            PatchCellContent::Grapheme {
                id: second_id,
                text: second_text,
                ..
            },
        ) = (first_content, second_content)
        else {
            panic!("prepared patches must contain graphemes");
        };
        assert_ne!(first_id, second_id);
        assert_eq!(first_text.as_ref(), "a");
        assert_eq!(second_text.as_ref(), "b");

        assert_eq!(renderer.commit(first), Ok(()));
        assert_eq!(renderer.commit(second), Err(CommitError::StaleFrame));
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
