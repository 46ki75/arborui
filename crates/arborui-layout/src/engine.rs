use arborui_core::{Insets, Point, Rect, Size};
use taffy::{
    AvailableSpace as TaffyAvailableSpace, Dimension as TaffyDimension, LengthPercentage,
    TaffyTree,
    geometry::{Rect as TaffyRect, Size as TaffySize},
    style::{
        AlignItems, FlexDirection as TaffyFlexDirection, JustifyContent, Position as TaffyPosition,
        Style as TaffyStyle,
    },
};

use crate::{
    Align, AvailableSpace, ComputedLayout, Dimension, FlexDirection, Justify, LayoutError,
    LayoutNodeId, LayoutStyle, MeasureInput, Position, tree::NodeStore,
};

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    tree: TaffyTree<LayoutNodeId>,
    backend_ids: Vec<Option<(LayoutNodeId, taffy::NodeId)>>,
    effective_root: Option<(LayoutNodeId, LayoutStyle)>,
}

impl Engine {
    pub(crate) fn new() -> Self {
        let mut tree = TaffyTree::new();
        tree.enable_rounding();
        Self {
            tree,
            backend_ids: Vec::new(),
            effective_root: None,
        }
    }

    pub(crate) fn from_nodes(tree_id: u64, nodes: &NodeStore) -> Self {
        let mut engine = Self::new();
        for (id, node) in nodes.iter(tree_id) {
            engine.add(id, node.style);
        }
        for (id, node) in nodes.iter(tree_id) {
            engine
                .set_children(id, &node.children)
                .expect("retained layout nodes must form a valid engine tree");
        }
        engine
    }

    pub(crate) fn add(&mut self, node: LayoutNodeId, style: LayoutStyle) {
        if self.backend_ids.len() <= node.index() {
            self.backend_ids.resize(node.index() + 1, None);
        }
        let backend = self
            .tree
            .new_leaf_with_context(taffy_style(style), node)
            .expect("adding a Taffy node to an in-memory tree cannot fail");
        self.backend_ids[node.index()] = Some((node, backend));
    }

    pub(crate) fn set_style(
        &mut self,
        node: LayoutNodeId,
        style: LayoutStyle,
    ) -> Result<(), LayoutError> {
        if let Some((root, canonical)) = &mut self.effective_root {
            if *root == node {
                *canonical = style;
            }
        }
        self.set_backend_style(node, style)
    }

    fn set_backend_style(
        &mut self,
        node: LayoutNodeId,
        style: LayoutStyle,
    ) -> Result<(), LayoutError> {
        let backend = self.backend(node)?;
        let style = taffy_style(style);
        if self.tree.style(backend).map_err(engine_error)? != &style {
            self.tree.set_style(backend, style).map_err(engine_error)?;
        }
        Ok(())
    }

    pub(crate) fn set_children(
        &mut self,
        parent: LayoutNodeId,
        children: &[LayoutNodeId],
    ) -> Result<(), LayoutError> {
        let backend_parent = self.backend(parent)?;
        let backend_children = children
            .iter()
            .map(|child| self.backend(*child))
            .collect::<Result<Vec<_>, _>>()?;
        if self.tree.children(backend_parent).map_err(engine_error)? != backend_children {
            self.tree
                .set_children(backend_parent, &backend_children)
                .map_err(engine_error)?;
        }
        Ok(())
    }

    pub(crate) fn remove(&mut self, node: LayoutNodeId) {
        let Some(slot) = self.backend_ids.get_mut(node.index()) else {
            return;
        };
        let Some((mapped, backend)) = *slot else {
            return;
        };
        if mapped != node {
            return;
        }
        *slot = None;
        if self.effective_root.is_some_and(|(root, _)| root == node) {
            self.effective_root = None;
        }
        self.tree
            .remove(backend)
            .expect("removing a known Taffy node cannot fail");
    }

    pub(crate) fn invalidate(&mut self, node: LayoutNodeId) -> Result<(), LayoutError> {
        let backend = self.backend(node)?;
        self.tree.mark_dirty(backend).map_err(engine_error)
    }

