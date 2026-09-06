use taffy::{
    AvailableSpace, CacheTree, Layout, LayoutFlexboxContainer, LayoutInput, LayoutOutput,
    LayoutPartialTree, MaybeMath, MaybeResolve, NodeId, RequestedAxis, RoundTree, RunMode,
    SizingMode, Style, TaffyTree, TraversePartialTree, TraverseTree, geometry::Size,
};

use crate::LayoutNodeId;

// TaffyTree owns topology and invalidation, but its high-level measurement callback
// omits LayoutInput. Store geometry here so the low-level adapter can retain that
// sizing context while still using Taffy's cache and cumulative rounding algorithms.
// TaffyTree's geometry getters are read-only; the setters live in private TaffyView,
// so its own geometry records cannot be populated by this adapter.
#[derive(Clone, Debug)]
pub(super) struct NodeLayout {
    node: LayoutNodeId,
    pub(super) unrounded: Layout,
    pub(super) rounded: Layout,
}

impl NodeLayout {
    pub(super) fn new(node: LayoutNodeId) -> Self {
        Self {
            node,
            unrounded: Layout::new(),
            rounded: Layout::new(),
        }
    }
}

pub(super) struct LayoutView<'a, F> {
    pub(super) tree: &'a mut TaffyTree<NodeLayout>,
    pub(super) measure: F,
}

impl<F> TraversePartialTree for LayoutView<'_, F> {
    type ChildIter<'a>
        = <TaffyTree<NodeLayout> as TraversePartialTree>::ChildIter<'a>
    where
        Self: 'a;

    fn child_ids(&self, node: NodeId) -> Self::ChildIter<'_> {
        self.tree.child_ids(node)
    }

    fn child_count(&self, node: NodeId) -> usize {
        self.tree.child_count(node)
    }

    fn get_child_id(&self, node: NodeId, index: usize) -> NodeId {
        self.tree.get_child_id(node, index)
    }
}

impl<F> TraverseTree for LayoutView<'_, F> {}

impl<F> CacheTree for LayoutView<'_, F> {
    fn cache_get(&self, node: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.tree.cache_get(node, input)
    }

    fn cache_store(&mut self, node: NodeId, input: &LayoutInput, output: LayoutOutput) {
        self.tree.cache_store(node, input, output);
    }

    fn cache_clear(&mut self, node: NodeId) {
        self.tree.cache_clear(node);
    }
}

impl<F> LayoutPartialTree for LayoutView<'_, F>
where
    F: FnMut(Size<Option<f32>>, Size<AvailableSpace>, LayoutNodeId) -> Size<f32>,
{
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type CustomIdent = <Style as taffy::CoreStyle>::CustomIdent;

    fn get_core_container_style(&self, node: NodeId) -> Self::CoreContainerStyle<'_> {
        self.tree
            .style(node)
            .expect("layout nodes must have styles")
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        self.tree
            .get_node_context_mut(node)
            .expect("layout nodes must have context")
            .unrounded = *layout;
    }

    fn compute_child_layout(&mut self, node: NodeId, mut input: LayoutInput) -> LayoutOutput {
        if input.run_mode == RunMode::PerformHiddenLayout {
            return taffy::compute_hidden_layout(self, node);
        }

        if input.sizing_mode == SizingMode::ContentSize
            && input.axis == RequestedAxis::Vertical
            && self.tree.parent(node).is_some_and(|parent| {
                matches!(
                    self.get_core_container_style(parent).flex_direction,
                    taffy::FlexDirection::Column | taffy::FlexDirection::ColumnReverse
                )
            })
        {
            // Taffy 0.12.1 passes an unclamped preferred cross size into column
            // flex-basis/automatic-minimum measurement. Resolve only that width:
            // ContentSize must continue to ignore MAIN-axis min/max constraints.
            // Do this before cache lookup and before Taffy subtracts box insets.
            let style = self.get_core_container_style(node);
            let min = style
                .min_size
                .width
                .maybe_resolve(input.parent_size.width, |_, _| 0.0);
            let max = style
                .max_size
                .width
                .maybe_resolve(input.parent_size.width, |_, _| 0.0);
            input.known_dimensions.width = input.known_dimensions.width.maybe_clamp(min, max);
        }

        taffy::compute_cached_layout(self, node, input, |view, node, input| {
            if view.get_core_container_style(node).display == taffy::Display::None {
                taffy::compute_hidden_layout(view, node)
            } else if view.child_count(node) > 0 {
                taffy::compute_flexbox_layout(view, node, input)
            } else {
                let style = view
                    .tree
                    .style(node)
                    .expect("layout nodes must have styles");
                let id = view
                    .tree
                    .get_node_context(node)
                    .expect("layout nodes must have context")
                    .node;
                taffy::compute_leaf_layout(
                    input,
                    style,
                    |_, _| 0.0,
                    |known, available| (view.measure)(known, available, id),
                )
            }
        })
    }
}

impl<F> LayoutFlexboxContainer for LayoutView<'_, F>
where
    F: FnMut(Size<Option<f32>>, Size<AvailableSpace>, LayoutNodeId) -> Size<f32>,
{
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node: NodeId) -> Self::FlexboxContainerStyle<'_> {
        self.get_core_container_style(node)
    }

    fn get_flexbox_child_style(&self, node: NodeId) -> Self::FlexboxItemStyle<'_> {
        self.get_core_container_style(node)
    }
}

impl<F> RoundTree for LayoutView<'_, F> {
    fn get_unrounded_layout(&self, node: NodeId) -> Layout {
        self.tree
            .get_node_context(node)
            .expect("layout nodes must have context")
            .unrounded
    }

    fn set_final_layout(&mut self, node: NodeId, layout: &Layout) {
        self.tree
            .get_node_context_mut(node)
            .expect("layout nodes must have context")
            .rounded = *layout;
    }
}
