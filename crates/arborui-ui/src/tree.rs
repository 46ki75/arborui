use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use arborui_core::{CursorState, CursorVisibility, Point, Rect, Size, Style};
use arborui_layout::{LayoutError, LayoutNodeId, LayoutTree};
use arborui_render::{
    Buffer, Canvas, CommitError, DrawError, FramePatch, HitId, HitMap, PreparedFrame, RenderError,
    Renderer, RendererStateId,
};
use arborui_text::{WidthPolicy, measure};

use crate::{
    DispatchOutcome, Element, EventContext, EventPhase, FocusChange, FocusError, Invalidation, Key,
    KeyAction, NodeId, PointerButton, PointerEventKind, ReconcileError, ReconcileReport,
    RetainedNode, UiEvent, UiKey,
    event::{DispatchState, EventRequest},
    focus::{FocusManager, is_effectively_visible},
};

/// Errors produced by the headless UI pipeline.
#[derive(Debug)]
pub enum UiError {
    /// Declarative identity could not be reconciled.
    Reconcile(ReconcileError),
    /// Layout computation failed.
    Layout(LayoutError),
    /// Frame painting or preparation failed.
    Render(RenderError),
}

/// Failure to commit a prepared UI and renderer transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiCommitError {
    /// The frame was prepared by another logical UI tree.
    ForeignTree,
    /// Retained interaction state changed after frame preparation.
    StaleTree,
    /// Renderer state changed or the frame belongs to another renderer.
    Renderer(CommitError),
}

impl fmt::Display for UiCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignTree => formatter.write_str("prepared frame belongs to another UI tree"),
            Self::StaleTree => formatter.write_str("UI state advanced after frame preparation"),
            Self::Renderer(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for UiCommitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ForeignTree | Self::StaleTree => None,
            Self::Renderer(error) => Some(error),
        }
    }
}

impl fmt::Display for UiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reconcile(error) => error.fmt(formatter),
            Self::Layout(error) => error.fmt(formatter),
            Self::Render(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for UiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Reconcile(error) => Some(error),
            Self::Layout(error) => Some(error),
            Self::Render(error) => Some(error),
        }
    }
}

impl From<ReconcileError> for UiError {
    fn from(error: ReconcileError) -> Self {
        Self::Reconcile(error)
    }
}

impl From<LayoutError> for UiError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<RenderError> for UiError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

/// Retained identity and geometry for a headless UI.
///
/// A clone is an independent logical tree and cannot commit frames prepared by
/// its source tree.
#[derive(Debug, Default)]
pub struct UiTree {
    owner: UiTreeOwner,
    nodes: HashMap<NodeId, RetainedNode>,
    root: Option<NodeId>,
    next_id: u64,
    pending: Invalidation,
    viewport: Option<Size>,
    width_policy: Option<WidthPolicy>,
    focus: FocusManager,
    captured_pointer: Option<NodeId>,
    suppressed_pointer_buttons: u8,
    hovered: Option<NodeId>,
    last_pointer: Option<Point>,
    pending_focus_change: Option<FocusChange>,
    revision: u64,
    renderer_state: Option<RendererStateId>,
}

/// A rendered frame and the retained UI state that produced it.
///
/// Commit or discard this value through [`UiTree`] so logical interaction
/// state and the renderer's committed frame advance together.
pub struct PreparedUiFrame {
    owner: UiTreeOwner,
    frame: PreparedFrame,
    tree: UiTree,
    base_revision: u64,
}

/// Opt-in timing and logical repaint work for UI frame preparation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UiPreparationTimings {
    /// Time spent cloning retained state, reconciling the declarative tree, and
    /// applying viewport invalidation.
    pub staging_reconciliation: Duration,
    /// Time spent constructing and computing layout, assigning geometry,
    /// repairing focus state, and resolving the cursor.
    pub layout: Duration,
    /// Time spent painting through the renderer.
    pub paint: Duration,
    /// Time spent diffing and constructing the renderer's prepared frame.
    pub diff: Duration,
    /// Number of logical repaint regions cleared and replayed.
    pub repaint_regions: usize,
    /// Number of terminal cells covered by the logical repaint regions.
    pub repaint_cells: u32,
}

#[derive(Clone, Debug)]
struct UiTreeOwner(Arc<()>);

impl Default for UiTreeOwner {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}

impl UiTreeOwner {
    fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy)]
struct PaintContext {
    clip: Rect,
    hit: Option<HitId>,
    style: Style,
    focused: Option<NodeId>,
}

struct RowDamage {
    rows: Vec<bool>,
    prefix_cells: Vec<u32>,
    regions: usize,
    cells: u32,
}

impl RowDamage {
    fn new(rows: Vec<bool>, width: u16) -> Self {
        let mut prefix_cells = Vec::with_capacity(rows.len().saturating_add(1));
        prefix_cells.push(0_u32);
        let mut regions = 0;
        let mut previous = false;
        for &damaged in &rows {
            if damaged && !previous {
                regions += 1;
            }
            let cells = prefix_cells
                .last()
                .copied()
                .unwrap_or_default()
                .saturating_add(u32::from(damaged) * u32::from(width));
            prefix_cells.push(cells);
            previous = damaged;
        }
        let cells = prefix_cells.last().copied().unwrap_or_default();
        Self {
            rows,
            prefix_cells,
            regions,
            cells,
        }
    }

    fn intersects(&self, bounds: Rect) -> bool {
        let start = usize::try_from(bounds.y.max(0))
            .unwrap_or(usize::MAX)
            .min(self.rows.len());
        let end = usize::try_from(bounds.bottom().max(0))
            .unwrap_or(usize::MAX)
            .min(self.rows.len());
        self.prefix_cells[end] > self.prefix_cells[start]
    }
}

struct FrameLayout {
    root: NodeId,
    cursor: CursorState,
    focused: Option<NodeId>,
}

const ALL_POINTER_BUTTONS: u8 = 0b111;

const fn pointer_button_mask(button: PointerButton) -> u8 {
    match button {
        PointerButton::Primary => 1,
        PointerButton::Secondary => 1 << 1,
        PointerButton::Middle => 1 << 2,
    }
}

impl PreparedUiFrame {
    /// Returns the terminal-independent visual patch.
    #[must_use]
    pub const fn patch(&self) -> &FramePatch {
        self.frame.patch()
    }

    /// Returns the complete prepared logical buffer.
    #[must_use]
    pub const fn buffer(&self) -> &Buffer {
        self.frame.buffer()
    }

    /// Returns the interactive map prepared with the frame.
    #[must_use]
    pub const fn hit_map(&self) -> &HitMap {
        self.frame.hit_map()
    }
}

impl Clone for UiTree {
    fn clone(&self) -> Self {
        let mut cloned = self.clone_for_staging();
        cloned.owner = UiTreeOwner::default();
        cloned
    }
}

impl UiTree {
    /// Creates an empty retained tree.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the retained root identity.
    #[must_use]
    pub const fn root(&self) -> Option<NodeId> {
        self.root
    }

    /// Returns a retained node.
    #[must_use]
    pub fn node(&self, node: NodeId) -> Option<&RetainedNode> {
        self.nodes.get(&node)
    }

    /// Returns the number of retained nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns whether no retained nodes exist.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns the highest pending invalidation level.
    #[must_use]
    pub const fn pending_invalidation(&self) -> Invalidation {
        self.pending
    }

    /// Returns the focused node in the active focus scope.
    #[must_use]
    pub fn focused(&self) -> Option<NodeId> {
        self.focus.focused()
    }

    /// Returns the active focus scope.
    #[must_use]
    pub const fn active_focus_scope(&self) -> Option<NodeId> {
        self.focus.active_scope()
    }

    /// Returns the node holding UI pointer capture.
    #[must_use]
    pub const fn captured_pointer(&self) -> Option<NodeId> {
        self.captured_pointer
    }

    /// Returns the node currently under the last pointer position.
    #[must_use]
    pub const fn hovered(&self) -> Option<NodeId> {
        self.hovered
    }

    /// Returns and clears a focus transition produced outside event dispatch.
    pub fn take_focus_change(&mut self) -> Option<FocusChange> {
        let change = self.pending_focus_change.take();
        if change.is_some() {
            self.bump_revision();
        }
        change
    }

    /// Selects an interactive node from a committed renderer hit map.
    #[must_use]
    pub fn hit_test(&self, hit_map: &HitMap, point: Point) -> Option<NodeId> {
        let node = NodeId(hit_map.get(point)?.value());
        self.nodes
            .get(&node)
            .filter(|node| node.interactive)
            .map(|_| node)
    }

    /// Moves focus directly to a retained node in the active scope.
    pub fn focus_node(&mut self, node: NodeId) -> Result<FocusChange, FocusError> {
        let change = self
            .focus
            .focus(&self.nodes, self.root, node, self.viewport)?;
        self.record_focus_change(change);
        Ok(change)
    }

    /// Moves focus to a unique explicit key in the active scope.
    pub fn focus_key(&mut self, key: &Key) -> Result<FocusChange, FocusError> {
        let change = self
            .focus
            .focus_key(&self.nodes, self.root, key, self.viewport)?;
        self.record_focus_change(change);
        Ok(change)
    }

    /// Traverses focus in retained tree order, wrapping at scope boundaries.
    pub fn traverse_focus(&mut self, reverse: bool) -> FocusChange {
        let change = self
            .focus
            .traverse(&self.nodes, self.root, reverse, self.viewport);
        self.record_focus_change(change);
        change
    }

    /// Requests work for one retained node and escalates the tree request.
    pub fn invalidate(&mut self, node: NodeId, requested: Invalidation) -> bool {
        let Some(node) = self.nodes.get_mut(&node) else {
            return false;
        };
        node.invalidation.request(requested);
        self.pending.request(requested);
        self.bump_revision();
        true
    }

    /// Commits a prepared visual frame and its matching retained UI state.
    pub fn commit(
        &mut self,
        prepared: PreparedUiFrame,
        renderer: &mut Renderer,
    ) -> Result<(), UiCommitError> {
        if !self.owner.is_same(&prepared.owner) {
            return Err(UiCommitError::ForeignTree);
        }
        if self.revision != prepared.base_revision {
            return Err(UiCommitError::StaleTree);
        }
        renderer
            .commit(prepared.frame)
            .map_err(UiCommitError::Renderer)?;
        let mut tree = prepared.tree;
        tree.renderer_state = Some(renderer.state_id());
        *self = tree;
        Ok(())
    }

    /// Discards a prepared frame without advancing retained or rendered state.
    pub fn discard(&mut self, prepared: PreparedUiFrame, renderer: &mut Renderer) {
        renderer.discard(prepared.frame);
    }

    /// Routes one event through handlers borrowed by the current element tree.
    ///
    /// Capture visits root through target, target visits the selected node,
    /// and bubble visits target through root. Handler
    /// requests are applied after routing; the last focus or capture request
    /// wins. `handled`, default prevention, and propagation are independent.
    pub fn dispatch<Message>(
        &mut self,
        element: &Element<'_, Message>,
        event: &UiEvent,
        renderer: &Renderer,
    ) -> Result<DispatchOutcome<Message>, ReconcileError> {
        if self.renderer_state != Some(renderer.state_id()) {
            return Err(ReconcileError::WrongCommittedRenderer);
        }
        self.dispatch_with_hit_map(element, event, renderer.hit_map())
    }

