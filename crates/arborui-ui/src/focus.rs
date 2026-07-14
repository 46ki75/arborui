use std::{collections::HashMap, fmt};

use arborui_core::{Point, Rect, Size};

use crate::{Key, NodeId, RetainedNode};

/// Logical focus transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusChange {
    /// Previously focused target.
    pub previous: Option<NodeId>,
    /// Newly focused target.
    pub current: Option<NodeId>,
}

impl FocusChange {
    pub(crate) const fn unchanged(node: Option<NodeId>) -> Self {
        Self {
            previous: node,
            current: node,
        }
    }

    /// Returns whether logical focus moved.
    #[must_use]
    pub fn changed(self) -> bool {
        self.previous != self.current
    }
}

/// Programmatic focus failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FocusError {
    /// No retained node has the requested identity.
    UnknownNode(NodeId),
    /// No node in the active scope has the requested key.
    UnknownKey(Key),
    /// More than one node in the active scope has the requested key.
    AmbiguousKey(Key),
    /// The requested node does not accept focus.
    NotFocusable(NodeId),
    /// The requested node is outside the active focus scope.
    InactiveScope(NodeId),
}

impl fmt::Display for FocusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownNode(node) => write!(formatter, "unknown focus node {node:?}"),
            Self::UnknownKey(key) => write!(formatter, "unknown focus key {key:?}"),
            Self::AmbiguousKey(key) => write!(formatter, "ambiguous focus key {key:?}"),
            Self::NotFocusable(node) => write!(formatter, "node {node:?} is not focusable"),
            Self::InactiveScope(node) => {
                write!(formatter, "node {node:?} is outside the active focus scope")
            }
        }
    }
}

impl std::error::Error for FocusError {}

#[derive(Clone, Debug, Default)]
pub(crate) struct FocusManager {
    active_scope: Option<NodeId>,
    focused_by_scope: HashMap<NodeId, NodeId>,
}

impl FocusManager {
    pub(crate) const fn active_scope(&self) -> Option<NodeId> {
        self.active_scope
    }

    pub(crate) fn focused(&self) -> Option<NodeId> {
        self.active_scope
            .and_then(|scope| self.focused_by_scope.get(&scope).copied())
    }

    pub(crate) fn sync(
        &mut self,
        nodes: &HashMap<NodeId, RetainedNode>,
        root: Option<NodeId>,
        viewport: Option<Size>,
    ) -> FocusChange {
        let previous = self.focused();
        let Some(root) = root.filter(|root| nodes.contains_key(root)) else {
            self.active_scope = None;
            self.focused_by_scope.clear();
            return FocusChange {
                previous,
                current: None,
            };
        };
        let mut preorder = Vec::new();
        collect_preorder_with_depth(nodes, root, 0, &mut preorder);
        let scopes = preorder
            .iter()
            .copied()
            .filter(|(node, _, _)| {
                (*node == root || nodes[node].focus_scope)
                    && is_effectively_visible(nodes, *node, viewport)
            })
            .collect::<Vec<_>>();
        self.active_scope = scopes
            .iter()
            .max_by_key(|(_, depth, order)| (*depth, *order))
            .map(|(node, _, _)| *node);
        self.focused_by_scope.retain(|scope, focused| {
            scopes.iter().any(|(candidate, _, _)| candidate == scope)
                && nodes.get(focused).is_some_and(|node| node.focusable)
                && is_effectively_visible(nodes, *focused, viewport)
                && scope_for(nodes, root, *focused) == Some(*scope)
        });
        FocusChange {
            previous,
            current: self.focused(),
        }
    }

    pub(crate) fn repair(
        &mut self,
        nodes: &HashMap<NodeId, RetainedNode>,
        root: Option<NodeId>,
        viewport: Option<Size>,
    ) -> FocusChange {
        let previous = self.focused();
        let Some(scope) = self.active_scope else {
            return FocusChange {
                previous,
                current: None,
            };
        };
        if previous.is_none() {
            let first = focusable_nodes(nodes, root, scope, viewport)
                .into_iter()
                .next();
            if let Some(first) = first {
                self.focused_by_scope.insert(scope, first);
            }
        }
        FocusChange {
            previous,
            current: self.focused(),
        }
    }

    pub(crate) fn focus(
        &mut self,
        nodes: &HashMap<NodeId, RetainedNode>,
        root: Option<NodeId>,
        node: NodeId,
        viewport: Option<Size>,
    ) -> Result<FocusChange, FocusError> {
        let retained = nodes.get(&node).ok_or(FocusError::UnknownNode(node))?;
        if !retained.focusable {
            return Err(FocusError::NotFocusable(node));
        }
        if !is_effectively_visible(nodes, node, viewport) {
            return Err(FocusError::NotFocusable(node));
        }
        let Some(scope) = self.active_scope else {
            return Err(FocusError::InactiveScope(node));
        };
        if root.and_then(|root| scope_for(nodes, root, node)) != Some(scope) {
            return Err(FocusError::InactiveScope(node));
        }
        let previous = self.focused();
        self.focused_by_scope.insert(scope, node);
        Ok(FocusChange {
            previous,
            current: Some(node),
        })
    }

