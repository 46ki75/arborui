use taffy::{
    AvailableSpace as TaffyAvailableSpace, Dimension as TaffyDimension, LengthPercentage,
    TaffyTree,
    geometry::{Rect as TaffyRect, Size as TaffySize},
    style::{AlignItems, FlexDirection as TaffyFlexDirection, JustifyContent, Style as TaffyStyle},
};
use yatui_core::{Insets, Point, Rect, Size};

use crate::{
    Align, AvailableSpace, ComputedLayout, Dimension, FlexDirection, Justify, LayoutError,
    LayoutNodeId, LayoutStyle, MeasureInput,
};

pub(crate) struct EngineResult {
    pub(crate) layouts: Vec<Option<ComputedLayout>>,
}

pub(crate) fn compute<F>(
    styles: &[(LayoutStyle, Vec<LayoutNodeId>)],
    root: LayoutNodeId,
    viewport: Size,
    mut measure: F,
) -> Result<EngineResult, LayoutError>
where
    F: FnMut(LayoutNodeId, MeasureInput) -> Size,
{
    let mut tree = TaffyTree::with_capacity(styles.len());
    let mut backend_ids = vec![None; styles.len()];
    build_node(&mut tree, styles, root, &mut backend_ids)?;
    let backend_root = backend_ids[root.index()].ok_or(LayoutError::UnknownNode(root))?;

    tree.compute_layout_with_measure(
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
    )?;

    let mut layouts = vec![None; styles.len()];
    collect_layouts(
        &tree,
        styles,
        &backend_ids,
        root,
        Point::ORIGIN,
        &mut layouts,
    )?;
    Ok(EngineResult { layouts })
}

fn build_node(
    tree: &mut TaffyTree<LayoutNodeId>,
    nodes: &[(LayoutStyle, Vec<LayoutNodeId>)],
    node: LayoutNodeId,
    backend_ids: &mut [Option<taffy::NodeId>],
) -> Result<taffy::NodeId, LayoutError> {
    if let Some(id) = backend_ids.get(node.index()).copied().flatten() {
        return Ok(id);
    }
    let (style, children) = nodes
        .get(node.index())
        .ok_or(LayoutError::UnknownNode(node))?;
    let backend = if children.is_empty() {
        tree.new_leaf_with_context(taffy_style(*style), node)?
    } else {
        let children = children
            .iter()
            .map(|child| build_node(tree, nodes, *child, backend_ids))
            .collect::<Result<Vec<_>, _>>()?;
        tree.new_with_children(taffy_style(*style), &children)?
    };
    backend_ids[node.index()] = Some(backend);
    Ok(backend)
}

fn collect_layouts(
    tree: &TaffyTree<LayoutNodeId>,
    nodes: &[(LayoutStyle, Vec<LayoutNodeId>)],
    backend_ids: &[Option<taffy::NodeId>],
    node: LayoutNodeId,
    parent_origin: Point,
    output: &mut [Option<ComputedLayout>],
) -> Result<(), LayoutError> {
    let backend = backend_ids[node.index()].ok_or(LayoutError::UnknownNode(node))?;
    let layout = tree.layout(backend)?;
    let origin =
        parent_origin.translated(round_i32(layout.location.x), round_i32(layout.location.y));
    let bounds = Rect::from_origin_size(
        origin,
        Size::new(round_u16(layout.size.width), round_u16(layout.size.height)),
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

    for child in &nodes[node.index()].1 {
        collect_layouts(tree, nodes, backend_ids, *child, origin, output)?;
    }
    Ok(())
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
        round_u16(value.top),
        round_u16(value.right),
        round_u16(value.bottom),
        round_u16(value.left),
    )
}

fn available_space(value: TaffyAvailableSpace) -> AvailableSpace {
    match value {
        TaffyAvailableSpace::Definite(value) => AvailableSpace::Definite(round_u16(value)),
        TaffyAvailableSpace::MinContent => AvailableSpace::MinContent,
        TaffyAvailableSpace::MaxContent => AvailableSpace::MaxContent,
    }
}

fn round_u16(value: f32) -> u16 {
    value.round().clamp(0.0, f32::from(u16::MAX)) as u16
}

fn round_i32(value: f32) -> i32 {
    value.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}
