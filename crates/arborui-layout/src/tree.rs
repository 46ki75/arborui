use std::fmt;

use arborui_core::{Insets, Rect, Size};

use crate::{Dimension, LayoutStyle, MeasureInput, engine};

/// Stable, library-owned identity for a node in one layout tree.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LayoutNodeId(usize);

impl LayoutNodeId {
    pub(crate) const fn index(self) -> usize {
        self.0
    }
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

/// Mutable tree of layout styles and computed integer geometry.
#[derive(Clone, Debug, Default)]
pub struct LayoutTree {
    nodes: Vec<Node>,
    layouts: Vec<Option<ComputedLayout>>,
}

impl LayoutTree {
    /// Creates an empty layout tree.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            layouts: Vec::new(),
        }
    }

    /// Adds an unattached node.
    pub fn add(&mut self, style: LayoutStyle) -> LayoutNodeId {
        let id = LayoutNodeId(self.nodes.len());
        self.nodes.push(Node {
            style,
            parent: None,
            children: Vec::new(),
        });
        self.layouts.push(None);
        id
    }

    /// Adds a node and assigns its children.
    pub fn add_with_children(
        &mut self,
        style: LayoutStyle,
        children: &[LayoutNodeId],
    ) -> Result<LayoutNodeId, LayoutError> {
        let node = self.add(style);
        self.set_children(node, children)?;
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
        let mut unique = std::collections::HashSet::with_capacity(children.len());
        for child in children {
            self.node(*child)?;
            if !unique.insert(*child) {
                return Err(LayoutError::DuplicateChild(*child));
            }
            let mut ancestor = Some(parent);
            while let Some(node) = ancestor {
                if node == *child {
                    return Err(LayoutError::Cycle(*child));
                }
                ancestor = self.node(node)?.parent;
            }
        }

        let old_children = std::mem::take(&mut self.node_mut(parent)?.children);
        for child in old_children {
            if self.node(child)?.parent == Some(parent) {
                self.node_mut(child)?.parent = None;
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
            .iter()
            .map(|node| (node.style, node.children.clone()))
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
        self.nodes.clear();
        self.layouts.clear();
    }

    fn node(&self, node: LayoutNodeId) -> Result<&Node, LayoutError> {
        self.nodes
            .get(node.index())
            .ok_or(LayoutError::UnknownNode(node))
    }

    fn node_mut(&mut self, node: LayoutNodeId) -> Result<&mut Node, LayoutError> {
        self.nodes
            .get_mut(node.index())
            .ok_or(LayoutError::UnknownNode(node))
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
}
