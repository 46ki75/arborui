use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use arborui_core::{Insets, Rect, Size};

use crate::{Dimension, LayoutStyle, MeasureInput, engine};

/// Stable, library-owned identity for a node in one layout tree.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LayoutNodeId {
    tree: u64,
    index: usize,
    generation: u64,
}

impl LayoutNodeId {
    pub(crate) const fn index(self) -> usize {
        self.index
    }
}

static NEXT_TREE_ID: AtomicU64 = AtomicU64::new(1);

fn next_tree_id() -> u64 {
    let id = NEXT_TREE_ID.fetch_add(1, Ordering::Relaxed);
    assert_ne!(id, 0, "layout tree identity space exhausted");
    id
}

/// Integer-cell geometry computed for one layout node.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ComputedLayout {
    /// Border-box bounds in root coordinates.
    pub bounds: Rect,
    /// Content-box bounds in root coordinates.
    pub content: Rect,
    /// Resolved padding.
    pub padding: Insets,
    /// Resolved border thickness.
    pub border: Insets,
    /// Relative paint order assigned by the layout engine.
    pub order: u32,
}

/// Errors produced by layout tree operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    /// A node does not belong to this tree.
    UnknownNode(LayoutNodeId),
    /// The same child was supplied more than once.
    DuplicateChild(LayoutNodeId),
    /// Assigning the children would create a cycle.
    Cycle(LayoutNodeId),
    /// Geometry has not been computed for the node.
    NotComputed(LayoutNodeId),
    /// The private layout engine rejected an operation.
    Engine(String),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownNode(node) => write!(formatter, "unknown layout node {node:?}"),
            Self::DuplicateChild(node) => write!(formatter, "duplicate layout child {node:?}"),
            Self::Cycle(node) => write!(formatter, "layout child {node:?} would create a cycle"),
            Self::NotComputed(node) => {
                write!(formatter, "layout node {node:?} has not been computed")
            }
            Self::Engine(error) => write!(formatter, "layout engine error: {error}"),
        }
    }
}

impl std::error::Error for LayoutError {}

#[derive(Clone, Debug)]
struct Node {
    style: LayoutStyle,
    parent: Option<LayoutNodeId>,
    children: Vec<LayoutNodeId>,
}

#[derive(Clone, Debug)]
struct NodeSlot {
    generation: u64,
    node: Option<Node>,
}

#[derive(Clone, Debug, Default)]
struct NodeStore {
    slots: Vec<NodeSlot>,
    free: Vec<usize>,
    len: usize,
}

impl NodeStore {
    const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }

    fn insert(&mut self, tree: u64, node: Node) -> LayoutNodeId {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index];
            slot.node = Some(node);
            LayoutNodeId {
                tree,
                index,
                generation: slot.generation,
            }
        } else {
            let index = self.slots.len();
            self.slots.push(NodeSlot {
                generation: 0,
                node: Some(node),
            });
            LayoutNodeId {
                tree,
                index,
                generation: 0,
            }
        }
    }

    fn get(&self, node: LayoutNodeId) -> Option<&Node> {
        let slot = self.slots.get(node.index)?;
        (slot.generation == node.generation)
            .then_some(slot.node.as_ref())
            .flatten()
    }

    fn get_mut(&mut self, node: LayoutNodeId) -> Option<&mut Node> {
        let slot = self.slots.get_mut(node.index)?;
        (slot.generation == node.generation)
            .then_some(slot.node.as_mut())
            .flatten()
    }

    fn remove(&mut self, node: LayoutNodeId) -> Option<Node> {
        let slot = self.slots.get_mut(node.index)?;
        if slot.generation != node.generation {
            return None;
        }
        let removed = slot.node.take()?;
        self.len -= 1;
        if slot.generation < u64::MAX {
            slot.generation += 1;
            self.free.push(node.index);
        }
        Some(removed)
    }

    #[cfg(test)]
    const fn len(&self) -> usize {
        self.len
    }
}

/// Mutable tree of layout styles and computed integer geometry.
#[derive(Clone, Debug, Default)]
pub struct LayoutTree {
    tree_id: u64,
    nodes: NodeStore,
    layouts: Vec<Option<ComputedLayout>>,
}