    fn dispatch_with_hit_map<Message>(
        &mut self,
        element: &Element<'_, Message>,
        event: &UiEvent,
        hit_map: &HitMap,
    ) -> Result<DispatchOutcome<Message>, ReconcileError> {
        validate_keys(element)?;
        if !self.view_matches_committed(element) {
            return Err(ReconcileError::ViewDoesNotMatchCommittedTree);
        }
        self.bump_revision();
        let root = self.root.expect("reconciliation always creates a root");
        let mut elements = HashMap::with_capacity(self.nodes.len());
        self.collect_elements(root, element, &mut elements);

        if let Some(pointer) = event.pointer() {
            let suppressed = match pointer.kind {
                PointerEventKind::Drag(button) | PointerEventKind::Up(button) => {
                    self.suppressed_pointer_buttons & pointer_button_mask(button) != 0
                }
                PointerEventKind::Down(button) => {
                    self.suppressed_pointer_buttons &= !pointer_button_mask(button);
                    false
                }
                PointerEventKind::Moved
                | PointerEventKind::Scroll(_)
                | PointerEventKind::ScrollHorizontal(_) => false,
            };
            if suppressed {
                self.last_pointer = Some(pointer.position);
                if let PointerEventKind::Up(button) = pointer.kind {
                    self.suppressed_pointer_buttons &= !pointer_button_mask(button);
                }
                let mut state = DispatchState::new();
                state.handled = true;
                state.default_prevented = true;
                self.update_hover(pointer.position, hit_map, &elements, &mut state);
                return Ok(DispatchOutcome::from_state(None, state));
            }
        }

        let target = if let Some(pointer) = event.pointer() {
            self.last_pointer = Some(pointer.position);
            let candidate = if matches!(
                pointer.kind,
                PointerEventKind::Drag(_) | PointerEventKind::Up(_)
            ) {
                self.captured_pointer
                    .filter(|node| self.nodes.contains_key(node))
                    .or_else(|| self.hit_test(hit_map, pointer.position))
            } else {
                self.hit_test(hit_map, pointer.position)
            };
            self.pointer_target(candidate).or(Some(root))
        } else if matches!(
            event,
            UiEvent::Key(_) | UiEvent::Text(_) | UiEvent::Paste(_)
        ) {
            self.focused().or(self.active_focus_scope()).or(Some(root))
        } else {
            Some(root)
        };

        let mut state = target.map_or_else(DispatchState::new, |target| {
            self.invoke_route(target, event, &elements)
        });
        let focus_origin = self
            .pending_focus_change
            .take()
            .map_or_else(|| self.focused(), |change| change.previous);

        if !state.default_prevented {
            match event {
                UiEvent::Key(key)
                    if key.key == UiKey::Tab
                        && key.action == KeyAction::Press
                        && matches!(
                            key.modifiers,
                            crate::KeyModifiers::NONE | crate::KeyModifiers::SHIFT
                        ) =>
                {
                    let reverse = key.modifiers.contains(crate::KeyModifiers::SHIFT);
                    let _ = self.traverse_focus(reverse);
                }
                UiEvent::Pointer(pointer) if matches!(pointer.kind, PointerEventKind::Down(_)) => {
                    if let Some(target) = target {
                        if let Some(target) = self.nearest_focusable(target) {
                            let _ = self.focus_node(target);
                        }
                    }
                }
                _ => {}
            }
        }

        let requests = std::mem::take(&mut state.requests);
        self.apply_requests(requests, true);
        let focus_change = FocusChange {
            previous: focus_origin,
            current: self.focused(),
        };
        self.pending_focus_change = None;
        if focus_change.changed() {
            if let Some(previous) = focus_change.previous {
                let transition = self.invoke_target(previous, &UiEvent::FocusLost, &elements);
                self.merge_transition(&mut state, transition);
            }
            if let Some(current) = focus_change.current {
                let transition = self.invoke_target(current, &UiEvent::FocusGained, &elements);
                self.merge_transition(&mut state, transition);
            }
        }

        if let Some(pointer) = event.pointer() {
            self.update_hover(pointer.position, hit_map, &elements, &mut state);
        }

        Ok(DispatchOutcome::from_state(target, state))
    }

    /// Delivers pending focus transitions and recomputes hover from a committed hit map.
    ///
    /// Call this after committing a prepared frame. Focus, enter, and leave are
    /// target-only, non-cancelable transitions.
    pub fn refresh_hover<Message>(
        &mut self,
        element: &Element<'_, Message>,
        renderer: &Renderer,
    ) -> Result<DispatchOutcome<Message>, ReconcileError> {
        if self.renderer_state != Some(renderer.state_id()) {
            return Err(ReconcileError::WrongCommittedRenderer);
        }
        self.refresh_hover_with_hit_map(element, renderer.hit_map())
    }

    fn refresh_hover_with_hit_map<Message>(
        &mut self,
        element: &Element<'_, Message>,
        hit_map: &HitMap,
    ) -> Result<DispatchOutcome<Message>, ReconcileError> {
        validate_keys(element)?;
        if !self.view_matches_committed(element) {
            return Err(ReconcileError::ViewDoesNotMatchCommittedTree);
        }
        self.bump_revision();
        let root = self.root.expect("reconciliation always creates a root");
        let mut elements = HashMap::with_capacity(self.nodes.len());
        self.collect_elements(root, element, &mut elements);
        let mut state = DispatchState::new();
        if let Some(change) = self.pending_focus_change.take() {
            if let Some(previous) = change.previous {
                let transition = self.invoke_target(previous, &UiEvent::FocusLost, &elements);
                self.merge_transition(&mut state, transition);
            }
            if let Some(current) = change.current {
                let transition = self.invoke_target(current, &UiEvent::FocusGained, &elements);
                self.merge_transition(&mut state, transition);
            }
        }

        let next = self.pointer_target(
            self.last_pointer
                .and_then(|position| self.hit_test(hit_map, position)),
        );
        let previous = self.hovered;
        if previous == next {
            return Ok(DispatchOutcome::from_state(next, state));
        }
        self.hovered = next;

        if let Some(previous) = previous {
            let transition = self.invoke_target(previous, &UiEvent::PointerLeft, &elements);
            self.merge_transition(&mut state, transition);
        }
        if let Some(next) = next {
            let transition = self.invoke_target(next, &UiEvent::PointerEntered, &elements);
            self.merge_transition(&mut state, transition);
        }
        Ok(DispatchOutcome::from_state(next, state))
    }

    /// Reconciles a borrowed declarative tree into owned retained metadata.
    pub fn reconcile<Message>(
        &mut self,
        element: &Element<'_, Message>,
    ) -> Result<ReconcileReport, ReconcileError> {
        validate_keys(element)?;
        let mut report = ReconcileReport::default();
        let root = self.reconcile_node(None, self.root, element, &mut report);
        self.root = Some(root);
        let focus_change = self.focus.sync(&self.nodes, self.root, self.viewport);
        self.record_focus_change(focus_change);
        self.repair_removed_interaction();
        self.repair_pointer_modality();
        report.invalidation = self.pending;
        self.renderer_state = None;
        self.bump_revision();
        Ok(report)
    }

    /// Reconciles and prepares a complete headless frame, reusing committed
    /// geometry and unchanged committed logical content when safe.
    pub fn prepare<Message>(
        &self,
        element: &Element<'_, Message>,
        viewport: Size,
        renderer: &mut Renderer,
    ) -> Result<PreparedUiFrame, UiError> {
        self.prepare_with_layout_mode(element, viewport, renderer, false)
    }

    /// Reconciles, fully lays out, and paints a headless reference frame.
    ///
    /// Unlike [`prepare`](Self::prepare), this path always recomputes layout so
    /// optimized preparation can be checked against a full-layout reference.
    pub fn prepare_full<Message>(
        &self,
        element: &Element<'_, Message>,
        viewport: Size,
        renderer: &mut Renderer,
    ) -> Result<PreparedUiFrame, UiError> {
        self.prepare_with_layout_mode(element, viewport, renderer, true)
    }

    fn prepare_with_layout_mode<Message>(
        &self,
        element: &Element<'_, Message>,
        viewport: Size,
        renderer: &mut Renderer,
        force_layout: bool,
    ) -> Result<PreparedUiFrame, UiError> {
        let renderer_matches = self.renderer_state == Some(renderer.state_id());
        let mut staged = self.clone_for_staging();
        let width_policy = renderer.width_policy();
        staged.stage_frame(element, viewport, width_policy)?;
        let layout = staged.prepare_frame_layout(element, viewport, width_policy, force_layout)?;
        let frame = match (force_layout, renderer_matches, staged.pending) {
            (false, true, Invalidation::None) => renderer.prepare_reused(layout.cursor)?,
            (false, true, Invalidation::Paint) => {
                let damage = staged.damaged_rows(viewport);
                renderer.prepare_from_current(layout.cursor, |canvas| {
                    staged.repaint_damaged_rows(
                        element,
                        canvas,
                        &damage,
                        layout.root,
                        layout.focused,
                    )
                })?
            }
            _ => renderer.prepare(viewport, layout.cursor, |canvas| {
                staged.paint_frame(element, canvas, viewport, layout.root, layout.focused)
            })?,
        };
        staged.finish_frame();
        Ok(PreparedUiFrame {
            owner: self.owner.clone(),
            frame,
            tree: staged,
            base_revision: self.revision,
        })
    }

    /// Reconciles and prepares a complete headless frame while measuring each
    /// phase and reusing clean committed geometry and logical content.
    pub fn prepare_timed<Message>(
        &self,
        element: &Element<'_, Message>,
        viewport: Size,
        renderer: &mut Renderer,
    ) -> Result<(PreparedUiFrame, UiPreparationTimings), UiError> {
        let staging_started = Instant::now();
        let renderer_matches = self.renderer_state == Some(renderer.state_id());
        let mut staged = self.clone_for_staging();
        let width_policy = renderer.width_policy();
        staged.stage_frame(element, viewport, width_policy)?;
        let staging_reconciliation = staging_started.elapsed();

        let (layout, layout_elapsed) = if staged.pending >= Invalidation::Layout {
            let layout_started = Instant::now();
            let layout = staged.layout_frame(element, viewport, width_policy)?;
            (layout, layout_started.elapsed())
        } else {
            (
                staged.resolve_frame_layout(element, viewport, width_policy),
                Duration::ZERO,
            )
        };

        let (frame, renderer_timings, repaint_regions, repaint_cells) =
            match (renderer_matches, staged.pending) {
                (true, Invalidation::None) => {
                    let (frame, timings) = renderer.prepare_reused_timed(layout.cursor)?;
                    (frame, timings, 0, 0)
                }
                (true, Invalidation::Paint) => {
                    let damage = staged.damaged_rows(viewport);
                    let repaint_regions = damage.regions;
                    let repaint_cells = damage.cells;
                    let (frame, timings) =
                        renderer.prepare_from_current_timed(layout.cursor, |canvas| {
                            staged.repaint_damaged_rows(
                                element,
                                canvas,
                                &damage,
                                layout.root,
                                layout.focused,
                            )
                        })?;
                    (frame, timings, repaint_regions, repaint_cells)
                }
                _ => {
                    let (frame, timings) =
                        renderer.prepare_timed(viewport, layout.cursor, |canvas| {
                            staged.paint_frame(
                                element,
                                canvas,
                                viewport,
                                layout.root,
                                layout.focused,
                            )
                        })?;
                    (
                        frame,
                        timings,
                        usize::from(!viewport.is_empty()),
                        viewport.area(),
                    )
                }
            };
        staged.finish_frame();
        let prepared = PreparedUiFrame {
            owner: self.owner.clone(),
            frame,
            tree: staged,
            base_revision: self.revision,
        };
        Ok((
            prepared,
            UiPreparationTimings {
                staging_reconciliation,
                layout: layout_elapsed,
                paint: renderer_timings.paint,
                diff: renderer_timings.diff,
                repaint_regions,
                repaint_cells,
            },
        ))
    }