    pub(crate) fn focus_key(
        &mut self,
        nodes: &HashMap<NodeId, RetainedNode>,
        root: Option<NodeId>,
        key: &Key,
        viewport: Option<Size>,
    ) -> Result<FocusChange, FocusError> {
        let Some(scope) = self.active_scope else {
            return Err(FocusError::UnknownKey(key.clone()));
        };
        let matches = focusable_nodes(nodes, root, scope, viewport)
            .into_iter()
            .filter(|node| nodes[node].key.as_ref() == Some(key))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Err(FocusError::UnknownKey(key.clone())),
            [node] => self.focus(nodes, root, *node, viewport),
            _ => Err(FocusError::AmbiguousKey(key.clone())),
        }
    }

    pub(crate) fn traverse(
        &mut self,
        nodes: &HashMap<NodeId, RetainedNode>,
        root: Option<NodeId>,
        reverse: bool,
        viewport: Option<Size>,
    ) -> FocusChange {
        let Some(scope) = self.active_scope else {
            return FocusChange::unchanged(None);
        };
        let candidates = focusable_nodes(nodes, root, scope, viewport);
        if candidates.is_empty() {
            return FocusChange::unchanged(self.focused());
        }
        let previous = self.focused();
        let index =
            previous.and_then(|focused| candidates.iter().position(|node| *node == focused));
        let next = match (index, reverse) {
            (Some(0), true) | (None, true) => *candidates.last().expect("non-empty candidates"),
            (Some(index), true) => candidates[index - 1],
            (Some(index), false) if index + 1 < candidates.len() => candidates[index + 1],
            _ => candidates[0],
        };
        self.focused_by_scope.insert(scope, next);
        FocusChange {
            previous,
            current: Some(next),
        }
    }
}

fn focusable_nodes(
    nodes: &HashMap<NodeId, RetainedNode>,
    root: Option<NodeId>,
    scope: NodeId,
    viewport: Option<Size>,
) -> Vec<NodeId> {
    let Some(root) = root else {
        return Vec::new();
    };
    let mut preorder = Vec::new();
    collect_preorder(nodes, root, &mut preorder);
    let mut candidates = preorder
        .into_iter()
        .enumerate()
        .filter(|(_, node)| {
            nodes[node].focusable
                && is_effectively_visible(nodes, *node, viewport)
                && scope_for(nodes, root, *node) == Some(scope)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(index, node)| (nodes[node].focus_order.unwrap_or(i32::MAX), *index));
    candidates.into_iter().map(|(_, node)| node).collect()
}

fn scope_for(nodes: &HashMap<NodeId, RetainedNode>, root: NodeId, node: NodeId) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        let retained = nodes.get(&candidate)?;
        if candidate == root || retained.focus_scope {
            return Some(candidate);
        }
        current = retained.parent;
    }
    None
}

fn is_effectively_visible(
    nodes: &HashMap<NodeId, RetainedNode>,
    node: NodeId,
    viewport: Option<Size>,
) -> bool {
    let mut current = Some(node);
    let mut target = true;
    let mut visible = viewport.map(|size| Rect::from_origin_size(Point::ORIGIN, size));
    while let Some(candidate) = current {
        let Some(retained) = nodes.get(&candidate) else {
            return false;
        };
        let clip = if target {
            retained.layout
        } else {
            retained.content
        };
        visible = Some(match visible {
            Some(rect) => match rect.intersection(clip) {
                Some(intersection) => intersection,
                None => return false,
            },
            None => retained.layout,
        });
        if retained.layout.is_empty() {
            return false;
        }
        target = false;
        current = retained.parent;
    }
    visible.is_some()
}

fn collect_preorder(nodes: &HashMap<NodeId, RetainedNode>, node: NodeId, output: &mut Vec<NodeId>) {
    let Some(retained) = nodes.get(&node) else {
        return;
    };
    output.push(node);
    for child in &retained.children {
        collect_preorder(nodes, *child, output);
    }
}

fn collect_preorder_with_depth(
    nodes: &HashMap<NodeId, RetainedNode>,
    node: NodeId,
    depth: usize,
    output: &mut Vec<(NodeId, usize, usize)>,
) {
    let Some(retained) = nodes.get(&node) else {
        return;
    };
    let order = output.len();
    output.push((node, depth, order));
    for child in &retained.children {
        collect_preorder_with_depth(nodes, *child, depth + 1, output);
    }
}
