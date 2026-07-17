use std::cell::{RefCell, RefMut};
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use arborui_core::{Insets, Rect, Size};

use crate::{LayoutStyle, MeasureInput, engine};

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
pub(crate) struct Node {
    pub(crate) style: LayoutStyle,
    parent: Option<LayoutNodeId>,
    pub(crate) children: Vec<LayoutNodeId>,
}

#[derive(Clone, Debug)]
struct NodeSlot {
    generation: u64,
    node: Option<Node>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NodeStore {
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

    pub(crate) fn get(&self, node: LayoutNodeId) -> Option<&Node> {
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

    pub(crate) fn iter(&self, tree: u64) -> impl Iterator<Item = (LayoutNodeId, &Node)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(move |(index, slot)| {
                slot.node.as_ref().map(|node| {
                    (
                        LayoutNodeId {
                            tree,
                            index,
                            generation: slot.generation,
                        },
                        node,
                    )
                })
            })
    }

    #[cfg(test)]
    const fn len(&self) -> usize {
        self.len
    }
}

/// Mutable tree of layout styles and computed integer geometry.
#[derive(Debug, Default)]
pub struct LayoutTree {
    tree_id: u64,
    nodes: NodeStore,
    layouts: Vec<Option<ComputedLayout>>,
    engine: Option<Rc<RefCell<engine::Engine>>>,
}

impl Clone for LayoutTree {
    fn clone(&self) -> Self {
        Self {
            tree_id: self.tree_id,
            nodes: self.nodes.clone(),
            layouts: self.layouts.clone(),
            engine: self.engine.as_ref().map(|engine| {
                let cloned = match engine.try_borrow() {
                    Ok(engine) => engine.clone(),
                    Err(_) => engine::Engine::from_nodes(self.tree_id, &self.nodes),
                };
                Rc::new(RefCell::new(cloned))
            }),
        }
    }
}

impl LayoutTree {
    /// Creates an empty layout tree.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tree_id: 0,
            nodes: NodeStore::new(),
            layouts: Vec::new(),
            engine: None,
        }
    }

    /// Clones logical state while sharing derived engine caches until mutation.
    ///
    /// This supports transactional staging with the same intrinsic measurement
    /// source. Callers must invalidate changed intrinsic content before layout.
    /// Do not access either tree reentrantly from a measurement callback.
    #[doc(hidden)]
    #[must_use]
    pub fn clone_for_staging(&self) -> Self {
        Self {
            tree_id: self.tree_id,
            nodes: self.nodes.clone(),
            layouts: self.layouts.clone(),
            engine: self.engine.clone(),
        }
    }

    /// Adds an unattached node.
    pub fn add(&mut self, style: LayoutStyle) -> LayoutNodeId {
        if self.engine.is_some() {
            self.detach_engine_or_rebuild();
        }
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
        self.engine
            .get_or_insert_with(|| Rc::new(RefCell::new(engine::Engine::new())));
        self.engine_mut()
            .expect("an initialized layout engine can be mutated")
            .add(id, style);
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
        self.node(node)?;
        self.detach_engine()?;
        self.engine_mut()?.set_style(node, style)?;
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
        self.detach_engine()?;
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
        self.engine_mut()?.set_children(parent, children)?;
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
    /// If the measurer's result for a node can differ from its previous result,
    /// callers must invalidate that node before computing. This also applies to
    /// staged trees, which may share cached measurements with their source tree.
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
        self.layouts.resize(self.nodes.slots.len(), None);
        let engine = Rc::clone(self.engine.as_ref().ok_or(LayoutError::UnknownNode(root))?);
        let mut engine = engine.try_borrow_mut().map_err(engine_borrow_error)?;
        engine.compute(&self.nodes, root, viewport, measure, &mut self.layouts)?;
        Ok(())
    }

    /// Invalidates cached intrinsic measurement for `node` and its ancestors.
    pub fn invalidate(&mut self, node: LayoutNodeId) -> Result<(), LayoutError> {
        self.node(node)?;
        self.engine_mut()?.invalidate(node)?;
        self.layouts.fill(None);
        Ok(())
    }

    /// Invalidates all cached intrinsic measurements in the tree.
    pub fn invalidate_all(&mut self) -> Result<(), LayoutError> {
        if self.nodes.len == 0 {
            self.layouts.fill(None);
            return Ok(());
        }
        let nodes = self
            .nodes
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                slot.node.as_ref().map(|_| LayoutNodeId {
                    tree: self.tree_id,
                    index,
                    generation: slot.generation,
                })
            })
            .collect::<Vec<_>>();
        {
            let mut engine = self.engine_mut()?;
            for node in nodes {
                engine.invalidate(node)?;
            }
        }
        self.layouts.fill(None);
        Ok(())
    }

    /// Removes a node and its complete subtree.
    pub fn remove(&mut self, node: LayoutNodeId) -> Result<(), LayoutError> {
        let parent = self.node(node)?.parent;
        self.detach_engine()?;
        if let Some(parent) = parent {
            let children = self
                .node(parent)?
                .children
                .iter()
                .copied()
                .filter(|child| *child != node)
                .collect::<Vec<_>>();
            self.node_mut(parent)?.children = children.clone();
            self.engine_mut()?.set_children(parent, &children)?;
        }
        self.remove_subtree(node);
        self.layouts.fill(None);
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
        self.engine = None;
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

    fn engine_mut(&mut self) -> Result<RefMut<'_, engine::Engine>, LayoutError> {
        self.detach_engine()?;
        self.engine
            .as_ref()
            .ok_or_else(engine_not_initialized)?
            .try_borrow_mut()
            .map_err(engine_borrow_error)
    }

    fn detach_engine(&mut self) -> Result<(), LayoutError> {
        let shared = self.engine.as_mut().ok_or_else(engine_not_initialized)?;
        if Rc::strong_count(shared) > 1 {
            let detached = shared.try_borrow().map_err(engine_borrow_error)?.clone();
            *shared = Rc::new(RefCell::new(detached));
        }
        Ok(())
    }

    fn detach_engine_or_rebuild(&mut self) {
        let Some(shared) = self.engine.as_mut() else {
            return;
        };
        if Rc::strong_count(shared) > 1 {
            // `add` cannot report a borrow error, so recover from a reentrant staged
            // computation by rebuilding this derived cache from canonical tree state.
            let detached = shared.try_borrow().map_or_else(
                |_| engine::Engine::from_nodes(self.tree_id, &self.nodes),
                |engine| engine.clone(),
            );
            *shared = Rc::new(RefCell::new(detached));
        }
    }

    fn remove_subtree(&mut self, node: LayoutNodeId) {
        if self.engine.is_some() {
            self.detach_engine_or_rebuild();
        }
        let Some(removed) = self.nodes.remove(node) else {
            return;
        };
        if self.engine.is_some() {
            self.engine_mut()
                .expect("an initialized layout engine can be mutated")
                .remove(node);
        }
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

    #[cfg(test)]
    fn shares_engine_with(&self, other: &Self) -> bool {
        self.engine
            .as_ref()
            .zip(other.engine.as_ref())
            .is_some_and(|(left, right)| Rc::ptr_eq(left, right))
    }
}

