use super::*;
use arborui_core::{CursorShape, Insets};
use arborui_layout::{Align, FlexDirection, Justify};
use arborui_text::{graphemes, measure};

#[test]
fn horizontally_centered_input_cursor_tracks_text() -> Result<(), Box<dyn Error>> {
    let buffer = TextBuffer::new("abc");
    let view = TextInput::new(&buffer, |updated| updated)
        .layout(LayoutStyle {
            justify: Justify::Center,
            ..LayoutStyle::new().size(Dimension::cells(8), Dimension::cells(1))
        })
        .build();
    let tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(8, 1), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(8, 1), &mut renderer)?;

    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(2, 0)),
        Some("a")
    );
    assert_eq!(
        prepared.patch().cursor.visibility,
        CursorVisibility::Visible
    );
    assert_eq!(prepared.patch().cursor.position, Point::new(5, 0));
    Ok(())
}

#[test]
fn vertically_centered_input_cursor_tracks_text() -> Result<(), Box<dyn Error>> {
    let buffer = TextBuffer::new("abc");
    let view = TextInput::new(&buffer, |updated| updated)
        .layout(LayoutStyle {
            align: Align::Center,
            ..LayoutStyle::new().size(Dimension::cells(8), Dimension::cells(3))
        })
        .build();
    let tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(8, 3), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(8, 3), &mut renderer)?;

    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(0, 1)),
        Some("a")
    );
    assert_eq!(
        prepared.patch().cursor.visibility,
        CursorVisibility::Visible
    );
    assert_eq!(prepared.patch().cursor.position, Point::new(3, 1));
    Ok(())
}

#[test]
fn aligned_overflow_home_cursor_tracks_visible_text() -> Result<(), Box<dyn Error>> {
    for justify in [Justify::Center, Justify::End] {
        let mut buffer = TextBuffer::new("abcdefghij");
        buffer.apply(TextEdit::Move {
            movement: TextMovement::Home,
            extend_selection: false,
        });
        let view = TextInput::new(&buffer, |updated| updated)
            .layout(LayoutStyle {
                justify,
                ..LayoutStyle::new().size(Dimension::cells(4), Dimension::cells(1))
            })
            .build();
        let tree = UiTree::new();
        let mut renderer = Renderer::new(Size::new(4, 1), WidthPolicy::Unicode);
        let prepared = tree.prepare(&view, Size::new(4, 1), &mut renderer)?;

        assert_eq!(
            prepared.patch().cursor.visibility,
            CursorVisibility::Visible,
            "{justify:?}"
        );
        assert_eq!(
            prepared.patch().cursor.position,
            Point::ORIGIN,
            "{justify:?}"
        );
        assert_eq!(
            patch_grapheme(prepared.patch(), Point::ORIGIN),
            Some("a"),
            "{justify:?}"
        );
    }
    Ok(())
}

fn assert_input_geometry(
    buffer: &TextBuffer,
    layout: LayoutStyle,
    policy: WidthPolicy,
) -> Result<(), Box<dyn Error>> {
    let viewport = Size::new(40, 14);
    let view = |scroll| {
        let input = TextInput::new(buffer, |updated| updated)
            .layout(layout)
            .build()
            .key("input");
        let input = if scroll {
            input
        } else {
            input.child_offset(Point::ORIGIN)
        };
        Element::container([input])
            .layout(LayoutStyle::new().padding(Insets::all(1)))
            .child_offset(Point::new(2, 1))
    };
    let mut natural_tree = UiTree::new();
    let mut natural_renderer = Renderer::new(viewport, policy);
    prepare_and_commit(
        &mut natural_tree,
        &view(false),
        viewport,
        &mut natural_renderer,
    )?;
    let parent = natural_tree.root().ok_or("missing parent")?;
    let input = natural_tree
        .node(parent)
        .ok_or("missing parent")?
        .children()[0];
    let input_node = natural_tree.node(input).ok_or("missing input")?;
    let text = input_node.children()[0];
    let natural_text = natural_tree.node(text).ok_or("missing text")?.content();
    let content = input_node.content();
    let prefix = i32::try_from(measure(&buffer.text()[..buffer.cursor().get()], policy).width)?;
    let natural_cursor = natural_text.origin().translated(prefix, 0);
    let expected = Point::new(
        natural_cursor.x.clamp(content.x, content.right() - 1),
        natural_cursor.y.clamp(content.y, content.bottom() - 1),
    );

    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(viewport, policy);
    let prepared = tree.prepare(&view(true), viewport, &mut renderer)?;
    let patch = prepared.patch().clone();
    tree.commit(prepared, &mut renderer)?;
    let parent = tree.root().ok_or("missing parent")?;
    let input = tree.node(parent).ok_or("missing parent")?.children()[0];
    let input_node = tree.node(input).ok_or("missing input")?;
    let text_node = tree.node(input_node.children()[0]).ok_or("missing text")?;
    let text_origin = text_node.content().origin();
    let context = format!(
        "{layout:?}, {policy:?}, {:?}, cursor {}",
        buffer.text(),
        buffer.cursor().get()
    );
    assert_eq!(
        patch.cursor.visibility,
        CursorVisibility::Visible,
        "{context}"
    );
    assert_eq!(patch.cursor.shape, CursorShape::Bar, "{context}");
    assert_eq!(patch.cursor.position, expected, "{context}");
    assert_eq!(
        patch.cursor.position,
        text_origin.translated(prefix, 0),
        "{context}"
    );
    assert_eq!(text_node.content().size(), natural_text.size(), "{context}");
    assert_eq!(tree.focused(), Some(input), "{context}");
    assert_eq!(input_node.key(), Some(&Key::from("input")));
    assert!(!text_node.is_focusable());

    let mut x = text_origin.x;
    for grapheme in graphemes(buffer.text(), policy) {
        let width = i32::try_from(grapheme.width)?;
        let point = Point::new(x, text_origin.y);
        // Column stretching can constrain the text's own paint clip before scrolling.
        if width > 0
            && content.contains(point)
            && content.contains(point.translated(width - 1, 0))
            && text_node.content().contains(point)
            && text_node.content().contains(point.translated(width - 1, 0))
        {
            assert_eq!(
                patch_grapheme(&patch, point),
                Some(grapheme.text),
                "{context}: {point:?}"
            );
        }
        x += width;
    }
    Ok(())
}