    pub(crate) fn compute<F>(
        &mut self,
        nodes: &NodeStore,
        root: LayoutNodeId,
        viewport: Size,
        mut measure: F,
        layouts: &mut [Option<ComputedLayout>],
    ) -> Result<(), LayoutError>
    where
        F: FnMut(LayoutNodeId, MeasureInput) -> Size,
    {
        self.prepare_root(nodes, root, viewport)?;
        let backend_root = self.backend(root)?;
        self.tree
            .compute_layout_with_measure(
                backend_root,
                TaffySize {
                    width: TaffyAvailableSpace::Definite(f32::from(viewport.width)),
                    height: TaffyAvailableSpace::Definite(f32::from(viewport.height)),
                },
                |known, available, _, context, _| {
                    let Some(node) = context.copied() else {
                        return TaffySize::ZERO;
                    };
                    let measured = measure(
                        node,
                        MeasureInput {
                            known_width: known.width.map(round_u16),
                            known_height: known.height.map(round_u16),
                            available_width: available_space(available.width),
                            available_height: available_space(available.height),
                        },
                    );
                    TaffySize {
                        width: f32::from(measured.width),
                        height: f32::from(measured.height),
                    }
                },
            )
            .map_err(engine_error)?;

        layouts.fill(None);
        self.collect_layouts(nodes, root, (0.0, 0.0), layouts)
    }

    fn prepare_root(
        &mut self,
        nodes: &NodeStore,
        root: LayoutNodeId,
        viewport: Size,
    ) -> Result<(), LayoutError> {
        if let Some((previous, style)) = self.effective_root {
            if previous != root && nodes.get(previous).is_some() {
                self.set_backend_style(previous, style)?;
                // Root computation overwrites parent-relative geometry even when
                // the style is unchanged, so invalidate the root and its ancestors.
                self.invalidate(previous)?;
            }
        }

        let style = nodes.get(root).ok_or(LayoutError::UnknownNode(root))?.style;
        let mut effective = style;
        if effective.width == Dimension::Auto {
            effective.width = Dimension::Cells(viewport.width);
        }
        if effective.height == Dimension::Auto {
            effective.height = Dimension::Cells(viewport.height);
        }
        self.set_backend_style(root, effective)?;
        self.effective_root = Some((root, style));
        Ok(())
    }

    fn collect_layouts(
        &self,
        nodes: &NodeStore,
        node: LayoutNodeId,
        parent_origin: (f32, f32),
        output: &mut [Option<ComputedLayout>],
    ) -> Result<(), LayoutError> {
        let backend = self.backend(node)?;
        let layout = self.tree.layout(backend).map_err(engine_error)?;
        let unrounded_layout = self.tree.unrounded_layout(backend);
        // Taffy's rounded sizes are cumulative edge differences; accumulate its
        // parent-relative source locations before producing root coordinates.
        let unrounded_origin = (
            parent_origin.0 + unrounded_layout.location.x,
            parent_origin.1 + unrounded_layout.location.y,
        );
        let origin = Point::new(round_i32(unrounded_origin.0), round_i32(unrounded_origin.1));
        let bounds = Rect::from_origin_size(
            origin,
            Size::new(
                integer_u16(layout.size.width),
                integer_u16(layout.size.height),
            ),
        );
        let border = insets(layout.border);
        let padding = insets(layout.padding);
        output[node.index()] = Some(ComputedLayout {
            bounds,
            content: bounds.inner(Insets::new(
                border.top.saturating_add(padding.top),
                border.right.saturating_add(padding.right),
                border.bottom.saturating_add(padding.bottom),
                border.left.saturating_add(padding.left),
            )),
            padding,
            border,
            order: layout.order,
        });

        let retained = nodes.get(node).ok_or(LayoutError::UnknownNode(node))?;
        for child in &retained.children {
            self.collect_layouts(nodes, *child, unrounded_origin, output)?;
        }
        Ok(())
    }

    fn backend(&self, node: LayoutNodeId) -> Result<taffy::NodeId, LayoutError> {
        self.backend_ids
            .get(node.index())
            .copied()
            .flatten()
            .filter(|(mapped, _)| *mapped == node)
            .map(|(_, backend)| backend)
            .ok_or(LayoutError::UnknownNode(node))
    }
}

