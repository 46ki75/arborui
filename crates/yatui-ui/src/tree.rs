use std::{collections::HashMap, fmt};

use yatui_core::{CursorState, Point, Rect, Size};
use yatui_layout::{LayoutError, LayoutNodeId, LayoutTree};
use yatui_render::{Canvas, DrawError, PreparedFrame, RenderError, Renderer};
use yatui_text::measure;

use crate::{Element, Invalidation, NodeId, ReconcileError, ReconcileReport, RetainedNode};

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
#[derive(Debug, Default)]
pub struct UiTree {
    nodes: HashMap<NodeId, RetainedNode>,
    root: Option<NodeId>,
    next_id: u64,
    pending: Invalidation,
    viewport: Option<Size>,
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

    /// Requests work for one retained node and escalates the tree request.
    pub fn invalidate(&mut self, node: NodeId, requested: Invalidation) -> bool {
        let Some(node) = self.nodes.get_mut(&node) else {
            return false;
        };
        node.invalidation.request(requested);
        self.pending.request(requested);
        true
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
        report.invalidation = self.pending;
        Ok(report)
    }

    /// Reconciles, lays out, and paints a complete headless frame.
    pub fn prepare<Message>(
        &mut self,
        element: &Element<'_, Message>,
        viewport: Size,
        cursor: CursorState,
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
        for (layout_node, retained, _) in &mapping {
            let bounds = layout_tree.layout(*layout_node)?.bounds;
            if let Some(node) = self.nodes.get_mut(retained) {
                node.layout = bounds;
            }
        }

        let prepared = renderer.prepare(viewport, cursor, |canvas| {
            self.paint_node(
                root,
                element,
                canvas,
                Rect::from_origin_size(Point::ORIGIN, viewport),
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
                retained.layout_style = element.layout_style();
                retained.visual_style = element.visual_style();
                retained.content_fingerprint = fingerprint;
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
                layout_style: element.layout_style(),
                visual_style: element.visual_style(),
                content_fingerprint: content_fingerprint(element),
                invalidation: Invalidation::Recompose,
            },
        );
        id
    }

    fn remove_subtree(&mut self, node: NodeId, report: &mut ReconcileReport) {
        let Some(node) = self.nodes.remove(&node) else {
            return;
        };
        report.removed += 1;
        self.pending.request(Invalidation::Recompose);
        for child in node.children {
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

    fn paint_node<Message>(
        &self,
        retained: NodeId,
        element: &Element<'_, Message>,
        canvas: &mut Canvas<'_>,
        inherited_clip: Rect,
    ) -> Result<(), DrawError> {
        let node = self
            .nodes
            .get(&retained)
            .expect("element and retained tree have matching structure");
        let clip = inherited_clip
            .intersection(node.layout)
            .unwrap_or(Rect::ZERO);
        {
            let mut scoped = canvas.scoped(clip, node.layout.origin());
            scoped.fill(
                Rect::new(0, 0, node.layout.width, node.layout.height),
                element.visual_style(),
            )?;
            if let Some(text) = element.text_content() {
                scoped.draw_text(Point::ORIGIN, text, element.visual_style(), None)?;
            }
        }
        for (child, element) in node.children.iter().zip(element.children()) {
            self.paint_node(*child, element, canvas, clip)?;
        }
        Ok(())
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
    let Some(text) = element.text_content() else {
        return 0;
    };
    text.as_bytes()
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
    use yatui_core::{Color, Size, Style};
    use yatui_layout::{Dimension, FlexDirection, LayoutStyle};
    use yatui_render::PatchCellContent;
    use yatui_text::WidthPolicy;

    use super::*;
    use crate::Key;

    fn keyed_text(key: u64, text: &str) -> Element<'_, ()> {
        Element::text(text).key(key)
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
            tree.prepare(
                &view,
                Size::new(8, 2),
                CursorState::default(),
                &mut renderer,
            )?
        };
        label.push('!');

        assert_eq!(label, "hello!");
        assert!(prepared.patch().runs.iter().any(|run| {
            run.cells.iter().any(|cell| {
                matches!(&cell.content, PatchCellContent::Grapheme { text, .. } if text.as_ref() == "h")
            })
        }));
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

        let _ = tree.prepare(
            &view,
            Size::new(10, 1),
            CursorState::default(),
            &mut renderer,
        )?;
        let root = tree.root().expect("root exists");
        let child = tree.node(root).expect("root exists").children()[0];
        assert_eq!(tree.node(child).expect("child exists").layout().width, 5);
        Ok(())
    }
}