impl LayoutTree {
    /// Creates an empty layout tree.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tree_id: 0,
            nodes: NodeStore::new(),
            layouts: Vec::new(),
        }
    }

    /// Adds an unattached node.
    pub fn add(&mut self, style: LayoutStyle) -> LayoutNodeId {
        if self.tree_id == 0 {
            self.tree_id = next_tree_id();
        }
        let id = self.nodes.insert(
            self.tree_id,
            Node {
                style,
                parent: None,
                children: Vec::new(),
            },
        );
        if id.index() == self.layouts.len() {
            self.layouts.push(None);
        } else {
            self.layouts[id.index()] = None;
        }
        id
    }

    /// Adds a node and assigns its children.
    pub fn add_with_children(
        &mut self,
        style: LayoutStyle,
        children: &[LayoutNodeId],
    ) -> Result<LayoutNodeId, LayoutError> {
        self.validate_children(None, children)?;
        let node = self.add(style);
        if let Err(error) = self.replace_children(node, children) {
            self.remove_subtree(node);
            return Err(error);
        }
        Ok(node)
    }

    /// Replaces a node's style.
    pub fn set_style(&mut self, node: LayoutNodeId, style: LayoutStyle) -> Result<(), LayoutError> {
        self.node_mut(node)?.style = style;
        self.layouts.fill(None);
        Ok(())
    }

    /// Replaces a node's children, reparenting them when necessary.
    pub fn set_children(
        &mut self,
        parent: LayoutNodeId,
        children: &[LayoutNodeId],
    ) -> Result<(), LayoutError> {
        self.node(parent)?;
        self.validate_children(Some(parent), children)?;
        self.replace_children(parent, children)
    }

    fn validate_children(
        &self,
        parent: Option<LayoutNodeId>,
        children: &[LayoutNodeId],
    ) -> Result<(), LayoutError> {
        let mut unique = std::collections::HashSet::with_capacity(children.len());
        for child in children {
            self.node(*child)?;
            if !unique.insert(*child) {
                return Err(LayoutError::DuplicateChild(*child));
            }
            let mut ancestor = parent;
            while let Some(node) = ancestor {
                if node == *child {
                    return Err(LayoutError::Cycle(*child));
                }
                ancestor = self.node(node)?.parent;
            }
        }
        Ok(())
    }

    fn replace_children(
        &mut self,
        parent: LayoutNodeId,
        children: &[LayoutNodeId],
    ) -> Result<(), LayoutError> {
        let old_children = std::mem::take(&mut self.node_mut(parent)?.children);
        for child in &old_children {
            if self.node(*child)?.parent == Some(parent) {
                self.node_mut(*child)?.parent = None;
            }
        }
        for child in children {
            if let Some(old_parent) = self.node(*child)?.parent {
                self.node_mut(old_parent)?
                    .children
                    .retain(|candidate| candidate != child);
            }
            self.node_mut(*child)?.parent = Some(parent);
        }
        self.node_mut(parent)?.children.extend_from_slice(children);
        for child in old_children {
            if self.node(child).is_ok_and(|node| node.parent.is_none()) {
                self.remove_subtree(child);
            }
        }
        self.layouts.fill(None);
        Ok(())
    }

    /// Returns a node's children.
    pub fn children(&self, node: LayoutNodeId) -> Result<&[LayoutNodeId], LayoutError> {
        Ok(&self.node(node)?.children)
    }

    /// Computes the root subtree for `viewport` using a caller-owned leaf measurer.
    ///
    /// An automatic root dimension fills the corresponding viewport dimension.
    pub fn compute<F>(
        &mut self,
        root: LayoutNodeId,
        viewport: Size,
        measure: F,
    ) -> Result<(), LayoutError>
    where
        F: FnMut(LayoutNodeId, MeasureInput) -> Size,
    {
        self.node(root)?;
        let mut nodes = self
            .nodes
            .slots
            .iter()
            .map(|slot| {
                slot.node.as_ref().map_or_else(
                    || (LayoutStyle::default(), Vec::new()),
                    |node| (node.style, node.children.clone()),
                )
            })
            .collect::<Vec<_>>();
        let root_style = &mut nodes[root.index()].0;
        if root_style.width == Dimension::Auto {
            root_style.width = Dimension::Cells(viewport.width);
        }
        if root_style.height == Dimension::Auto {
            root_style.height = Dimension::Cells(viewport.height);
        }
        self.layouts = engine::compute(&nodes, root, viewport, measure)?.layouts;
        Ok(())
    }

    /// Returns geometry from the most recent computation.
    pub fn layout(&self, node: LayoutNodeId) -> Result<ComputedLayout, LayoutError> {
        self.node(node)?;
        self.layouts[node.index()].ok_or(LayoutError::NotComputed(node))
    }

    /// Removes every node and computed layout.
    pub fn clear(&mut self) {
        self.tree_id = next_tree_id();
        self.nodes = NodeStore::new();
        self.layouts.clear();
    }

    fn node(&self, node: LayoutNodeId) -> Result<&Node, LayoutError> {
        if node.tree != self.tree_id {
            return Err(LayoutError::UnknownNode(node));
        }
        self.nodes.get(node).ok_or(LayoutError::UnknownNode(node))
    }

    fn node_mut(&mut self, node: LayoutNodeId) -> Result<&mut Node, LayoutError> {
        if node.tree != self.tree_id {
            return Err(LayoutError::UnknownNode(node));
        }
        self.nodes
            .get_mut(node)
            .ok_or(LayoutError::UnknownNode(node))
    }

    fn remove_subtree(&mut self, node: LayoutNodeId) {
        let Some(removed) = self.nodes.remove(node) else {
            return;
        };
        self.layouts[node.index()] = None;
        for child in removed.children {
            if self
                .node(child)
                .is_ok_and(|candidate| candidate.parent == Some(node))
            {
                self.remove_subtree(child);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Align, Dimension, FlexDirection};

    #[test]
    fn computes_percentages_rounding_and_constraints() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let child = tree.add(LayoutStyle {
            width: Dimension::percent(50),
            min_width: Dimension::cells(4),
            max_width: Dimension::cells(5),
            ..LayoutStyle::default()
        });
        let root = tree.add_with_children(
            LayoutStyle::new().size(Dimension::cells(9), Dimension::cells(2)),
            &[child],
        )?;

        tree.compute(root, Size::new(9, 2), |_, _| Size::ZERO)?;

        assert_eq!(tree.layout(child)?.bounds.width, 5);
        Ok(())
    }

    #[test]
    fn distributes_flex_growth_and_shrinkage() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let growing = tree.add(
            LayoutStyle::new()
                .size(Dimension::cells(2), Dimension::cells(1))
                .flex(1, 1),
        );
        let fixed = tree.add(
            LayoutStyle::new()
                .size(Dimension::cells(2), Dimension::cells(1))
                .flex(0, 1),
        );
        let root = tree.add_with_children(
            LayoutStyle::new().size(Dimension::cells(8), Dimension::cells(1)),
            &[growing, fixed],
        )?;
        tree.compute(root, Size::new(8, 1), |_, _| Size::ZERO)?;
        assert_eq!(tree.layout(growing)?.bounds.width, 6);

        tree.set_style(
            root,
            LayoutStyle::new().size(Dimension::cells(3), Dimension::cells(1)),
        )?;
        tree.compute(root, Size::new(3, 1), |_, _| Size::ZERO)?;
        assert_eq!(tree.layout(growing)?.bounds.width, 2);
        assert_eq!(tree.layout(fixed)?.bounds.width, 1);
        Ok(())
    }

    #[test]
    fn integrates_measurement_and_box_geometry() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let leaf = tree.add(LayoutStyle {
            padding: Insets::symmetric(1, 2),
            border: Insets::all(1),
            ..LayoutStyle::default()
        });
        let root = tree.add_with_children(
            LayoutStyle {
                align: Align::Start,
                ..LayoutStyle::default()
            },
            &[leaf],
        )?;

        tree.compute(root, Size::new(20, 10), |node, _| {
            assert_eq!(node, leaf);
            Size::new(5, 2)
        })?;

        let layout = tree.layout(leaf)?;
        assert_eq!(layout.bounds.size(), Size::new(11, 6));
        assert_eq!(layout.content.size(), Size::new(5, 2));
        Ok(())
    }

    #[test]
    fn recomputes_after_viewport_resize() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let child = tree.add(LayoutStyle::new().size(Dimension::percent(100), Dimension::cells(1)));
        let root = tree.add_with_children(
            LayoutStyle::new().direction(FlexDirection::Column),
            &[child],
        )?;

        tree.compute(root, Size::new(8, 2), |_, _| Size::ZERO)?;
        assert_eq!(tree.layout(child)?.bounds.width, 8);
        tree.compute(root, Size::new(13, 2), |_, _| Size::ZERO)?;
        assert_eq!(tree.layout(child)?.bounds.width, 13);
        Ok(())
    }

    #[test]
    fn rejects_layout_node_id_from_another_tree() {
        let mut first = LayoutTree::new();
        let foreign = first.add(LayoutStyle::default());
        let mut second = LayoutTree::new();
        second.add(LayoutStyle::default());

        assert_eq!(
            second.children(foreign),
            Err(LayoutError::UnknownNode(foreign))
        );
    }

    #[test]
    fn rejects_layout_node_id_obtained_before_clear() {
        let mut tree = LayoutTree::new();
        let stale = tree.add(LayoutStyle::default());
        tree.clear();
        tree.add(LayoutStyle::default());

        assert_eq!(tree.children(stale), Err(LayoutError::UnknownNode(stale)));
    }

    #[test]
    fn failed_add_with_duplicate_children_does_not_retain_node() {
        let mut tree = LayoutTree::new();
        let child = tree.add(LayoutStyle::default());

        assert_eq!(
            tree.add_with_children(LayoutStyle::default(), &[child, child]),
            Err(LayoutError::DuplicateChild(child))
        );
        assert_eq!(
            tree.nodes.len(),
            1,
            "the failed parent must not be retained"
        );
    }

    #[test]
    fn replacing_dynamic_children_does_not_retain_detached_nodes() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let root = tree.add(LayoutStyle::default());

        for width in 1..=8 {
            let child =
                tree.add(LayoutStyle::new().size(Dimension::cells(width), Dimension::cells(1)));
            tree.set_children(root, &[child])?;
            tree.compute(root, Size::new(8, 1), |_, _| Size::ZERO)?;
        }

        assert_eq!(tree.children(root)?.len(), 1);
        assert_eq!(
            tree.nodes.len(),
            2,
            "only the root and its current dynamic child should remain retained"
        );
        Ok(())
    }

    #[test]
    fn replacing_children_rejects_stale_descendants_after_slot_reuse() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let descendant = tree.add(LayoutStyle::default());
        let child = tree.add_with_children(LayoutStyle::default(), &[descendant])?;
        let root = tree.add_with_children(LayoutStyle::default(), &[child])?;
        let replacement = tree.add(LayoutStyle::default());

        tree.set_children(root, &[replacement])?;
        tree.add(LayoutStyle::default());
        tree.add(LayoutStyle::default());

        assert_eq!(tree.children(child), Err(LayoutError::UnknownNode(child)));
        assert_eq!(
            tree.children(descendant),
            Err(LayoutError::UnknownNode(descendant))
        );
        Ok(())
    }

    #[test]
    fn reparented_descendant_survives_pruning_its_old_ancestor() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let descendant = tree.add(LayoutStyle::default());
        let child = tree.add_with_children(LayoutStyle::default(), &[descendant])?;
        let root = tree.add_with_children(LayoutStyle::default(), &[child])?;

        tree.set_children(root, &[descendant])?;

        assert_eq!(tree.children(root)?, &[descendant]);
        assert_eq!(tree.children(descendant)?, &[]);
        assert_eq!(tree.children(child), Err(LayoutError::UnknownNode(child)));
        Ok(())
    }

    #[test]
    fn reparenting_preserves_the_child_id() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let child = tree.add(LayoutStyle::default());
        let first_parent = tree.add_with_children(LayoutStyle::default(), &[child])?;
        let second_parent = tree.add(LayoutStyle::default());

        tree.set_children(second_parent, &[child])?;

        assert_eq!(tree.children(first_parent)?, &[]);
        assert_eq!(tree.children(second_parent)?, &[child]);
        tree.set_style(child, LayoutStyle::default())?;
        Ok(())
    }
}