fn taffy_style(style: LayoutStyle) -> TaffyStyle {
    TaffyStyle {
        size: TaffySize {
            width: dimension(style.width),
            height: dimension(style.height),
        },
        min_size: TaffySize {
            width: dimension(style.min_width),
            height: dimension(style.min_height),
        },
        max_size: TaffySize {
            width: dimension(style.max_width),
            height: dimension(style.max_height),
        },
        flex_direction: match style.direction {
            FlexDirection::Row => TaffyFlexDirection::Row,
            FlexDirection::Column => TaffyFlexDirection::Column,
            FlexDirection::RowReverse => TaffyFlexDirection::RowReverse,
            FlexDirection::ColumnReverse => TaffyFlexDirection::ColumnReverse,
        },
        align_items: Some(match style.align {
            Align::Start => AlignItems::START,
            Align::Center => AlignItems::CENTER,
            Align::End => AlignItems::END,
            Align::Stretch => AlignItems::STRETCH,
        }),
        justify_content: Some(match style.justify {
            Justify::Start => JustifyContent::START,
            Justify::Center => JustifyContent::CENTER,
            Justify::End => JustifyContent::END,
            Justify::SpaceBetween => JustifyContent::SPACE_BETWEEN,
            Justify::SpaceAround => JustifyContent::SPACE_AROUND,
            Justify::SpaceEvenly => JustifyContent::SPACE_EVENLY,
        }),
        flex_grow: f32::from(style.flex_grow),
        flex_shrink: f32::from(style.flex_shrink),
        gap: TaffySize {
            width: LengthPercentage::length(f32::from(style.gap)),
            height: LengthPercentage::length(f32::from(style.gap)),
        },
        padding: taffy_insets(style.padding),
        border: taffy_insets(style.border),
        position: match style.position {
            Position::Relative => TaffyPosition::Relative,
            Position::Absolute => TaffyPosition::Absolute,
        },
        ..TaffyStyle::default()
    }
}

fn dimension(value: Dimension) -> TaffyDimension {
    match value {
        Dimension::Auto => TaffyDimension::auto(),
        Dimension::Cells(value) => TaffyDimension::length(f32::from(value)),
        Dimension::Percent(value) => TaffyDimension::percent(f32::from(value) / 100.0),
    }
}

fn taffy_insets(value: Insets) -> TaffyRect<LengthPercentage> {
    TaffyRect {
        top: LengthPercentage::length(f32::from(value.top)),
        right: LengthPercentage::length(f32::from(value.right)),
        bottom: LengthPercentage::length(f32::from(value.bottom)),
        left: LengthPercentage::length(f32::from(value.left)),
    }
}

fn insets(value: TaffyRect<f32>) -> Insets {
    Insets::new(
        integer_u16(value.top),
        integer_u16(value.right),
        integer_u16(value.bottom),
        integer_u16(value.left),
    )
}

fn available_space(value: TaffyAvailableSpace) -> AvailableSpace {
    match value {
        TaffyAvailableSpace::Definite(value) => AvailableSpace::Definite(floor_u16(value)),
        TaffyAvailableSpace::MinContent => AvailableSpace::MinContent,
        TaffyAvailableSpace::MaxContent => AvailableSpace::MaxContent,
    }
}

fn engine_error(error: taffy::TaffyError) -> LayoutError {
    LayoutError::Engine(error.to_string())
}

fn floor_u16(value: f32) -> u16 {
    value.floor().clamp(0.0, f32::from(u16::MAX)) as u16
}

fn round_u16(value: f32) -> u16 {
    value.round().clamp(0.0, f32::from(u16::MAX)) as u16
}

fn integer_u16(value: f32) -> u16 {
    value.clamp(0.0, f32::from(u16::MAX)) as u16
}

fn round_i32(value: f32) -> i32 {
    (value + 0.5)
        .floor()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definite_measure_constraints_do_not_round_up() {
        assert_eq!(
            available_space(TaffyAvailableSpace::Definite(4.9)),
            AvailableSpace::Definite(4)
        );
    }
}
