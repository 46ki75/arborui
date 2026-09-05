use super::*;
use arborui_core::Insets;
use arborui_layout::{Align, Justify};

#[test]
fn child_cursor_uses_content_size_origin_and_offsets_once() -> Result<(), UiError> {
    let view = Element::<()>::container([Element::container([Element::text("abc").layout(
        LayoutStyle::new()
            .padding(Insets::all(1))
            .border(Insets::all(1)),
    )])
    .layout(LayoutStyle {
        justify: Justify::Center,
        align: Align::Center,
        ..LayoutStyle::new().size(Dimension::cells(12), Dimension::cells(9))
    })
    .focusable(true)
    .child_offset(Point::new(-1, 1))
    .cursor_with_child(0, 0, |policy, size| {
        assert_eq!(policy, WidthPolicy::Cjk);
        assert_eq!(size, Size::new(3, 1));
        CursorState::visible(Point::new(3, 0))
            .with_shape(CursorShape::Bar)
            .with_blinking(true)
    })])
    .layout(LayoutStyle::new().padding(Insets::all(1)))
    .child_offset(Point::new(1, -1));
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(16, 12), WidthPolicy::Cjk);
    let prepared = tree.prepare(&view, Size::new(16, 12), &mut renderer)?;
    let cursor = prepared.patch().cursor;
    assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));
    let root = tree.root().expect("root exists");
    let input = tree.nodes[&root].children[0];
    let child = tree.nodes[&input].children[0];
    assert_eq!(tree.nodes[&child].content, Rect::new(6, 5, 3, 1));
    assert_eq!(
        cursor,
        CursorState::visible(Point::new(9, 5))
            .with_shape(CursorShape::Bar)
            .with_blinking(true)
    );
    Ok(())
}

#[test]
fn child_cursor_can_use_spare_cell_beyond_leaf_and_empty_child() -> Result<(), UiError> {
    for text in ["abc", ""] {
        let width = i32::try_from(text.len()).expect("short text");
        let view = Element::<()>::container([Element::text(text)])
            .layout(LayoutStyle {
                align: Align::Start,
                ..LayoutStyle::new().size(Dimension::cells(8), Dimension::cells(3))
            })
            .focusable(true)
            .cursor_with_child(0, 0, move |_, _| CursorState::visible(Point::new(width, 0)));
        let tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(8, 3), WidthPolicy::Unicode);
        let prepared = tree.prepare(&view, Size::new(8, 3), &mut renderer)?;
        assert_eq!(
            prepared.patch().cursor,
            CursorState::visible(Point::new(width, 0))
        );
    }
    Ok(())
}

#[test]
fn cursor_anchor_index_presence_and_legacy_setters_invalidate_at_same_fingerprint()
-> Result<(), UiError> {
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(8, 3), WidthPolicy::Unicode);
    for (anchor, expected) in [(Some(0), 1), (Some(1), 3), (None, 0), (Some(0), 1)] {
        let view = Element::<()>::container([Element::text("ab"), Element::text("cd")])
            .layout(
                LayoutStyle::new()
                    .size(Dimension::cells(8), Dimension::cells(3))
                    .padding(Insets::all(1)),
            )
            .focusable(true);
        let view = match anchor {
            Some(index) => {
                view.cursor_with_child(index, 0, |_, _| CursorState::visible(Point::ORIGIN))
            }
            None => view
                .cursor_with_child(99, 0, |_, _| panic!("replaced callback"))
                .cursor_with(0, |_, size| {
                    assert_eq!(size, Size::new(8, 3));
                    CursorState::visible(Point::ORIGIN)
                }),
        };
        tree.reconcile(&view)?;
        assert!(tree.pending_invalidation() >= Invalidation::Paint);
        let prepared = tree.prepare(&view, Size::new(8, 3), &mut renderer)?;
        assert_eq!(
            prepared.patch().cursor.position,
            Point::new(expected, i32::from(anchor.is_some()))
        );
        assert_eq!(
            prepared.patch().cursor.visibility,
            CursorVisibility::Visible
        );
        assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));
    }
    let view = Element::<()>::container([])
        .focusable(true)
        .cursor_with_child(99, 0, |_, _| panic!("replaced callback"))
        .cursor(CursorState::visible(Point::ORIGIN));
    let prepared = tree.prepare(&view, Size::new(8, 3), &mut renderer)?;
    assert_eq!(prepared.patch().cursor, CursorState::visible(Point::ORIGIN));
    Ok(())
}

#[test]
fn missing_cursor_child_hides_without_invoking_callback() -> Result<(), UiError> {
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(8, 1), WidthPolicy::Unicode);
    for present in [false, true, false] {
        let view = Element::<()>::container(present.then(|| Element::text("abc")))
            .focusable(true)
            .cursor_with_child(0, 0, |_, _| {
                assert!(present, "missing child must not call the callback");
                CursorState::visible(Point::ORIGIN)
            });
        let prepared = tree.prepare(&view, Size::new(8, 1), &mut renderer)?;
        assert_eq!(
            prepared.patch().cursor,
            if present {
                CursorState::visible(Point::ORIGIN)
            } else {
                CursorState::HIDDEN
            }
        );
        assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));
    }
    Ok(())
}