    fn clone_for_staging(&self) -> Self {
        Self {
            owner: self.owner.clone(),
            nodes: self.nodes.clone(),
            root: self.root,
            next_id: self.next_id,
            pending: self.pending,
            viewport: self.viewport,
            width_policy: self.width_policy,
            focus: self.focus.clone(),
            captured_pointer: self.captured_pointer,
            suppressed_pointer_buttons: self.suppressed_pointer_buttons,
            hovered: self.hovered,
            last_pointer: self.last_pointer,
            pending_focus_change: self.pending_focus_change,
            revision: self.revision,
            renderer_state: self.renderer_state,
        }
    }

    fn stage_frame<Message>(
        &mut self,
        element: &Element<'_, Message>,
        viewport: Size,
        width_policy: WidthPolicy,
    ) -> Result<(), UiError> {
        self.reconcile(element)?;
        if self.viewport != Some(viewport) {
            self.pending.request(Invalidation::Layout);
            self.viewport = Some(viewport);
        }
        if self.width_policy != Some(width_policy) {
            self.pending.request(Invalidation::Layout);
            self.width_policy = Some(width_policy);
        }
        Ok(())
    }

    fn prepare_frame_layout<Message>(
        &mut self,
        element: &Element<'_, Message>,
        viewport: Size,
        width_policy: WidthPolicy,
        force_layout: bool,
    ) -> Result<FrameLayout, UiError> {
        if force_layout || self.pending >= Invalidation::Layout {
            self.layout_frame(element, viewport, width_policy)
        } else {
            Ok(self.resolve_frame_layout(element, viewport, width_policy))
        }
    }

    fn layout_frame<Message>(
        &mut self,
        element: &Element<'_, Message>,
        viewport: Size,
        width_policy: WidthPolicy,
    ) -> Result<FrameLayout, UiError> {
        let root = self.root.expect("reconciliation always creates a root");
        let mut layout_tree = LayoutTree::new();
        let mut mapping = Vec::with_capacity(self.nodes.len());
        let layout_root = self.build_layout(root, element, &mut layout_tree, &mut mapping)?;
        let by_layout = mapping
            .iter()
            .map(|(layout, _, element)| (*layout, *element))
            .collect::<HashMap<_, _>>();
        layout_tree.compute(layout_root, viewport, |node, input| {
            let Some(element) = by_layout.get(&node) else {
                return Size::ZERO;
            };
            let Some(text) = element.text_content() else {
                return Size::ZERO;
            };
            let metrics = measure(text, width_policy);
            Size::new(
                input.known_width.unwrap_or(saturating_u16(metrics.width)),
                input.known_height.unwrap_or(saturating_u16(metrics.height)),
            )
        })?;
        let by_retained_layout = mapping
            .iter()
            .map(|(layout, retained, _)| (*retained, *layout))
            .collect::<HashMap<_, _>>();
        self.assign_layout(
            root,
            element,
            &layout_tree,
            &by_retained_layout,
            Point::ORIGIN,
            width_policy,
        )?;

        Ok(self.resolve_frame_layout(element, viewport, width_policy))
    }

    fn resolve_frame_layout<Message>(
        &mut self,
        element: &Element<'_, Message>,
        viewport: Size,
        width_policy: WidthPolicy,
    ) -> FrameLayout {
        let root = self.root.expect("reconciliation always creates a root");
        let focus_change = self.focus.sync(&self.nodes, self.root, self.viewport);
        self.record_focus_change(focus_change);
        let focus_change = self.focus.repair(&self.nodes, self.root, self.viewport);
        self.record_focus_change(focus_change);
        self.repair_removed_interaction();
        self.repair_pointer_modality();
        let mut by_retained = HashMap::with_capacity(self.nodes.len());
        self.collect_elements(root, element, &mut by_retained);
        let cursor = self.resolve_cursor(&by_retained, viewport, width_policy);
        let focused = self.focused();
        FrameLayout {
            root,
            cursor,
            focused,
        }
    }

    fn paint_frame<Message>(
        &self,
        element: &Element<'_, Message>,
        canvas: &mut Canvas<'_>,
        viewport: Size,
        root: NodeId,
        focused: Option<NodeId>,
    ) -> Result<(), DrawError> {
        self.paint_node(
            root,
            element,
            canvas,
            PaintContext {
                clip: Rect::from_origin_size(Point::ORIGIN, viewport),
                hit: None,
                style: Style::default(),
                focused,
            },
            None,
        )
    }

    fn repaint_damaged_rows<Message>(
        &self,
        element: &Element<'_, Message>,
        canvas: &mut Canvas<'_>,
        damage: &RowDamage,
        root: NodeId,
        focused: Option<NodeId>,
    ) -> Result<(), DrawError> {
        let clip = canvas.clip();
        let mut canvas = canvas.with_damage_rows(&damage.rows);
        canvas.fill(clip, Style::default())?;
        self.paint_node(
            root,
            element,
            &mut canvas,
            PaintContext {
                clip,
                hit: None,
                style: Style::default(),
                focused,
            },
            Some(damage),
        )
    }

    fn damaged_rows(&self, viewport: Size) -> RowDamage {
        let viewport = Rect::from_origin_size(Point::ORIGIN, viewport);
        let mut rows = vec![false; usize::from(viewport.height)];
        let mut include = |bounds: Rect| {
            let Some(damage) = bounds.intersection(viewport) else {
                return;
            };
            let start = usize::try_from(damage.y).expect("viewport-clipped row is nonnegative");
            let end =
                usize::try_from(damage.bottom()).expect("viewport-clipped row end is nonnegative");
            rows[start..end].fill(true);
        };
        for node in self
            .nodes
            .values()
            .filter(|node| node.invalidation == Invalidation::Paint)
        {
            include(node.layout);
        }
        if let Some(change) = self.pending_focus_change {
            for node in [change.previous, change.current].into_iter().flatten() {
                if let Some(node) = self.nodes.get(&node) {
                    include(node.layout);
                }
            }
        }
        RowDamage::new(rows, viewport.width)
    }

    fn finish_frame(&mut self) {
        self.pending = Invalidation::None;
        for node in self.nodes.values_mut() {
            node.invalidation = Invalidation::None;
        }
    }

    fn reconcile_node<Message>(
        &mut self,
        parent: Option<NodeId>,
        candidate: Option<NodeId>,
        element: &Element<'_, Message>,
        report: &mut ReconcileReport,
    ) -> NodeId {
        let compatible = candidate.is_some_and(|node| {
            self.nodes.get(&node).is_some_and(|retained| {
                retained.kind == element.kind() && retained.key.as_ref() == element.explicit_key()
            })
        });
        let node_id = if compatible {
            report.reused += 1;
            candidate.expect("compatible candidate exists")
        } else {
            if let Some(candidate) = candidate {
                self.remove_subtree(candidate, report);
            }
            let node = self.allocate_node(parent, element);
            report.created += 1;
            self.pending.request(Invalidation::Recompose);
            node
        };

        if compatible {
            let fingerprint = content_fingerprint(element);
            let mut requested = Invalidation::None;
            {
                let retained = self
                    .nodes
                    .get_mut(&node_id)
                    .expect("compatible node exists");
                retained.parent = parent;
                if retained.layout_style != element.layout_style()
                    || retained.content_fingerprint != fingerprint
                {
                    requested.request(Invalidation::Layout);
                } else if retained.visual_style != element.visual_style()
                    || retained.focus_style != element.focused_style()
                {
                    requested.request(Invalidation::Paint);
                }
                if retained.interactive != element.is_interactive()
                    || retained.focusable != element.is_focusable()
                    || retained.focus_scope != element.is_focus_scope()
                    || retained.pointer_modal != element.is_pointer_modal()
                    || retained.focus_order != element.explicit_focus_order()
                    || retained.cursor_intent != element.fixed_cursor_intent()
                    || retained.cursor_fingerprint != element.cursor_fingerprint()
                    || retained.dynamic_cursor != element.has_dynamic_cursor()
                    || retained.fill_background != element.fills_background()
                {
                    requested.request(Invalidation::Paint);
                }
                if retained.child_offset != element.fixed_children_offset()
                    || retained.child_offset_fingerprint != element.child_offset_fingerprint()
                    || retained.dynamic_child_offset != element.has_dynamic_child_offset()
                {
                    requested.request(Invalidation::Layout);
                }
                if retained.paint_fingerprint != element.paint_fingerprint() {
                    requested.request(Invalidation::Paint);
                }
                retained.layout_style = element.layout_style();
                retained.visual_style = element.visual_style();
                retained.focus_style = element.focused_style();
                retained.content_fingerprint = fingerprint;
                retained.interactive = element.is_interactive();
                retained.focusable = element.is_focusable();
                retained.focus_scope = element.is_focus_scope();
                retained.pointer_modal = element.is_pointer_modal();
                retained.focus_order = element.explicit_focus_order();
                retained.cursor_intent = element.fixed_cursor_intent();
                retained.cursor_fingerprint = element.cursor_fingerprint();
                retained.dynamic_cursor = element.has_dynamic_cursor();
                retained.child_offset = element.fixed_children_offset();
                retained.child_offset_fingerprint = element.child_offset_fingerprint();
                retained.dynamic_child_offset = element.has_dynamic_child_offset();
                retained.fill_background = element.fills_background();
                retained.paint_fingerprint = element.paint_fingerprint();
                retained.invalidation.request(requested);
            }
            self.pending.request(requested);
        }

        let old_children = self.nodes[&node_id].children.clone();
        let keyed = old_children
            .iter()
            .filter_map(|child| {
                self.nodes[child]
                    .key
                    .as_ref()
                    .map(|key| (key.clone(), *child))
            })
            .collect::<HashMap<_, _>>();
        let mut new_children = Vec::with_capacity(element.children().len());
        for (index, child) in element.children().iter().enumerate() {
            let candidate = match child.explicit_key() {
                Some(key) => keyed.get(key).copied(),
                None => old_children
                    .get(index)
                    .copied()
                    .filter(|node| self.nodes.get(node).is_some_and(|node| node.key.is_none())),
            };
            new_children.push(self.reconcile_node(Some(node_id), candidate, child, report));
        }
        for old in old_children {
            if self.nodes.contains_key(&old) && !new_children.contains(&old) {
                self.remove_subtree(old, report);
            }
        }
        let retained = self
            .nodes
            .get_mut(&node_id)
            .expect("reconciled node exists");
        if retained.children != new_children {
            retained.invalidation.request(Invalidation::Recompose);
            self.pending.request(Invalidation::Recompose);
        }
        retained.children = new_children;
        node_id
    }

