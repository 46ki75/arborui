//! Facade-only access to child-anchored cursor and scroll geometry.

use arborui::{
    CursorState, Dimension, Element, LayoutStyle, Point, Renderer, Size, UiTree, WidthPolicy,
};

#[test]
fn facade_exposes_borrowing_child_geometry_callbacks() -> Result<(), Box<dyn std::error::Error>> {
    let text = String::from("abc");
    let prefix = 3;
    let view = Element::<()>::container([Element::text(&text)])
        .layout(LayoutStyle::new().size(Dimension::cells(8), Dimension::cells(1)))
        .focusable(true)
        .cursor_with_child(0, 0, |_, size| {
            assert_eq!(size, Size::new(3, 1));
            CursorState::visible(Point::new(prefix, 0))
        })
        .child_offset_with_child(0, 0, |size, _, child| {
            assert_eq!(size, Size::new(8, 1));
            assert_eq!(child.size(), Size::new(3, 1));
            Point::new(1, 0)
        });
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(8, 1), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(8, 1), &mut renderer)?;
    assert_eq!(
        prepared.patch().cursor,
        CursorState::visible(Point::new(4, 0))
    );
    tree.commit(prepared, &mut renderer)?;
    drop(view);
    drop(text);
    assert!(tree.root().is_some());
    Ok(())
}