#[test]
fn input_cursor_preserves_all_alignments_directions_spacing_and_insets()
-> Result<(), Box<dyn Error>> {
    for direction in [
        FlexDirection::Row,
        FlexDirection::RowReverse,
        FlexDirection::Column,
        FlexDirection::ColumnReverse,
    ] {
        for align in [Align::Start, Align::Center, Align::End, Align::Stretch] {
            for justify in [
                Justify::Start,
                Justify::Center,
                Justify::End,
                Justify::SpaceBetween,
                Justify::SpaceAround,
                Justify::SpaceEvenly,
            ] {
                for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
                    for (text, size) in [
                        ("", None),
                        ("abc", None),
                        ("abc", Some(Size::new(16, 5))),
                        ("abcdefghij", Some(Size::new(4, 1))),
                        ("a\u{b7}\u{1f469}\u{200d}\u{1f4bb}z", Some(Size::new(5, 3))),
                    ] {
                        let mut layout = LayoutStyle {
                            direction,
                            align,
                            justify,
                            gap: 1,
                            ..LayoutStyle::new()
                                .border(Insets::all(1))
                                .padding(Insets::all(1))
                        };
                        if let Some(size) = size {
                            layout = layout.size(
                                Dimension::cells(size.width + 4),
                                Dimension::cells(size.height + 4),
                            );
                        }
                        for movement in [TextMovement::Home, TextMovement::Right, TextMovement::End]
                        {
                            let mut buffer = TextBuffer::new(text);
                            buffer.apply(TextEdit::Move {
                                movement: TextMovement::Home,
                                extend_selection: false,
                            });
                            buffer.apply(TextEdit::Move {
                                movement,
                                extend_selection: false,
                            });
                            assert_input_geometry(&buffer, layout, policy)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn aligned_input_incremental_frames_match_full_layout() -> Result<(), Box<dyn Error>> {
    let mut tree = UiTree::new();
    let mut reference_tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(8, 3), WidthPolicy::Unicode);
    let mut reference_renderer = Renderer::new(Size::new(8, 3), WidthPolicy::Unicode);
    let mut buffer = TextBuffer::new("a\u{b7}\u{1f469}\u{200d}\u{1f4bb}z");
    let mut identity = None;
    for (size, direction, justify, align, policy, movement) in [
        (
            Size::new(12, 3),
            FlexDirection::Row,
            Justify::Start,
            Align::Start,
            WidthPolicy::Unicode,
            TextMovement::End,
        ),
        (
            Size::new(12, 3),
            FlexDirection::Row,
            Justify::Center,
            Align::Center,
            WidthPolicy::Unicode,
            TextMovement::End,
        ),
        (
            Size::new(5, 3),
            FlexDirection::Row,
            Justify::Center,
            Align::Center,
            WidthPolicy::Unicode,
            TextMovement::End,
        ),
        (
            Size::new(5, 3),
            FlexDirection::Row,
            Justify::Center,
            Align::Center,
            WidthPolicy::Unicode,
            TextMovement::Home,
        ),
        (
            Size::new(5, 3),
            FlexDirection::Row,
            Justify::Center,
            Align::Center,
            WidthPolicy::Cjk,
            TextMovement::End,
        ),
        (
            Size::new(5, 3),
            FlexDirection::Row,
            Justify::Center,
            Align::Center,
            WidthPolicy::WcWidth,
            TextMovement::End,
        ),
        (
            Size::new(5, 3),
            FlexDirection::ColumnReverse,
            Justify::End,
            Align::End,
            WidthPolicy::WcWidth,
            TextMovement::Home,
        ),
        (
            Size::new(5, 3),
            FlexDirection::ColumnReverse,
            Justify::End,
            Align::End,
            WidthPolicy::WcWidth,
            TextMovement::Right,
        ),
        (
            Size::new(12, 3),
            FlexDirection::RowReverse,
            Justify::SpaceEvenly,
            Align::Stretch,
            WidthPolicy::Unicode,
            TextMovement::End,
        ),
    ] {
        buffer.apply(TextEdit::Move {
            movement,
            extend_selection: false,
        });
        renderer.set_width_policy(policy);
        reference_renderer.set_width_policy(policy);
        let view = TextInput::new(&buffer, |updated| updated)
            .layout(LayoutStyle {
                direction,
                justify,
                align,
                ..LayoutStyle::new()
            })
            .build()
            .key("input");
        // Also exercise the unchanged-frame path for each transition.
        for _ in 0..2 {
            let prepared = tree.prepare(&view, size, &mut renderer)?;
            let reference = reference_tree.prepare_full(&view, size, &mut reference_renderer)?;
            assert_eq!(prepared.patch(), reference.patch());
            assert_eq!(prepared.buffer(), reference.buffer());
            assert_eq!(prepared.hit_map(), reference.hit_map());
            assert_eq!(
                prepared.patch().cursor.visibility,
                CursorVisibility::Visible
            );
            let cursor = prepared.patch().cursor.position;
            tree.commit(prepared, &mut renderer)?;
            reference_tree.commit(reference, &mut reference_renderer)?;
            let input = tree.root().ok_or("missing input")?;
            assert_eq!(input, *identity.get_or_insert(input));
            assert_eq!(tree.focused(), Some(input));
            let text = tree.node(input).ok_or("missing input")?.children()[0];
            let prefix =
                i32::try_from(measure(&buffer.text()[..buffer.cursor().get()], policy).width)?;
            assert_eq!(
                cursor,
                tree.node(text)
                    .ok_or("missing text")?
                    .content()
                    .origin()
                    .translated(prefix, 0)
            );
        }
    }
    Ok(())
}

#[test]
fn aligned_input_keeps_root_keyboard_target_and_focus_order() -> Result<(), Box<dyn Error>> {
    let buffer = TextBuffer::new("abc");
    let view = Element::container([
        Button::new("next", || TextBuffer::new("button"))
            .build()
            .key("button")
            .focus_order(2),
        TextInput::new(&buffer, |updated| updated)
            .focus_order(-1)
            .layout(LayoutStyle {
                justify: Justify::Center,
                align: Align::Center,
                ..LayoutStyle::new().size(Dimension::cells(8), Dimension::cells(3))
            })
            .build()
            .key("input"),
    ]);
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(16, 3), WidthPolicy::Unicode);
    prepare_and_commit(&mut tree, &view, Size::new(16, 3), &mut renderer)?;
    let input = tree.focused().ok_or("missing focus")?;
    assert_eq!(
        tree.node(input).and_then(|node| node.key()),
        Some(&Key::from("input"))
    );
    assert_eq!(
        tree.node(input).ok_or("missing input")?.kind(),
        arborui_ui::WidgetKind::Custom("text-input")
    );
    let home = tree.dispatch(&view, &key(UiKey::Home, KeyModifiers::NONE), &renderer)?;
    assert!(home.handled);
    assert_eq!(
        home.messages.first().map(|buffer| buffer.cursor().get()),
        Some(0)
    );
    let typed = tree.dispatch(
        &view,
        &key(UiKey::Character('d'), KeyModifiers::NONE),
        &renderer,
    )?;
    assert_eq!(typed.messages.first().map(TextBuffer::text), Some("abcd"));
    let _ = tree.dispatch(&view, &key(UiKey::Tab, KeyModifiers::NONE), &renderer)?;
    assert_eq!(
        tree.focused()
            .and_then(|id| tree.node(id))
            .and_then(|node| node.key()),
        Some(&Key::from("button"))
    );
    let _ = tree.dispatch(&view, &key(UiKey::Tab, KeyModifiers::SHIFT), &renderer)?;
    assert_eq!(tree.focused(), Some(input));
    Ok(())
}
