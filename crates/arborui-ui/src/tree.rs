use std::{collections::HashMap, fmt};

use arborui_core::{CursorState, CursorVisibility, Point, Rect, Size};
use arborui_layout::{LayoutError, LayoutNodeId, LayoutTree};
use arborui_render::{
    Buffer, Canvas, CommitError, DrawError, FramePatch, HitId, HitMap, PreparedFrame, RenderError,
    Renderer, RendererStateId,
};
use arborui_text::measure;

use crate::{
    DispatchOutcome, Element, EventContext, EventPhase, FocusChange, FocusError, Invalidation, Key,
    KeyAction, NodeId, PointerEventKind, ReconcileError, ReconcileReport, RetainedNode, UiEvent,
    UiKey,
    event::{DispatchState, EventRequest},
    focus::FocusManager,
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
    /// Retained interaction state changed after frame preparation.
    StaleTree,
    /// Renderer state changed or the frame belongs to another renderer.
    Renderer(CommitError),
}

impl fmt::Display for UiCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleTree => formatter.write_str("UI state advanced after frame preparation"),
            Self::Renderer(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for UiCommitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StaleTree => None,
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
#[derive(Clone, Debug, Default)]
pub struct UiTree {
    nodes: HashMap<NodeId, RetainedNode>,
    root: Option<NodeId>,
    next_id: u64,
    pending: Invalidation,
    viewport: Option<Size>,
    focus: FocusManager,
    captured_pointer: Option<NodeId>,
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
    frame: PreparedFrame,
    tree: UiTree,
    base_revision: u64,
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

        let target = if let Some(pointer) = event.pointer() {
            self.last_pointer = Some(pointer.position);
            if matches!(
                pointer.kind,
                PointerEventKind::Drag(_) | PointerEventKind::Up(_)
            ) {
                self.captured_pointer
                    .filter(|node| self.nodes.contains_key(node))
                    .or_else(|| self.hit_test(hit_map, pointer.position))
                    .or(Some(root))
            } else {
                self.hit_test(hit_map, pointer.position).or(Some(root))
            }
        } else if matches!(
            event,
            UiEvent::Key(_) | UiEvent::Text(_) | UiEvent::Paste(_)
        ) {
            self.focused().or(Some(root))
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
            let next = self.hit_test(hit_map, pointer.position);
            let previous = self.hovered;
            if previous != next {
                self.hovered = next;
                if let Some(previous) = previous {
                    let transition = self.invoke_target(previous, &UiEvent::PointerLeft, &elements);
                    self.merge_transition(&mut state, transition);
                }
                if let Some(next) = next {
                    let transition = self.invoke_target(next, &UiEvent::PointerEntered, &elements);
                    self.merge_transition(&mut state, transition);
                }
            }
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

        let next = self
            .last_pointer
            .and_then(|position| self.hit_test(hit_map, position));
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
        report.invalidation = self.pending;
        self.renderer_state = None;
        self.bump_revision();
        Ok(report)
    }

    /// Reconciles, lays out, and paints a complete headless frame.
    pub fn prepare<Message>(
        &self,
        element: &Element<'_, Message>,
        viewport: Size,
        renderer: &mut Renderer,
    ) -> Result<PreparedUiFrame, UiError> {
        let mut staged = self.clone();
        let frame = staged.prepare_frame(element, viewport, renderer)?;
        Ok(PreparedUiFrame {
            frame,
            tree: staged,
            base_revision: self.revision,
        })
    }

    fn prepare_frame<Message>(
        &mut self,
        element: &Element<'_, Message>,
        viewport: Size,
        renderer: &mut Renderer,
    ) -> Result<PreparedFrame, UiError> {
        self.reconcile(element)?;
        if self.viewport != Some(viewport) {
            self.pending.request(Invalidation::Layout);
            self.viewport = Some(viewport);
        }

        let root = self.root.expect("reconciliation always creates a root");
        let mut layout_tree = LayoutTree::new();
        let mut mapping = Vec::with_capacity(self.nodes.len());
        let layout_root = self.build_layout(root, element, &mut layout_tree, &mut mapping)?;
        let by_layout = mapping
            .iter()
            .map(|(layout, _, element)| (*layout, *element))
            .collect::<HashMap<_, _>>();
        let width_policy = renderer.width_policy();
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

        let focus_change = self.focus.sync(&self.nodes, self.root, self.viewport);
        self.record_focus_change(focus_change);
        let focus_change = self.focus.repair(&self.nodes, self.root, self.viewport);
        self.record_focus_change(focus_change);
        let by_retained = mapping
            .iter()
            .map(|(_, retained, element)| (*retained, *element))
            .collect::<HashMap<_, _>>();
        let cursor = self.resolve_cursor(&by_retained, viewport, width_policy);

        let prepared = renderer.prepare(viewport, cursor, |canvas| {
            self.paint_node(
                root,
                element,
                canvas,
                Rect::from_origin_size(Point::ORIGIN, viewport),
                None,
            )
        })?;
        self.pending = Invalidation::None;
        for node in self.nodes.values_mut() {
            node.invalidation = Invalidation::None;
        }
        Ok(prepared)
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
                } else if retained.visual_style != element.visual_style() {
                    requested.request(Invalidation::Paint);
                }
                if retained.interactive != element.is_interactive()
                    || retained.focusable != element.is_focusable()
                    || retained.focus_scope != element.is_focus_scope()
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
                retained.content_fingerprint = fingerprint;
                retained.interactive = element.is_interactive();
                retained.focusable = element.is_focusable();
                retained.focus_scope = element.is_focus_scope();
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
        self.nodes
            .get_mut(&node_id)
            .expect("reconciled node exists")
            .children = new_children;
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
                content_fingerprint: content_fingerprint(element),
                invalidation: Invalidation::Recompose,
                interactive: element.is_interactive(),
                focusable: element.is_focusable(),
                focus_scope: element.is_focus_scope(),
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
        width_policy: arborui_text::WidthPolicy,
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
        inherited_clip: Rect,
        inherited_hit: Option<HitId>,
    ) -> Result<(), DrawError> {
        let node = self
            .nodes
            .get(&retained)
            .expect("element and retained tree have matching structure");
        let clip = inherited_clip
            .intersection(node.layout)
            .unwrap_or(Rect::ZERO);
        let hit = if element.is_interactive() {
            Some(HitId::new(retained.0))
        } else {
            inherited_hit
        };
        {
            let mut scoped = canvas.scoped(clip, node.layout.origin()).with_hit(hit);
            if element.fills_background() {
                scoped.fill(
                    Rect::new(0, 0, node.layout.width, node.layout.height),
                    element.visual_style(),
                )?;
            }
            if let Some(text) = element.text_content() {
                let text_origin = Point::new(
                    node.content.x.saturating_sub(node.layout.x),
                    node.content.y.saturating_sub(node.layout.y),
                );
                let mut text_canvas = scoped.scoped(node.content, node.layout.origin());
                text_canvas.draw_text(text_origin, text, element.visual_style(), None)?;
            }
            element.paint_content(node.layout.size(), &mut scoped)?;
        }
        let children_clip = clip.intersection(node.content).unwrap_or(Rect::ZERO);
        for (child, element) in node.children.iter().zip(element.children()) {
            self.paint_node(*child, element, canvas, children_clip, hit)?;
        }
        Ok(())
    }

    fn resolve_cursor<Message>(
        &self,
        elements: &HashMap<NodeId, &Element<'_, Message>>,
        viewport: Size,
        width_policy: arborui_text::WidthPolicy,
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
        }
        if self
            .hovered
            .is_some_and(|node| !self.nodes.contains_key(&node))
        {
            self.hovered = None;
        }
    }

    fn record_focus_change(&mut self, change: FocusChange) {
        if !change.changed() {
            return;
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

#[cfg(test)]
mod tests {
    use arborui_core::{Color, CursorShape, Size, Style};
    use arborui_layout::{Dimension, FlexDirection, LayoutStyle};
    use arborui_render::PatchCellContent;
    use arborui_text::WidthPolicy;

    use super::*;
    use crate::{Key, PointerEvent};

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