    fn allocate_node<Message>(
        &mut self,
        parent: Option<NodeId>,
        element: &Element<'_, Message>,
    ) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        self.nodes.insert(
            id,
            RetainedNode {
                key: element.explicit_key().cloned(),
                kind: element.kind(),
                parent,
                children: Vec::new(),
                layout: Rect::ZERO,
                content: Rect::ZERO,
                layout_style: element.layout_style(),
                visual_style: element.visual_style(),
                focus_style: element.focused_style(),
                content_fingerprint: content_fingerprint(element),
                invalidation: Invalidation::Recompose,
                interactive: element.is_interactive(),
                focusable: element.is_focusable(),
                focus_scope: element.is_focus_scope(),
                pointer_modal: element.is_pointer_modal(),
                focus_order: element.explicit_focus_order(),
                cursor_intent: element.fixed_cursor_intent(),
                cursor_fingerprint: element.cursor_fingerprint(),
                dynamic_cursor: element.has_dynamic_cursor(),
                child_offset: element.fixed_children_offset(),
                child_offset_fingerprint: element.child_offset_fingerprint(),
                dynamic_child_offset: element.has_dynamic_child_offset(),
                fill_background: element.fills_background(),
                paint_fingerprint: element.paint_fingerprint(),
            },
        );
        id
    }

    fn remove_subtree(&mut self, node: NodeId, report: &mut ReconcileReport) {
        let Some(retained) = self.nodes.remove(&node) else {
            return;
        };
        report.removed += 1;
        self.pending.request(Invalidation::Recompose);
        if self.captured_pointer == Some(node) {
            self.captured_pointer = None;
            self.suppressed_pointer_buttons |= ALL_POINTER_BUTTONS;
        }
        if self.hovered == Some(node) {
            self.hovered = None;
        }
        for child in retained.children {
            self.remove_subtree(child, report);
        }
    }

    fn build_layout<'a, Message>(
        &self,
        retained: NodeId,
        element: &'a Element<'a, Message>,
        tree: &mut LayoutTree,
        mapping: &mut Vec<(LayoutNodeId, NodeId, &'a Element<'a, Message>)>,
    ) -> Result<LayoutNodeId, LayoutError> {
        let retained_node = self
            .nodes
            .get(&retained)
            .expect("element and retained tree have matching structure");
        let children = retained_node
            .children
            .iter()
            .zip(element.children())
            .map(|(child, element)| self.build_layout(*child, element, tree, mapping))
            .collect::<Result<Vec<_>, _>>()?;
        let layout = tree.add_with_children(element.layout_style(), &children)?;
        mapping.push((layout, retained, element));
        Ok(layout)
    }

    fn assign_layout<Message>(
        &mut self,
        retained: NodeId,
        element: &Element<'_, Message>,
        tree: &LayoutTree,
        mapping: &HashMap<NodeId, LayoutNodeId>,
        offset: Point,
        width_policy: WidthPolicy,
    ) -> Result<(), LayoutError> {
        let layout = tree.layout(mapping[&retained])?;
        let bounds = layout.bounds.translated(offset.x, offset.y);
        let content = layout.content.translated(offset.x, offset.y);
        let node = self
            .nodes
            .get_mut(&retained)
            .expect("mapped retained node exists");
        node.layout = bounds;
        node.content = content;
        let child_offset = element.children_offset(bounds.size(), width_policy);
        let offset = offset.translated(child_offset.x, child_offset.y);
        let children = self.nodes[&retained].children.clone();
        for (child, child_element) in children.into_iter().zip(element.children()) {
            self.assign_layout(child, child_element, tree, mapping, offset, width_policy)?;
        }
        Ok(())
    }

    fn paint_node<Message>(
        &self,
        retained: NodeId,
        element: &Element<'_, Message>,
        canvas: &mut Canvas<'_>,
        inherited: PaintContext,
        damage: Option<&RowDamage>,
    ) -> Result<(), DrawError> {
        let node = self
            .nodes
            .get(&retained)
            .expect("element and retained tree have matching structure");
        let Some(clip) = inherited.clip.intersection(node.layout) else {
            return Ok(());
        };
        if damage.is_some_and(|damage| !damage.intersects(clip)) {
            return Ok(());
        }
        let style = inherit_style(inherited.style, node.visual_style);
        let style = if inherited.focused == Some(retained) {
            inherit_style(style, node.focus_style)
        } else {
            style
        };
        let hit = if element.is_interactive() {
            Some(HitId::new(retained.0))
        } else {
            inherited.hit
        };
        {
            let mut scoped = canvas.scoped(clip, node.layout.origin()).with_hit(hit);
            if element.fills_background() {
                scoped.fill(
                    Rect::new(0, 0, node.layout.width, node.layout.height),
                    style,
                )?;
            }
            if let Some(text) = element.text_content() {
                let text_origin = Point::new(
                    node.content.x.saturating_sub(node.layout.x),
                    node.content.y.saturating_sub(node.layout.y),
                );
                let mut text_canvas = scoped.scoped(node.content, node.layout.origin());
                text_canvas.draw_text(text_origin, text, style, None)?;
            }
            element.paint_content(node.layout.size(), &mut scoped)?;
        }
        let children_clip = clip.intersection(node.content).unwrap_or(Rect::ZERO);
        for (child, element) in node.children.iter().zip(element.children()) {
            self.paint_node(
                *child,
                element,
                canvas,
                PaintContext {
                    clip: children_clip,
                    hit,
                    style,
                    focused: inherited.focused,
                },
                damage,
            )?;
        }
        Ok(())
    }

    fn resolve_cursor<Message>(
        &self,
        elements: &HashMap<NodeId, &Element<'_, Message>>,
        viewport: Size,
        width_policy: WidthPolicy,
    ) -> CursorState {
        let Some(focused) = self.focused() else {
            return CursorState::HIDDEN;
        };
        let (Some(node), Some(element)) = (self.nodes.get(&focused), elements.get(&focused)) else {
            return CursorState::HIDDEN;
        };
        let Some(mut cursor) = element.cursor_intent(width_policy, node.layout.size()) else {
            return CursorState::HIDDEN;
        };
        if cursor.visibility == CursorVisibility::Hidden {
            return CursorState::HIDDEN;
        }
        cursor.position = node
            .layout
            .origin()
            .translated(cursor.position.x, cursor.position.y);
        let viewport = Rect::from_origin_size(Point::ORIGIN, viewport);
        let mut clip = Some(viewport);
        let mut current = Some(focused);
        let mut target = true;
        while let Some(candidate) = current {
            let Some(retained) = self.nodes.get(&candidate) else {
                return CursorState::HIDDEN;
            };
            let bounds = if target {
                retained.layout
            } else {
                retained.content
            };
            clip = clip.and_then(|clip| clip.intersection(bounds));
            target = false;
            current = retained.parent;
        }
        if !clip.is_some_and(|clip| clip.contains(cursor.position)) {
            return CursorState::HIDDEN;
        }
        cursor
    }

    fn repair_removed_interaction(&mut self) {
        if self
            .captured_pointer
            .is_some_and(|node| !self.nodes.contains_key(&node))
        {
            self.captured_pointer = None;
            self.suppressed_pointer_buttons |= ALL_POINTER_BUTTONS;
        }
        if self
            .hovered
            .is_some_and(|node| !self.nodes.contains_key(&node))
        {
            self.hovered = None;
        }
    }

    fn repair_pointer_modality(&mut self) {
        let Some(scope) = self.active_pointer_modal() else {
            return;
        };
        if self
            .captured_pointer
            .is_some_and(|node| !self.is_descendant_of(node, scope))
        {
            self.captured_pointer = None;
            self.suppressed_pointer_buttons |= ALL_POINTER_BUTTONS;
        }
    }

    fn update_hover<Message>(
        &mut self,
        position: Point,
        hit_map: &HitMap,
        elements: &HashMap<NodeId, &Element<'_, Message>>,
        state: &mut DispatchState<Message>,
    ) {
        let next = self.pointer_target(self.hit_test(hit_map, position));
        let previous = self.hovered;
        if previous == next {
            return;
        }
        self.hovered = next;
        if let Some(previous) = previous {
            let transition = self.invoke_target(previous, &UiEvent::PointerLeft, elements);
            self.merge_transition(state, transition);
        }
        if let Some(next) = next {
            let transition = self.invoke_target(next, &UiEvent::PointerEntered, elements);
            self.merge_transition(state, transition);
        }
    }

    fn pointer_target(&self, candidate: Option<NodeId>) -> Option<NodeId> {
        let Some(scope) = self.active_pointer_modal() else {
            return candidate;
        };
        candidate
            .filter(|node| self.is_descendant_of(*node, scope))
            .or(Some(scope))
    }

    fn active_pointer_modal(&self) -> Option<NodeId> {
        let root = self.root?;
        let mut order = 0_usize;
        let mut active = None;
        self.visit_pointer_modals(root, 0, &mut order, &mut active);
        active.map(|(_, _, node)| node)
    }

    fn visit_pointer_modals(
        &self,
        node: NodeId,
        depth: usize,
        order: &mut usize,
        active: &mut Option<(usize, usize, NodeId)>,
    ) {
        let Some(retained) = self.nodes.get(&node) else {
            return;
        };
        let node_order = *order;
        *order = (*order).saturating_add(1);
        if retained.pointer_modal
            && is_effectively_visible(&self.nodes, node, self.viewport)
            && active
                .as_ref()
                .is_none_or(|(active_depth, active_order, _)| {
                    (depth, node_order) > (*active_depth, *active_order)
                })
        {
            *active = Some((depth, node_order, node));
        }
        for child in &retained.children {
            self.visit_pointer_modals(*child, depth.saturating_add(1), order, active);
        }
    }

    fn is_descendant_of(&self, node: NodeId, ancestor: NodeId) -> bool {
        let mut current = Some(node);
        while let Some(candidate) = current {
            if candidate == ancestor {
                return true;
            }
            current = self.nodes.get(&candidate).and_then(|node| node.parent);
        }
        false
    }

    fn record_focus_change(&mut self, change: FocusChange) {
        if !change.changed() {
            return;
        }
        for node in [change.previous, change.current].into_iter().flatten() {
            if let Some(node) = self.nodes.get_mut(&node) {
                node.invalidation.request(Invalidation::Paint);
            }
        }
        let combined = match self.pending_focus_change {
            Some(previous) => FocusChange {
                previous: previous.previous,
                current: change.current,
            },
            None => change,
        };
        self.pending_focus_change = combined.changed().then_some(combined);
        self.pending.request(Invalidation::Paint);
        self.bump_revision();
    }

    fn nearest_focusable(&self, node: NodeId) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(candidate) = current {
            let retained = self.nodes.get(&candidate)?;
            if retained.focusable {
                return Some(candidate);
            }
            current = retained.parent;
        }
        None
    }

    fn view_matches_committed<Message>(&self, element: &Element<'_, Message>) -> bool {
        self.root
            .is_some_and(|root| self.element_matches_node(root, element))
    }

    fn element_matches_node<Message>(&self, node: NodeId, element: &Element<'_, Message>) -> bool {
        let Some(retained) = self.nodes.get(&node) else {
            return false;
        };
        retained.kind == element.kind()
            && retained.key.as_ref() == element.explicit_key()
            && retained.children.len() == element.children().len()
            && retained
                .children
                .iter()
                .zip(element.children())
                .all(|(child, element)| self.element_matches_node(*child, element))
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn collect_elements<'view, 'content, Message>(
        &self,
        retained: NodeId,
        element: &'view Element<'content, Message>,
        output: &mut HashMap<NodeId, &'view Element<'content, Message>>,
    ) {
        output.insert(retained, element);
        let Some(node) = self.nodes.get(&retained) else {
            return;
        };
        for (child, element) in node.children.iter().zip(element.children()) {
            self.collect_elements(*child, element, output);
        }
    }

    fn invoke_route<Message>(
        &self,
        target: NodeId,
        event: &UiEvent,
        elements: &HashMap<NodeId, &Element<'_, Message>>,
    ) -> DispatchState<Message> {
        let mut route = Vec::new();
        let mut current = Some(target);
        while let Some(node) = current {
            route.push(node);
            current = self.nodes.get(&node).and_then(|node| node.parent);
        }
        route.reverse();

        let mut state = DispatchState::new();
        for node in route.iter().copied() {
            invoke_handlers(node, EventPhase::Capture, event, elements, &mut state);
            if state.propagation_stopped {
                return state;
            }
        }
        invoke_handlers(target, EventPhase::Target, event, elements, &mut state);
        if state.propagation_stopped {
            return state;
        }
        for node in route.iter().copied().rev() {
            invoke_handlers(node, EventPhase::Bubble, event, elements, &mut state);
            if state.propagation_stopped {
                break;
            }
        }
        state
    }

    fn invoke_target<Message>(
        &self,
        target: NodeId,
        event: &UiEvent,
        elements: &HashMap<NodeId, &Element<'_, Message>>,
    ) -> DispatchState<Message> {
        let mut state = DispatchState::new();
        invoke_handlers(target, EventPhase::Target, event, elements, &mut state);
        state
    }

    fn apply_requests(&mut self, requests: Vec<EventRequest>, allow_focus: bool) {
        for request in requests {
            match request {
                EventRequest::Focus(node) if allow_focus => {
                    let _ = self.focus_node(node);
                }
                EventRequest::CapturePointer(node) if self.nodes.contains_key(&node) => {
                    self.captured_pointer = Some(node);
                }
                EventRequest::ReleasePointer => self.captured_pointer = None,
                EventRequest::Invalidate(node, invalidation) => {
                    let _ = self.invalidate(node, invalidation);
                }
                EventRequest::Focus(_) | EventRequest::CapturePointer(_) => {}
            }
        }
    }

    fn merge_transition<Message>(
        &mut self,
        state: &mut DispatchState<Message>,
        mut transition: DispatchState<Message>,
    ) {
        self.apply_requests(std::mem::take(&mut transition.requests), false);
        state.messages.append(&mut transition.messages);
        state.handled |= transition.handled;
        state.default_prevented |= transition.default_prevented;
        state.propagation_stopped |= transition.propagation_stopped;
    }
}