fn engine_not_initialized() -> LayoutError {
    LayoutError::Engine("layout engine is not initialized".into())
}

fn engine_borrow_error<T>(_: T) -> LayoutError {
    LayoutError::Engine("layout engine is already borrowed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Align, Dimension, FlexDirection, Justify};

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
    fn cumulative_rounding_covers_nested_fractional_percentage_parent() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let first = tree.add(LayoutStyle::new().size(Dimension::percent(33), Dimension::cells(1)));
        let second = tree.add(LayoutStyle::new().size(Dimension::percent(33), Dimension::cells(1)));
        let third = tree.add(LayoutStyle::new().size(Dimension::percent(34), Dimension::cells(1)));
        let parent = tree.add_with_children(
            LayoutStyle::new().size(Dimension::percent(90), Dimension::cells(1)),
            &[first, second, third],
        )?;
        let root = tree.add_with_children(
            LayoutStyle {
                justify: Justify::Center,
                ..LayoutStyle::new().size(Dimension::cells(11), Dimension::cells(1))
            },
            &[parent],
        )?;

        tree.compute(root, Size::new(11, 1), |_, _| Size::ZERO)?;

        let parent = tree.layout(parent)?.bounds;
        let first = tree.layout(first)?.bounds;
        let second = tree.layout(second)?.bounds;
        let third = tree.layout(third)?.bounds;
        assert_eq!(parent, Rect::new(1, 0, 9, 1));
        assert_eq!(first.x, parent.x);
        assert_eq!(first.right(), second.x);
        assert_eq!(second.right(), third.x);
        assert_eq!(third.right(), parent.right());
        Ok(())
    }

    #[test]
    fn cumulative_rounding_distributes_equal_flex_without_seams() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let child_style = LayoutStyle::new()
            .size(Dimension::cells(0), Dimension::cells(1))
            .flex(1, 1);
        let first = tree.add(child_style);
        let second = tree.add(child_style);
        let third = tree.add(child_style);
        let root = tree.add_with_children(
            LayoutStyle::new().size(Dimension::cells(10), Dimension::cells(1)),
            &[first, second, third],
        )?;

        tree.compute(root, Size::new(10, 1), |_, _| Size::ZERO)?;

        let root = tree.layout(root)?.bounds;
        let first = tree.layout(first)?.bounds;
        let second = tree.layout(second)?.bounds;
        let third = tree.layout(third)?.bounds;
        assert_eq!([first.width, second.width, third.width], [3, 4, 3]);
        assert_eq!(first.x, root.x);
        assert_eq!(first.right(), second.x);
        assert_eq!(second.right(), third.x);
        assert_eq!(third.right(), root.right());
        Ok(())
    }

    #[test]
    fn cumulative_rounding_distributes_vertical_flex_without_seams() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let child_style = LayoutStyle::new()
            .size(Dimension::cells(1), Dimension::cells(0))
            .flex(1, 1);
        let first = tree.add(child_style);
        let second = tree.add(child_style);
        let third = tree.add(child_style);
        let root = tree.add_with_children(
            LayoutStyle {
                direction: FlexDirection::Column,
                ..LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(10))
            },
            &[first, second, third],
        )?;

        tree.compute(root, Size::new(1, 10), |_, _| Size::ZERO)?;

        let root = tree.layout(root)?.bounds;
        let first = tree.layout(first)?.bounds;
        let second = tree.layout(second)?.bounds;
        let third = tree.layout(third)?.bounds;
        assert_eq!([first.height, second.height, third.height], [3, 4, 3]);
        assert_eq!(first.y, root.y);
        assert_eq!(first.bottom(), second.y);
        assert_eq!(second.bottom(), third.y);
        assert_eq!(third.bottom(), root.bottom());
        Ok(())
    }

    #[test]
    fn cumulative_rounding_preserves_space_between_gap() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let child_style = LayoutStyle::new().size(Dimension::cells(3), Dimension::cells(1));
        let first = tree.add(child_style);
        let second = tree.add(child_style);
        let root = tree.add_with_children(
            LayoutStyle {
                justify: Justify::SpaceBetween,
                ..LayoutStyle::new().size(Dimension::cells(11), Dimension::cells(1))
            },
            &[first, second],
        )?;

        tree.compute(root, Size::new(11, 1), |_, _| Size::ZERO)?;

        let root = tree.layout(root)?.bounds;
        let first = tree.layout(first)?.bounds;
        let second = tree.layout(second)?.bounds;
        assert_eq!(first.x, root.x);
        assert_eq!(second.right(), root.right());
        assert_eq!(second.x - first.right(), 5);
        Ok(())
    }

    #[test]
    fn cumulative_rounding_centers_overflow_without_seams() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let child_style = LayoutStyle::new()
            .size(Dimension::cells(1), Dimension::cells(1))
            .flex(0, 0);
        let first = tree.add(child_style);
        let second = tree.add(child_style);
        let third = tree.add(child_style);
        let root = tree.add_with_children(
            LayoutStyle {
                justify: Justify::Center,
                ..LayoutStyle::new().size(Dimension::cells(2), Dimension::cells(1))
            },
            &[first, second, third],
        )?;

        tree.compute(root, Size::new(2, 1), |_, _| Size::ZERO)?;

        let first = tree.layout(first)?.bounds;
        let second = tree.layout(second)?.bounds;
        let third = tree.layout(third)?.bounds;
        assert_eq!([first.x, second.x, third.x], [0, 1, 2]);
        assert_eq!(first.right(), second.x);
        assert_eq!(second.right(), third.x);
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
    fn reuses_cached_measurement_until_explicitly_invalidated() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let leaf = tree.add(LayoutStyle::default());
        let root = tree.add_with_children(LayoutStyle::default(), &[leaf])?;
        let mut measurements = 0;

        tree.compute(root, Size::new(8, 1), |_, _| {
            measurements += 1;
            Size::new(3, 1)
        })?;
        assert!(measurements > 0);

        measurements = 0;
        tree.compute(root, Size::new(8, 1), |_, _| {
            measurements += 1;
            Size::new(3, 1)
        })?;
        assert_eq!(measurements, 0);

        tree.invalidate(leaf)?;
        tree.compute(root, Size::new(8, 1), |_, _| {
            measurements += 1;
            Size::new(5, 1)
        })?;
        assert!(measurements > 0);
        assert_eq!(tree.layout(leaf)?.bounds.width, 5);
        Ok(())
    }

    #[test]
    fn cloned_trees_share_computed_cache_until_semantic_mutation() -> Result<(), LayoutError> {
        let mut source = LayoutTree::new();
        let child =
            source.add(LayoutStyle::new().size(Dimension::percent(100), Dimension::cells(1)));
        let root = source.add_with_children(LayoutStyle::default(), &[child])?;
        source.compute(root, Size::new(8, 1), |_, _| Size::ZERO)?;

        let mut staged = source.clone_for_staging();
        assert!(source.shares_engine_with(&staged));

        staged.compute(root, Size::new(13, 1), |_, _| Size::ZERO)?;
        assert!(source.shares_engine_with(&staged));
        assert_eq!(source.layout(child)?.bounds.width, 8);
        assert_eq!(staged.layout(child)?.bounds.width, 13);

        staged.set_style(
            child,
            LayoutStyle::new().size(Dimension::cells(3), Dimension::cells(1)),
        )?;
        assert!(!source.shares_engine_with(&staged));
        staged.compute(root, Size::new(13, 1), |_, _| Size::ZERO)?;
        source.compute(root, Size::new(8, 1), |_, _| Size::ZERO)?;
        assert_eq!(source.layout(child)?.bounds.width, 8);
        assert_eq!(staged.layout(child)?.bounds.width, 3);
        Ok(())
    }

    #[test]
    fn ordinary_clones_keep_measurement_caches_independent() -> Result<(), LayoutError> {
        let mut source = LayoutTree::new();
        let leaf = source.add(LayoutStyle::default());
        let root = source.add_with_children(LayoutStyle::default(), &[leaf])?;
        let mut cloned = source.clone();

        source.compute(root, Size::new(8, 1), |_, _| Size::new(3, 1))?;
        cloned.compute(root, Size::new(8, 1), |_, _| Size::new(5, 1))?;

        assert_eq!(source.layout(leaf)?.bounds.width, 3);
        assert_eq!(cloned.layout(leaf)?.bounds.width, 5);
        Ok(())
    }

    #[test]
    fn ordinary_clone_during_staged_measurement_uses_an_independent_engine()
    -> Result<(), LayoutError> {
        let mut source = LayoutTree::new();
        let leaf = source.add(LayoutStyle::default());
        let root = source.add_with_children(LayoutStyle::default(), &[leaf])?;
        let mut staged = source.clone_for_staging();
        let mut cloned = None;

        staged.compute(root, Size::new(8, 1), |_, _| {
            cloned = Some(source.clone());
            Size::new(3, 1)
        })?;

        let mut cloned = cloned.expect("the leaf measurement should clone the source tree");
        assert!(!source.shares_engine_with(&cloned));
        cloned.compute(root, Size::new(8, 1), |_, _| Size::new(5, 1))?;
        assert_eq!(cloned.layout(leaf)?.bounds.width, 5);
        Ok(())
    }

    #[test]
    fn reentrant_staging_mutation_fails_before_changing_logical_state() -> Result<(), LayoutError> {
        let mut source = LayoutTree::new();
        let leaf = source.add(LayoutStyle::default());
        let root = source.add_with_children(LayoutStyle::default(), &[leaf])?;
        let mut staged = source.clone_for_staging();
        let changed = LayoutStyle::new().size(Dimension::cells(5), Dimension::cells(1));
        let mut mutation = None;

        staged.compute(root, Size::new(8, 1), |_, _| {
            mutation = Some(source.set_style(leaf, changed));
            Size::new(3, 1)
        })?;

        assert!(matches!(mutation, Some(Err(LayoutError::Engine(_)))));
        assert_eq!(source.node(leaf)?.style, LayoutStyle::default());
        Ok(())
    }

    #[test]
    fn reentrant_staging_add_detaches_from_the_borrowed_engine() -> Result<(), LayoutError> {
        let mut source = LayoutTree::new();
        let leaf = source.add(LayoutStyle::default());
        let root = source.add_with_children(LayoutStyle::default(), &[leaf])?;
        let mut staged = source.clone_for_staging();
        let mut added = None;

        staged.compute(root, Size::new(8, 1), |_, _| {
            added = Some(source.add(LayoutStyle::default()));
            Size::new(3, 1)
        })?;

        let added = added.expect("the leaf measurement should add a source node");
        assert!(source.node(added).is_ok());
        assert!(!source.shares_engine_with(&staged));
        Ok(())
    }

    #[test]
    fn restores_updated_canonical_style_when_switching_roots() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let first = tree.add(LayoutStyle::default());
        let second = tree.add(LayoutStyle::default());

        tree.compute(first, Size::new(8, 1), |_, _| Size::ZERO)?;
        tree.set_style(
            first,
            LayoutStyle::new().size(Dimension::cells(3), Dimension::cells(1)),
        )?;
        tree.compute(second, Size::new(8, 1), |_, _| Size::ZERO)?;
        tree.compute(first, Size::new(8, 1), |_, _| Size::ZERO)?;

        assert_eq!(tree.layout(first)?.bounds.size(), Size::new(3, 1));
        Ok(())
    }

    #[test]
    fn removing_a_subtree_updates_its_parent() -> Result<(), LayoutError> {
        let mut tree = LayoutTree::new();
        let descendant = tree.add(LayoutStyle::default());
        let child = tree.add_with_children(LayoutStyle::default(), &[descendant])?;
        let root = tree.add_with_children(LayoutStyle::default(), &[child])?;

        tree.remove(child)?;
        tree.compute(root, Size::new(8, 1), |_, _| Size::ZERO)?;

        assert!(tree.children(root)?.is_empty());
        assert_eq!(tree.layout(child), Err(LayoutError::UnknownNode(child)));
        assert_eq!(
            tree.layout(descendant),
            Err(LayoutError::UnknownNode(descendant))
        );
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