#[test]
fn child_cursor_clips_to_owner_content_ancestors_and_viewport() -> Result<(), UiError> {
    // Each case clips an otherwise valid caret at a different boundary.
    for (owner_width, parent_width, viewport_width, cursor_x, visible) in [
        (8, 12, 14, 5, true),
        (8, 12, 14, 6, false),
        (12, 8, 14, 5, false),
        (12, 14, 6, 4, false),
        (2, 12, 14, 0, false),
        (12, 14, 0, 0, false),
    ] {
        let view = Element::<()>::container([Element::container([Element::text("a")])
            .layout(
                LayoutStyle::new()
                    .size(Dimension::cells(owner_width), Dimension::cells(3))
                    .padding(Insets::all(1))
                    .flex(0, 0),
            )
            .focusable(true)
            .cursor_with_child(0, 0, move |_, _| {
                CursorState::visible(Point::new(cursor_x, 0))
            })])
        .layout(
            LayoutStyle::new()
                .size(Dimension::cells(parent_width), Dimension::cells(5))
                .padding(Insets::all(1)),
        );
        let tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(viewport_width, 5), WidthPolicy::Unicode);
        let prepared = tree.prepare(&view, Size::new(viewport_width, 5), &mut renderer)?;
        assert_eq!(
            prepared.patch().cursor.visibility == CursorVisibility::Visible,
            visible,
            "{owner_width}/{parent_width}/{viewport_width}/{cursor_x}"
        );
    }
    Ok(())
}

#[test]
fn child_offset_geometry_is_local_unshifted_and_recomputed() -> Result<(), UiError> {
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(16, 5), WidthPolicy::Unicode);
    for anchor in [Some(0), Some(1), None, Some(0), Some(99)] {
        let input = Element::<()>::container([Element::text("ab"), Element::text("cd")])
            .layout(
                LayoutStyle::new()
                    .size(Dimension::cells(8), Dimension::cells(3))
                    .padding(Insets::all(1)),
            )
            .focusable(true)
            .cursor_with_child(0, 0, |_, _| CursorState::visible(Point::ORIGIN));
        let input = if let Some(index) = anchor {
            input.child_offset_with_child(index, 0, move |size, policy, child| {
                assert_eq!(size, Size::new(6, 1));
                assert_eq!(policy, WidthPolicy::Unicode);
                assert_eq!(child, Rect::new(if index == 0 { 0 } else { 2 }, 0, 2, 1));
                Point::new(2, 0)
            })
        } else {
            input
                .child_offset_with_child(99, 0, |_, _, _| panic!("replaced callback"))
                .child_offset_with(0, |size, _| {
                    assert_eq!(size, Size::new(8, 3));
                    Point::ORIGIN
                })
        };
        let view = Element::container([input]).child_offset(Point::new(3, 1));
        tree.reconcile(&view)?;
        assert!(tree.pending_invalidation() >= Invalidation::Layout);
        let prepared = tree.prepare(&view, Size::new(16, 5), &mut renderer)?;
        let x = if matches!(anchor, Some(0 | 1)) { 6 } else { 4 };
        assert_eq!(
            prepared.patch().cursor,
            CursorState::visible(Point::new(x, 2))
        );
        assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));
    }
    let view = Element::<()>::container([Element::text("a")])
        .focusable(true)
        .cursor_with_child(0, 0, |_, _| CursorState::visible(Point::ORIGIN))
        .child_offset_with_child(99, 0, |_, _, _| panic!("replaced callback"))
        .child_offset(Point::new(1, 0));
    let prepared = tree.prepare(&view, Size::new(16, 5), &mut renderer)?;
    assert_eq!(
        prepared.patch().cursor,
        CursorState::visible(Point::new(1, 0))
    );
    Ok(())
}

#[test]
fn child_cursor_can_anchor_a_text_group_without_moving_focus() -> Result<(), UiError> {
    let view = Element::<()>::container([
        Element::container([Element::text("a"), Element::text("b"), Element::text("c")])
            .layout(LayoutStyle::new().direction(FlexDirection::Row)),
        Element::container([])
            .layout(LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(1))),
    ])
    .layout(LayoutStyle {
        justify: Justify::Center,
        ..LayoutStyle::new().size(Dimension::cells(8), Dimension::cells(1))
    })
    .focusable(true)
    .cursor_with_child(0, 0, |_, size| {
        assert_eq!(size, Size::new(3, 1));
        CursorState::visible(Point::new(3, 0))
    });
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(8, 1), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(8, 1), &mut renderer)?;
    assert_eq!(
        prepared.patch().cursor,
        CursorState::visible(Point::new(5, 0))
    );
    assert_eq!(tree.commit(prepared, &mut renderer), Ok(()));
    assert_eq!(tree.focused(), tree.root());
    Ok(())
}