fn invoke_handlers<Message>(
    node: NodeId,
    phase: EventPhase,
    event: &UiEvent,
    elements: &HashMap<NodeId, &Element<'_, Message>>,
    state: &mut DispatchState<Message>,
) {
    let Some(element) = elements.get(&node) else {
        return;
    };
    for handler in element.handlers(phase) {
        let mut context = EventContext::new(node, phase, state);
        handler(event, &mut context);
        if state.propagation_stopped {
            break;
        }
    }
}

fn validate_keys<Message>(element: &Element<'_, Message>) -> Result<(), ReconcileError> {
    let mut keys = std::collections::HashSet::with_capacity(element.children().len());
    for child in element.children() {
        if let Some(key) = child.explicit_key() {
            if !keys.insert(key) {
                return Err(ReconcileError::DuplicateSiblingKey(key.clone()));
            }
        }
        validate_keys(child)?;
    }
    Ok(())
}

fn content_fingerprint<Message>(element: &Element<'_, Message>) -> u64 {
    element
        .text_content()
        .unwrap_or_default()
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn saturating_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn inherit_style(inherited: Style, local: Style) -> Style {
    Style {
        foreground: local.foreground.or(inherited.foreground),
        background: local.background.or(inherited.background),
        underline_color: local.underline_color.or(inherited.underline_color),
        modifiers: inherited.modifiers | local.modifiers,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use arborui_core::{Color, CursorShape, Modifier, Size, Style};
    use arborui_layout::{Dimension, FlexDirection, LayoutStyle, Position};
    use arborui_render::PatchCellContent;
    use arborui_text::WidthPolicy;

    use super::*;
    use crate::{Key, PointerEvent, WidgetKind};

    fn keyed_text(key: u64, text: &str) -> Element<'_, ()> {
        Element::text(text).key(key)
    }

    fn prepare_and_commit<Message>(
        tree: &mut UiTree,
        view: &Element<'_, Message>,
        size: Size,
        renderer: &mut Renderer,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let prepared = tree.prepare(view, size, renderer)?;
        tree.commit(prepared, renderer)?;
        Ok(())
    }

    fn compare_incremental_and_reference<Message>(
        incremental_tree: &mut UiTree,
        reference_tree: &mut UiTree,
        view: &Element<'_, Message>,
        size: Size,
        incremental_renderer: &mut Renderer,
        reference_renderer: &mut Renderer,
        expect_layout: Option<bool>,
    ) -> Result<(), UiError> {
        let (incremental, timings) =
            incremental_tree.prepare_timed(view, size, incremental_renderer)?;
        let reference = reference_tree.prepare_full(view, size, reference_renderer)?;

        if let Some(expect_layout) = expect_layout {
            assert_eq!(timings.layout == Duration::ZERO, !expect_layout);
        }
        assert_eq!(incremental.patch(), reference.patch());
        assert_eq!(incremental.buffer(), reference.buffer());
        assert_eq!(incremental.hit_map(), reference.hit_map());
        assert_eq!(incremental.tree.root, reference.tree.root);
        assert_eq!(incremental.tree.nodes.len(), reference.tree.nodes.len());
        for (id, incremental_node) in &incremental.tree.nodes {
            let reference_node = reference.tree.nodes.get(id).expect("matching node exists");
            assert_eq!(incremental_node.key, reference_node.key);
            assert_eq!(incremental_node.kind, reference_node.kind);
            assert_eq!(incremental_node.children, reference_node.children);
            assert_eq!(incremental_node.layout, reference_node.layout);
            assert_eq!(incremental_node.content, reference_node.content);
        }

        incremental_tree
            .commit(incremental, incremental_renderer)
            .expect("incremental frame commits");
        reference_tree
            .commit(reference, reference_renderer)
            .expect("reference frame commits");
        assert_eq!(incremental_renderer.current(), reference_renderer.current());
        assert_eq!(incremental_renderer.hit_map(), reference_renderer.hit_map());
        Ok(())
    }

    fn pointer_message(
        message: &'static str,
    ) -> impl Fn(&UiEvent, &mut EventContext<'_, &'static str>) {
        move |event, context| {
            if matches!(event, UiEvent::Pointer(_)) {
                context.emit(message);
            }
        }
    }

    #[test]
    fn keyed_children_keep_identity_when_reordered() -> Result<(), ReconcileError> {
        let mut tree = UiTree::new();
        let first = Element::container([keyed_text(1, "one"), keyed_text(2, "two")]);
        tree.reconcile(&first)?;
        let root = tree.root().expect("root exists");
        let original = tree.node(root).expect("root exists").children().to_vec();

        let second = Element::container([keyed_text(2, "two"), keyed_text(1, "one")]);
        let report = tree.reconcile(&second)?;
        let reordered = tree.node(root).expect("root exists").children();

        assert_eq!(reordered, &[original[1], original[0]]);
        assert_eq!(report.created, 0);
        assert_eq!(report.removed, 0);
        assert_eq!(report.invalidation, Invalidation::Recompose);
        Ok(())
    }

    #[test]
    fn duplicate_keys_fail_before_mutating_the_tree() {
        let mut tree = UiTree::new();
        let duplicate = Element::container([keyed_text(1, "one"), keyed_text(1, "again")]);

        assert!(matches!(
            tree.reconcile(&duplicate),
            Err(ReconcileError::DuplicateSiblingKey(Key::Integer(1)))
        ));
        assert!(tree.is_empty());
    }

    #[test]
    fn incompatible_kind_replaces_and_removes_subtree() -> Result<(), ReconcileError> {
        let mut tree = UiTree::new();
        let first =
            Element::<()>::container([Element::container([Element::text("child")]).key(1_u64)]);
        tree.reconcile(&first)?;
        assert_eq!(tree.len(), 3);

        let second = Element::<()>::container([Element::text("replacement").key(1_u64)]);
        let report = tree.reconcile(&second)?;

        assert_eq!(tree.len(), 2);
        assert_eq!(report.created, 1);
        assert_eq!(report.removed, 2);
        Ok(())
    }

    #[test]
    fn borrowed_view_is_laid_out_and_painted_synchronously() -> Result<(), UiError> {
        let mut label = String::from("hello");
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(8, 2), WidthPolicy::Unicode);
        let prepared = {
            let view = Element::<()>::container([
                Element::text(&label).style(Style::new().foreground(Color::BrightGreen))
            ])
            .layout(LayoutStyle::new().direction(FlexDirection::Column));
            tree.prepare(&view, Size::new(8, 2), &mut renderer)?
        };
        let painted = prepared.patch().runs.iter().any(|run| {
            run.cells.iter().any(|cell| {
                matches!(&cell.content, PatchCellContent::Grapheme { text, .. } if text.as_ref() == "h")
            })
        });
        assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));
        label.push('!');

        assert_eq!(label, "hello!");
        assert!(painted);
        let root = tree.root().expect("root exists");
        assert_eq!(
            tree.node(root).expect("root exists").layout().size(),
            Size::new(8, 2)
        );
        assert_eq!(tree.pending_invalidation(), Invalidation::None);
        Ok(())
    }

    #[test]
    fn timed_preparation_matches_untimed_state_and_commit() -> Result<(), UiError> {
        let size = Size::new(5, 1);
        let view = Element::<()>::text("hello").interactive(true);
        let mut untimed_tree = UiTree::new();
        let mut timed_tree = UiTree::new();
        let mut untimed_renderer = Renderer::new(size, WidthPolicy::Unicode);
        let mut timed_renderer = Renderer::new(size, WidthPolicy::Unicode);

        let untimed = untimed_tree.prepare(&view, size, &mut untimed_renderer)?;
        let (timed, _timings) = timed_tree.prepare_timed(&view, size, &mut timed_renderer)?;
        assert_eq!(timed.patch(), untimed.patch());
        assert_eq!(timed.buffer(), untimed.buffer());
        assert_eq!(timed.hit_map(), untimed.hit_map());
        assert!(untimed_tree.is_empty());
        assert!(timed_tree.is_empty());

        assert_eq!(untimed_tree.commit(untimed, &mut untimed_renderer), Ok(()));
        assert_eq!(timed_tree.commit(timed, &mut timed_renderer), Ok(()));
        assert_eq!(timed_tree.root(), untimed_tree.root());
        assert_eq!(timed_tree.len(), untimed_tree.len());
        assert_eq!(timed_tree.pending_invalidation(), Invalidation::None);
        assert_eq!(timed_renderer.current(), untimed_renderer.current());
        assert_eq!(timed_renderer.hit_map(), untimed_renderer.hit_map());

        let root = timed_tree.root().expect("committed tree has a root");
        assert_eq!(
            timed_tree
                .node(root)
                .expect("committed root exists")
                .layout(),
            untimed_tree
                .node(root)
                .expect("committed root exists")
                .layout()
        );
        Ok(())
    }

    #[test]
    fn unchanged_preparation_skips_paint_callbacks() -> Result<(), UiError> {
        let size = Size::new(5, 1);
        let paint_calls = Rc::new(Cell::new(0));
        let view = Element::<()>::container([]).paint(0, {
            let paint_calls = Rc::clone(&paint_calls);
            move |_, _| {
                paint_calls.set(paint_calls.get() + 1);
                Ok(())
            }
        });
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);

        prepare_and_commit(&mut tree, &view, size, &mut renderer).expect("initial frame commits");
        assert_eq!(paint_calls.get(), 1);

        let unchanged = tree.prepare(&view, size, &mut renderer)?;
        assert_eq!(paint_calls.get(), 1);
        assert!(unchanged.patch().runs.is_empty());
        assert_eq!(tree.commit(unchanged, &mut renderer), Ok(()));

        renderer.invalidate();
        let repaint = tree.prepare(&view, size, &mut renderer)?;
        assert_eq!(paint_calls.get(), 2);
        assert!(repaint.patch().full_repaint);
        Ok(())
    }

    #[test]
    fn fragmented_row_damage_counts_regions_and_cells() {
        let damage = RowDamage::new(vec![true, false, true, false, true, false, true, false], 8);

        assert_eq!(damage.regions, 4);
        assert_eq!(damage.cells, 32);
        assert!(damage.intersects(Rect::new(0, 4, 8, 1)));
        assert!(!damage.intersects(Rect::new(0, 5, 8, 1)));
        assert!(damage.intersects(Rect::new(0, 3, 8, 2)));
    }

    #[test]
    fn paint_only_preparation_skips_clean_row_callbacks() -> Result<(), UiError> {
        fn counted_row(
            key: u64,
            text: &'static str,
            style: Style,
            calls: &Rc<Cell<usize>>,
        ) -> Element<'static, ()> {
            let calls = Rc::clone(calls);
            Element::text(text)
                .key(key)
                .style(style)
                .paint(0, move |_, _| {
                    calls.set(calls.get() + 1);
                    Ok(())
                })
        }

        let size = Size::new(8, 4);
        let calls = std::array::from_fn::<_, 4, _>(|_| Rc::new(Cell::new(0)));
        let root_calls = Rc::new(Cell::new(0));
        let count_root = || {
            let root_calls = Rc::clone(&root_calls);
            move |_: Size, _: &mut Canvas<'_>| {
                root_calls.set(root_calls.get() + 1);
                Ok(())
            }
        };
        let initial = Element::container([
            counted_row(0, "zero", Style::new(), &calls[0]),
            counted_row(1, "one", Style::new(), &calls[1]),
            counted_row(2, "two", Style::new(), &calls[2]),
            counted_row(3, "three", Style::new(), &calls[3]),
        ])
        .layout(LayoutStyle::new().direction(FlexDirection::Column))
        .paint(0, count_root());
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &initial, size, &mut renderer)
            .expect("initial frame commits");
        assert!(calls.iter().all(|calls| calls.get() == 1));
        assert_eq!(root_calls.get(), 1);

        let changed = Element::container([
            counted_row(0, "zero", Style::new(), &calls[0]),
            counted_row(1, "one", Style::new().foreground(Color::Blue), &calls[1]),
            counted_row(2, "two", Style::new(), &calls[2]),
            counted_row(3, "three", Style::new(), &calls[3]),
        ])
        .layout(LayoutStyle::new().direction(FlexDirection::Column))
        .paint(0, count_root());
        let prepared = tree.prepare(&changed, size, &mut renderer)?;

        assert_eq!(calls.each_ref().map(|calls| calls.get()), [1, 2, 1, 1]);
        assert_eq!(root_calls.get(), 2);
        assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));

        let separated = Element::container([
            counted_row(0, "zero", Style::new(), &calls[0]),
            counted_row(
                1,
                "one",
                Style::new().foreground(Color::BrightGreen),
                &calls[1],
            ),
            counted_row(2, "two", Style::new(), &calls[2]),
            counted_row(3, "three", Style::new().foreground(Color::Blue), &calls[3]),
        ])
        .layout(LayoutStyle::new().direction(FlexDirection::Column))
        .paint(0, count_root());
        let (prepared, timings) = tree.prepare_timed(&separated, size, &mut renderer)?;

        assert_eq!(calls.each_ref().map(|calls| calls.get()), [1, 3, 1, 2]);
        assert_eq!(root_calls.get(), 3);
        assert_eq!(timings.repaint_regions, 2);
        assert_eq!(timings.repaint_cells, 16);
        assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));
        Ok(())
    }

    #[test]
    fn incremental_preparation_matches_full_reference_across_transitions() -> Result<(), UiError> {
        let size = Size::new(12, 3);
        let mut incremental_tree = UiTree::new();
        let mut reference_tree = UiTree::new();
        let mut incremental_renderer = Renderer::new(size, WidthPolicy::Unicode);
        let mut reference_renderer = Renderer::new(size, WidthPolicy::Unicode);

        let initial = Element::<()>::container([
            Element::text("label").key(1_u64).interactive(true),
            Element::text("界").key(2_u64),
        ]);
        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &initial,
            size,
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(true),
        )?;

        let paint_only = Element::<()>::container([
            Element::text("label")
                .key(1_u64)
                .interactive(true)
                .style(Style::new().foreground(Color::Blue)),
            Element::text("界").key(2_u64),
        ]);
        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &paint_only,
            size,
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(false),
        )?;
        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &paint_only,
            size,
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(false),
        )?;

        let text_changed = Element::<()>::container([
            Element::text("long label")
                .key(1_u64)
                .interactive(true)
                .style(Style::new().foreground(Color::Blue)),
            Element::text("界").key(2_u64),
        ]);
        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &text_changed,
            size,
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(true),
        )?;

        let translated = Element::<()>::container([
            Element::text("long label")
                .key(1_u64)
                .interactive(true)
                .style(Style::new().foreground(Color::Blue)),
            Element::text("界").key(2_u64),
        ])
        .child_offset(Point::new(1, 0));
        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &translated,
            size,
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(true),
        )?;

        let reordered = Element::<()>::container([
            Element::text("界").key(2_u64),
            Element::text("long label")
                .key(1_u64)
                .interactive(true)
                .style(Style::new().foreground(Color::Blue)),
        ])
        .child_offset(Point::new(1, 0));
        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &reordered,
            size,
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(true),
        )?;
        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &reordered,
            Size::new(8, 2),
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(true),
        )?;

        let separated_initial = Element::<()>::container([
            Element::text("top").key(10_u64).interactive(true),
            Element::text("clean 界").key(11_u64).interactive(true),
            Element::text("clean two").key(12_u64).interactive(true),
            Element::text("bottom").key(13_u64).interactive(true),
        ])
        .layout(LayoutStyle::new().direction(FlexDirection::Column));
        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &separated_initial,
            Size::new(12, 4),
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(true),
        )?;

        let separated_changed = Element::<()>::container([
            Element::text("top")
                .key(10_u64)
                .interactive(true)
                .style(Style::new().foreground(Color::Blue)),
            Element::text("clean 界").key(11_u64).interactive(true),
            Element::text("clean two").key(12_u64).interactive(true),
            Element::text("bottom")
                .key(13_u64)
                .interactive(true)
                .style(Style::new().foreground(Color::BrightGreen)),
        ])
        .layout(LayoutStyle::new().direction(FlexDirection::Column));
        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &separated_changed,
            Size::new(12, 4),
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(false),
        )?;
        Ok(())
    }

    #[test]
    fn paint_only_ancestor_style_handles_clipped_absolute_descendant() -> Result<(), UiError> {
        let size = Size::new(4, 3);
        let view = |color| {
            Element::<()>::container([Element::container([Element::text("A\nB\nC").layout(
                LayoutStyle::new()
                    .size(Dimension::cells(1), Dimension::cells(3))
                    .position(Position::Absolute),
            )])
            .key(1_u64)
            .layout(LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(1)))
            .style(Style::new().foreground(color))])
        };
        let mut incremental_tree = UiTree::new();
        let mut reference_tree = UiTree::new();
        let mut incremental_renderer = Renderer::new(size, WidthPolicy::Unicode);
        let mut reference_renderer = Renderer::new(size, WidthPolicy::Unicode);

        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &view(Color::Red),
            size,
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(true),
        )?;

        let root = incremental_tree.root().expect("root exists");
        let ancestor = incremental_tree.node(root).expect("root exists").children()[0];
        let descendant = incremental_tree
            .node(ancestor)
            .expect("ancestor exists")
            .children()[0];
        assert!(
            incremental_tree
                .node(descendant)
                .expect("descendant exists")
                .layout()
                .bottom()
                > incremental_tree
                    .node(ancestor)
                    .expect("ancestor exists")
                    .layout()
                    .bottom()
        );

        compare_incremental_and_reference(
            &mut incremental_tree,
            &mut reference_tree,
            &view(Color::Blue),
            size,
            &mut incremental_renderer,
            &mut reference_renderer,
            Some(false),
        )?;

        assert_eq!(
            incremental_renderer
                .current()
                .get(Point::ORIGIN)
                .expect("origin is visible")
                .style
                .foreground,
            Some(Color::Blue)
        );
        assert_eq!(
            incremental_renderer
                .current()
                .get(Point::new(0, 1))
                .expect("second row is visible")
                .style,
            Style::default()
        );
        Ok(())
    }

    #[test]
    fn generated_incremental_frames_match_full_reference() -> Result<(), UiError> {
        const LABELS: [&str; 4] = ["a", "wide label", "界", "e\u{301}"];
        let initial_size = Size::new(12, 3);
        let mut incremental_tree = UiTree::new();
        let mut reference_tree = UiTree::new();
        let mut incremental_renderer = Renderer::new(initial_size, WidthPolicy::Unicode);
        let mut reference_renderer = Renderer::new(initial_size, WidthPolicy::Unicode);
        let mut state = 0x9e37_79b9_u32;

        for _ in 0..64 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let first_label = LABELS[(state as usize) & 3];
            let second_label = LABELS[((state >> 2) as usize) & 3];
            let first = Element::text(first_label)
                .key(1_u64)
                .interactive(true)
                .style(Style::new().foreground(if state & 0x10 == 0 {
                    Color::Blue
                } else {
                    Color::BrightGreen
                }));
            let second = Element::text(second_label).key(2_u64);
            let children = if state & 0x20 == 0 {
                [first, second]
            } else {
                [second, first]
            };
            let view = Element::<()>::container(children).child_offset(Point::new(
                ((state >> 6) & 1) as i32,
                ((state >> 7) & 1) as i32,
            ));
            let size = Size::new(
                8 + ((state >> 8) & 7) as u16,
                2 + ((state >> 11) & 1) as u16,
            );
            compare_incremental_and_reference(
                &mut incremental_tree,
                &mut reference_tree,
                &view,
                size,
                &mut incremental_renderer,
                &mut reference_renderer,
                None,
            )?;
        }
        Ok(())
    }

    #[test]
    fn text_changes_request_layout_and_style_changes_request_paint() -> Result<(), ReconcileError> {
        let mut tree = UiTree::new();
        tree.reconcile(&Element::<()>::text("a"))?;
        tree.pending = Invalidation::None;
        for node in tree.nodes.values_mut() {
            node.invalidation = Invalidation::None;
        }

        let report = tree.reconcile(&Element::<()>::text("longer"))?;
        assert_eq!(report.invalidation, Invalidation::Layout);
        tree.pending = Invalidation::None;
        let report = tree.reconcile(
            &Element::<()>::text("longer").style(Style::new().foreground(Color::Blue)),
        )?;
        assert_eq!(report.invalidation, Invalidation::Paint);

        tree.pending = Invalidation::None;
        for node in tree.nodes.values_mut() {
            node.invalidation = Invalidation::None;
        }
        let report = tree.reconcile(
            &Element::<()>::text("longer")
                .style(Style::new().foreground(Color::Blue))
                .focus_style(Style::new().add_modifiers(Modifier::REVERSED)),
        )?;
        assert_eq!(report.invalidation, Invalidation::Paint);
        Ok(())
    }

    #[test]
    fn percentage_layout_flows_through_ui_tree() -> Result<(), UiError> {
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(10, 1), WidthPolicy::Unicode);
        let view = Element::<()>::container([Element::text("x")
            .layout(LayoutStyle::new().size(Dimension::percent(50), Dimension::cells(1)))]);

        let prepared = tree.prepare(&view, Size::new(10, 1), &mut renderer)?;
        assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));
        let root = tree.root().expect("root exists");
        let child = tree.node(root).expect("root exists").children()[0];
        assert_eq!(tree.node(child).expect("child exists").layout().width, 5);
        Ok(())
    }

    #[test]
    fn discarded_ui_frame_preserves_committed_identity() -> Result<(), Box<dyn std::error::Error>> {
        let initial = Element::<()>::container([Element::text("old").key(1_u64).interactive(true)]);
        let changed = Element::<()>::container([Element::text("new").key(2_u64).interactive(true)]);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(3, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &initial, Size::new(3, 1), &mut renderer)?;
        let root = tree.root().expect("root exists");
        let committed_child = tree.node(root).expect("root exists").children()[0];

        let prepared = tree.prepare(&changed, Size::new(3, 1), &mut renderer)?;
        tree.discard(prepared, &mut renderer);

        assert_eq!(
            tree.node(root).expect("root exists").children(),
            &[committed_child]
        );
        assert_eq!(
            tree.dispatch(&changed, &UiEvent::Tick, &renderer),
            Err(ReconcileError::ViewDoesNotMatchCommittedTree)
        );
        Ok(())
    }

    #[test]
    fn rejects_commit_after_interaction_state_advances() -> Result<(), Box<dyn std::error::Error>> {
        let view = Element::<()>::text("x").interactive(true);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view, Size::new(1, 1), &mut renderer)?;
        let prepared = tree.prepare(&view, Size::new(1, 1), &mut renderer)?;
        let root = tree.root().expect("root exists");
        assert!(tree.invalidate(root, Invalidation::Paint));

        assert_eq!(
            tree.commit(prepared, &mut renderer),
            Err(UiCommitError::StaleTree)
        );
        Ok(())
    }

    #[test]
    fn rejects_commit_of_frame_prepared_by_another_ui_tree()
    -> Result<(), Box<dyn std::error::Error>> {
        let target_view = Element::<()>::container([Element::text("target")]).key(1_u64);
        let foreign_view = Element::<()>::text("foreign").key(2_u64);
        let mut target = UiTree::new();
        let mut foreign = UiTree::new();
        target.reconcile(&target_view)?;
        foreign.reconcile(&foreign_view)?;
        assert_eq!(target.revision, foreign.revision);

        let mut renderer = Renderer::new(Size::new(7, 1), WidthPolicy::Unicode);
        let renderer_state = renderer.state_id();
        let prepared = foreign.prepare(&foreign_view, Size::new(7, 1), &mut renderer)?;
        let target_root = target.root().expect("target root exists");

        assert_eq!(
            target.commit(prepared, &mut renderer),
            Err(UiCommitError::ForeignTree)
        );
        assert_eq!(renderer.state_id(), renderer_state);
        assert_eq!(target.len(), 2);
        assert_eq!(
            target.node(target_root).expect("target root remains").key(),
            Some(&Key::Integer(1))
        );
        assert_eq!(
            target
                .node(target_root)
                .expect("target root remains")
                .kind(),
            WidgetKind::Container
        );
        Ok(())
    }

    #[test]
    fn cloned_ui_trees_have_independent_frame_ownership() -> Result<(), UiError> {
        let source = UiTree::new();
        let mut cloned = source.clone();
        let view = Element::<()>::text("frame");
        let mut renderer = Renderer::new(Size::new(5, 1), WidthPolicy::Unicode);
        let renderer_state = renderer.state_id();
        let prepared = source.prepare(&view, Size::new(5, 1), &mut renderer)?;

        assert_eq!(
            cloned.commit(prepared, &mut renderer),
            Err(UiCommitError::ForeignTree)
        );
        assert_eq!(renderer.state_id(), renderer_state);
        assert!(cloned.is_empty());
        Ok(())
    }

    #[test]
    fn noninteractive_child_inherits_interactive_parent_hit()
    -> Result<(), Box<dyn std::error::Error>> {
        let view = Element::<()>::container([
            Element::container([Element::text("label")]).interactive(true)
        ]);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(5, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view, Size::new(5, 1), &mut renderer)?;
        let root = tree.root().expect("root exists");
        let interactive = tree.node(root).expect("root exists").children()[0];

        assert_eq!(
            tree.hit_test(renderer.hit_map(), Point::ORIGIN),
            Some(interactive)
        );
        Ok(())
    }

    #[test]
    fn dispatches_capture_target_and_bubble_in_order() -> Result<(), Box<dyn std::error::Error>> {
        let target = Element::text("x")
            .on_event(EventPhase::Capture, pointer_message("target capture"))
            .on_event(EventPhase::Target, pointer_message("target"))
            .on_event(EventPhase::Bubble, pointer_message("target bubble"));
        let parent = Element::container([target])
            .on_event(EventPhase::Capture, pointer_message("parent capture"))
            .on_event(EventPhase::Bubble, pointer_message("parent bubble"));
        let view = Element::<&'static str>::container([parent])
            .on_event(EventPhase::Capture, pointer_message("root capture"))
            .on_event(EventPhase::Bubble, pointer_message("root bubble"));
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(3, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view, Size::new(3, 1), &mut renderer)?;

        let outcome = tree.dispatch(
            &view,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Moved,
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &renderer,
        )?;

        assert_eq!(
            outcome.messages,
            [
                "root capture",
                "parent capture",
                "target capture",
                "target",
                "target bubble",
                "parent bubble",
                "root bubble"
            ]
        );
        Ok(())
    }

    #[test]
    fn propagation_flags_are_independent() -> Result<(), Box<dyn std::error::Error>> {
        let target = Element::text("x").on_event(EventPhase::Target, |event, context| {
            if matches!(event, UiEvent::Pointer(_)) {
                context.emit("target");
                context.mark_handled();
                context.prevent_default();
            }
        });
        let parent =
            Element::container([target]).on_event(EventPhase::Bubble, pointer_message("bubble"));
        let view = Element::<&'static str>::container([parent]);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(2, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view, Size::new(2, 1), &mut renderer)?;

        let outcome = tree.dispatch(
            &view,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Moved,
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &renderer,
        )?;

        assert_eq!(outcome.messages, ["target", "bubble"]);
        assert!(outcome.handled);
        assert!(outcome.default_prevented);
        assert!(!outcome.propagation_stopped);
        Ok(())
    }

    #[test]
    fn stop_propagation_skips_later_handlers() -> Result<(), Box<dyn std::error::Error>> {
        let target = Element::text("x").on_event(EventPhase::Target, pointer_message("target"));
        let parent = Element::container([target])
            .on_event(EventPhase::Capture, |event, context| {
                if matches!(event, UiEvent::Pointer(_)) {
                    context.emit("stop");
                    context.stop_propagation();
                }
            })
            .on_event(EventPhase::Capture, pointer_message("skipped"));
        let view = Element::<&'static str>::container([parent])
            .on_event(EventPhase::Capture, pointer_message("root"));
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(2, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view, Size::new(2, 1), &mut renderer)?;

        let outcome = tree.dispatch(
            &view,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Moved,
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &renderer,
        )?;

        assert_eq!(outcome.messages, ["root", "stop"]);
        assert!(outcome.propagation_stopped);
        Ok(())
    }

    #[test]
    fn pointer_capture_overrides_drag_and_release_hit_testing()
    -> Result<(), Box<dyn std::error::Error>> {
        let left = Element::text("L")
            .layout(LayoutStyle::new().size(Dimension::cells(3), Dimension::cells(1)))
            .on_event(EventPhase::Target, |event, context| match event {
                UiEvent::Pointer(PointerEvent {
                    kind: PointerEventKind::Down(_),
                    ..
                }) => context.capture_pointer(),
                UiEvent::Pointer(PointerEvent {
                    kind: PointerEventKind::Drag(_),
                    ..
                }) => context.emit("left drag"),
                UiEvent::Pointer(PointerEvent {
                    kind: PointerEventKind::Up(_),
                    ..
                }) => context.release_pointer(),
                _ => {}
            });
        let right = Element::text("R")
            .layout(LayoutStyle::new().size(Dimension::cells(3), Dimension::cells(1)))
            .on_event(EventPhase::Target, |event, context| {
                if matches!(
                    event,
                    UiEvent::Pointer(PointerEvent {
                        kind: PointerEventKind::Drag(_),
                        ..
                    })
                ) {
                    context.emit("right drag");
                }
            });
        let view = Element::<&'static str>::container([left, right]);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(6, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view, Size::new(6, 1), &mut renderer)?;
        let pointer = |kind, x| {
            UiEvent::Pointer(PointerEvent {
                kind,
                position: Point::new(x, 0),
                modifiers: crate::KeyModifiers::NONE,
            })
        };

        let _ = tree.dispatch(
            &view,
            &pointer(PointerEventKind::Down(crate::PointerButton::Primary), 0),
            &renderer,
        )?;
        assert!(tree.captured_pointer().is_some());
        let captured = tree.dispatch(
            &view,
            &pointer(PointerEventKind::Drag(crate::PointerButton::Primary), 4),
            &renderer,
        )?;
        assert_eq!(captured.messages, ["left drag"]);

        let _ = tree.dispatch(
            &view,
            &pointer(PointerEventKind::Up(crate::PointerButton::Primary), 4),
            &renderer,
        )?;
        assert_eq!(tree.captured_pointer(), None);
        let ordinary = tree.dispatch(
            &view,
            &pointer(PointerEventKind::Drag(crate::PointerButton::Primary), 4),
            &renderer,
        )?;
        assert_eq!(ordinary.messages, ["right drag"]);
        Ok(())
    }

    #[test]
    fn tab_traversal_wraps_forward_and_reverse() -> Result<(), Box<dyn std::error::Error>> {
        let view = Element::<()>::container([
            Element::text("a")
                .key(1_u64)
                .focusable(true)
                .layout(LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(1))),
            Element::text("b")
                .key(2_u64)
                .focusable(true)
                .layout(LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(1))),
        ]);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(2, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view, Size::new(2, 1), &mut renderer)?;
        let root = tree.root().expect("root exists");
        let children = tree.node(root).expect("root exists").children().to_vec();
        assert_eq!(tree.focused(), Some(children[0]));

        let tab = UiEvent::Key(crate::UiKeyEvent {
            key: UiKey::Tab,
            modifiers: crate::KeyModifiers::NONE,
            action: KeyAction::Press,
        });
        let _ = tree.dispatch(&view, &tab, &renderer)?;
        assert_eq!(tree.focused(), Some(children[1]));

        let reverse_tab = UiEvent::Key(crate::UiKeyEvent {
            key: UiKey::Tab,
            modifiers: crate::KeyModifiers::SHIFT,
            action: KeyAction::Press,
        });
        let _ = tree.dispatch(&view, &reverse_tab, &renderer)?;
        assert_eq!(tree.focused(), Some(children[0]));
        Ok(())
    }

    #[test]
    fn removing_focus_scope_restores_previous_focus() -> Result<(), Box<dyn std::error::Error>> {
        let base = Element::<()>::container([Element::text("base")
            .key(1_u64)
            .focusable(true)
            .layout(LayoutStyle::new().size(Dimension::cells(4), Dimension::cells(1)))]);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(8, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &base, Size::new(8, 1), &mut renderer)?;
        let base_focus = tree.focused();

        let overlay = Element::<()>::container([
            Element::text("base")
                .key(1_u64)
                .focusable(true)
                .layout(LayoutStyle::new().size(Dimension::cells(4), Dimension::cells(1))),
            Element::container([Element::text("dialog")
                .focusable(true)
                .layout(LayoutStyle::new().size(Dimension::cells(4), Dimension::cells(1)))])
            .key(2_u64)
            .focus_scope(true),
        ]);
        prepare_and_commit(&mut tree, &overlay, Size::new(8, 1), &mut renderer)?;
        assert_ne!(tree.focused(), base_focus);
        assert!(tree.active_focus_scope().is_some());

        prepare_and_commit(&mut tree, &base, Size::new(8, 1), &mut renderer)?;
        assert_eq!(tree.focused(), base_focus);
        Ok(())
    }

    #[test]
    fn opening_pointer_modal_releases_pointer_capture_from_lower_scope()
    -> Result<(), Box<dyn std::error::Error>> {
        let captured = Element::<()>::text("base")
            .key(1_u64)
            .layout(LayoutStyle::new().size(Dimension::cells(4), Dimension::cells(1)))
            .on_event(EventPhase::Target, |event, context| {
                if matches!(
                    event,
                    UiEvent::Pointer(PointerEvent {
                        kind: PointerEventKind::Down(_),
                        ..
                    })
                ) {
                    context.capture_pointer();
                }
            });
        let base = Element::container([captured]);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(8, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &base, Size::new(8, 1), &mut renderer)?;
        let _ = tree.dispatch(
            &base,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Down(crate::PointerButton::Primary),
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &renderer,
        )?;
        assert!(tree.captured_pointer().is_some());

        let overlay = Element::container([
            Element::<()>::text("base")
                .key(1_u64)
                .layout(LayoutStyle::new().size(Dimension::cells(4), Dimension::cells(1)))
                .interactive(true),
            Element::container([Element::text("dialog").focusable(true)])
                .key(2_u64)
                .focus_scope(true)
                .pointer_modal(true),
        ]);
        prepare_and_commit(&mut tree, &overlay, Size::new(8, 1), &mut renderer)?;

        assert_eq!(tree.captured_pointer(), None);
        Ok(())
    }

    #[test]
    fn removing_captured_target_suppresses_the_rest_of_its_pointer_sequence()
    -> Result<(), Box<dyn std::error::Error>> {
        let background = || {
            Element::text("background")
                .key("background")
                .layout(LayoutStyle::new().size(Dimension::cells(10), Dimension::cells(1)))
                .on_event(EventPhase::Target, |event, context| match event {
                    UiEvent::PointerEntered => context.emit("background-enter"),
                    UiEvent::Pointer(PointerEvent {
                        kind: PointerEventKind::Up(_),
                        ..
                    }) => context.emit("background-up"),
                    _ => {}
                })
        };
        let close = Element::text("close")
            .layout(LayoutStyle::new().size(Dimension::cells(5), Dimension::cells(1)))
            .on_event(EventPhase::Target, |event, context| {
                if matches!(
                    event,
                    UiEvent::Pointer(PointerEvent {
                        kind: PointerEventKind::Down(_),
                        ..
                    })
                ) {
                    context.capture_pointer();
                    context.emit("close");
                }
            });
        let modal = Element::container([close])
            .key("modal")
            .layout(
                LayoutStyle::new()
                    .size(Dimension::cells(10), Dimension::cells(1))
                    .position(arborui_layout::Position::Absolute),
            )
            .pointer_modal(true)
            .interactive(true);
        let with_modal = Element::container([background(), modal]);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(10, 1), WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &with_modal, Size::new(10, 1), &mut renderer)?;

        let down = tree.dispatch(
            &with_modal,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Down(crate::PointerButton::Primary),
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &renderer,
        )?;
        assert_eq!(down.messages, ["close"]);
        assert!(tree.captured_pointer().is_some());

        let without_modal = Element::container([background()]);
        prepare_and_commit(&mut tree, &without_modal, Size::new(10, 1), &mut renderer)?;
        let root = tree.root().expect("root exists");
        let background_node = tree.node(root).expect("root exists").children()[0];
        let drag = tree.dispatch(
            &without_modal,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Drag(crate::PointerButton::Primary),
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &renderer,
        )?;
        assert_eq!(drag.messages, ["background-enter"]);
        assert!(drag.handled);
        assert_eq!(drag.target, None);
        assert_eq!(tree.hovered(), Some(background_node));

        let secondary_down = tree.dispatch(
            &without_modal,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Down(crate::PointerButton::Secondary),
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &renderer,
        )?;
        assert!(secondary_down.messages.is_empty());

        let primary_up = tree.dispatch(
            &without_modal,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Up(crate::PointerButton::Primary),
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &renderer,
        )?;
        assert!(primary_up.messages.is_empty());
        assert!(primary_up.handled);
        assert_eq!(primary_up.target, None);

        let secondary_up = tree.dispatch(
            &without_modal,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Up(crate::PointerButton::Secondary),
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &renderer,
        )?;
        assert_eq!(secondary_up.messages, ["background-up"]);
        Ok(())
    }

    #[test]
    fn stationary_pointer_recomputes_hover_from_new_hit_map()
    -> Result<(), Box<dyn std::error::Error>> {
        let transition = |name| {
            Element::text(name)
                .key(name)
                .on_event(EventPhase::Target, move |event, context| match event {
                    UiEvent::PointerEntered => context.emit((name, "enter")),
                    UiEvent::PointerLeft => context.emit((name, "leave")),
                    _ => {}
                })
        };
        let view = Element::<(&str, &str)>::container([transition("left"), transition("right")]);
        let mut tree = UiTree::new();
        tree.reconcile(&view)?;
        let root = tree.root().expect("root exists");
        let children = tree.node(root).expect("root exists").children().to_vec();
        let mut first = HitMap::new(Size::new(1, 1));
        let _ = first.set(Point::ORIGIN, HitId::new(children[0].0));
        let mut second = HitMap::new(Size::new(1, 1));
        let _ = second.set(Point::ORIGIN, HitId::new(children[1].0));

        let moved = tree.dispatch_with_hit_map(
            &view,
            &UiEvent::Pointer(PointerEvent {
                kind: PointerEventKind::Moved,
                position: Point::ORIGIN,
                modifiers: crate::KeyModifiers::NONE,
            }),
            &first,
        )?;
        assert_eq!(moved.messages, [("left", "enter")]);
        assert!(
            tree.refresh_hover_with_hit_map(&view, &first)?
                .messages
                .is_empty()
        );
        assert_eq!(
            tree.refresh_hover_with_hit_map(&view, &second)?.messages,
            [("left", "leave"), ("right", "enter")]
        );
        Ok(())
    }

    #[test]
    fn focused_cursor_is_translated_and_clipped() -> Result<(), UiError> {
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(4, 1), WidthPolicy::Unicode);
        let visible = Element::<()>::container([Element::text("abc")
            .focusable(true)
            .cursor(
                CursorState::visible(Point::new(1, 0))
                    .with_shape(CursorShape::Bar)
                    .with_blinking(true),
            )
            .layout(LayoutStyle::new().size(Dimension::cells(3), Dimension::cells(1)))]);
        let prepared = tree.prepare(&visible, Size::new(4, 1), &mut renderer)?;
        assert_eq!(prepared.patch().cursor.position, Point::new(1, 0));
        assert_eq!(
            prepared.patch().cursor.visibility,
            CursorVisibility::Visible
        );
        assert_eq!(prepared.patch().cursor.shape, CursorShape::Bar);
        assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));

        let clipped = Element::<()>::container([Element::text("abc")
            .focusable(true)
            .cursor(CursorState::visible(Point::new(9, 0)))
            .layout(LayoutStyle::new().size(Dimension::cells(3), Dimension::cells(1)))]);
        let prepared = tree.prepare(&clipped, Size::new(4, 1), &mut renderer)?;
        assert_eq!(prepared.patch().cursor, CursorState::HIDDEN);
        Ok(())
    }
}
