use arborui_render::CellContent;

use super::*;

fn empty_block(title: &str, size: Size) -> Element<'_, ()> {
    let child = Element::container([])
        .layout(LayoutStyle::new().size(Dimension::cells(0), Dimension::cells(0)));
    Block::new(child)
        .title(title)
        .border(crate::BorderSet::Ascii)
        .layout(
            LayoutStyle::new()
                .size(Dimension::cells(size.width), Dimension::cells(size.height))
                .flex(0, 0),
        )
        .build()
}

fn character_rows(patch: &FramePatch) -> Vec<String> {
    assert!(patch.full_repaint);
    (0..i32::from(patch.size.height))
        .map(|y| {
            patch
                .runs
                .iter()
                .filter(|run| run.position.y == y)
                .flat_map(|run| &run.cells)
                .map(|cell| match &cell.content {
                    PatchCellContent::Grapheme { text, .. } => text.as_ref(),
                    PatchCellContent::Empty => " ",
                    PatchCellContent::Continuation { .. } => "",
                })
                .collect()
        })
        .collect()
}

#[test]
fn multiline_block_title_stays_in_top_border() -> Result<(), Box<dyn Error>> {
    // A nonempty child could erase the title's accidental interior writes.
    let child = Element::<()>::container([])
        .layout(LayoutStyle::new().size(Dimension::cells(0), Dimension::cells(0)));
    let view = Block::new(child)
        .title("\nXYZ")
        .layout(LayoutStyle::new().size(Dimension::cells(7), Dimension::cells(3)))
        .build();
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(7, 3), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(7, 3), &mut renderer)?;

    for x in 1..6 {
        assert_eq!(
            prepared
                .buffer()
                .get(Point::new(x, 1))
                .map(|cell| cell.content),
            Some(CellContent::Empty),
            "title painted interior column {x}"
        );
    }
    for (point, expected) in [
        (Point::new(0, 0), "\u{250c}"),
        (Point::new(6, 0), "\u{2510}"),
        (Point::new(0, 2), "\u{2514}"),
        (Point::new(6, 2), "\u{2518}"),
    ] {
        assert_eq!(patch_grapheme(prepared.patch(), point), Some(expected));
    }
    assert_eq!(
        character_rows(prepared.patch()),
        [
            "\u{250c}  \u{2500}\u{2500}\u{2500}\u{2510}",
            "\u{2502}     \u{2502}",
            "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
        ]
    );
    tree.commit(prepared, &mut renderer)?;
    let root = tree
        .node(tree.root().ok_or("missing block root")?)
        .ok_or("missing block")?;
    let child = tree.node(root.children()[0]).ok_or("missing child")?;
    assert_eq!(root.layout().size(), Size::new(7, 3));
    assert_eq!(child.layout().size(), Size::new(0, 0));
    Ok(())
}

#[test]
fn titles_use_only_the_first_logical_line() -> Result<(), Box<dyn Error>> {
    let size = Size::new(7, 3);
    for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
        for separator in [
            "\n", "\u{b}", "\u{c}", "\r", "\u{85}", "\u{2028}", "\u{2029}", "\r\n",
        ] {
            for (prefix, top) in [("", "+  ---+"), ("T", "+ T --+")] {
                let title = format!("{prefix}{separator}XYZ");
                let view = empty_block(&title, size);
                let tree = UiTree::new();
                let mut renderer = Renderer::new(size, policy);
                let prepared = tree.prepare(&view, size, &mut renderer)?;

                assert_eq!(
                    character_rows(prepared.patch()),
                    [top, "|     |", "+-----+"],
                    "title {title:?}, policy {policy:?}"
                );
                for x in 1..6 {
                    assert_eq!(
                        prepared
                            .buffer()
                            .get(Point::new(x, 1))
                            .map(|cell| cell.content),
                        Some(CellContent::Empty)
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn title_fitting_preserves_whole_graphemes_under_each_width_policy() -> Result<(), Box<dyn Error>> {
    for (title, widths) in [
        ("\u{754c}", [2, 2, 2]),
        ("e\u{301}", [1, 1, 1]),
        ("\u{b7}", [1, 2, 1]),
        (
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}",
            [2, 2, 8],
        ),
    ] {
        for (policy, width) in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth]
            .into_iter()
            .zip(widths)
        {
            for (available, expected) in [
                (width + 2, format!("+ {title} +")),
                (width + 1, format!("+ {title}+")),
                (width, format!("+ {}+", "-".repeat(usize::from(width - 1)))),
            ] {
                let size = Size::new(available + 2, 3);
                let title = format!("{title}\nXYZ");
                // ASCII borders isolate title fitting from ambiguous border glyph widths.
                let view = empty_block(&title, size);
                let tree = UiTree::new();
                let mut renderer = Renderer::new(size, policy);
                let prepared = tree.prepare(&view, size, &mut renderer)?;

                assert_eq!(
                    character_rows(prepared.patch()),
                    [
                        expected,
                        format!("|{}|", " ".repeat(usize::from(available))),
                        format!("+{}+", "-".repeat(usize::from(available))),
                    ],
                    "title {title:?}, policy {policy:?}, available {available}"
                );
                assert_eq!(patch_grapheme(prepared.patch(), Point::ORIGIN), Some("+"));
                assert_eq!(
                    patch_grapheme(prepared.patch(), Point::new(i32::from(size.width) - 1, 0)),
                    Some("+")
                );
            }
        }
    }
    Ok(())
}

#[test]
fn titles_leave_corners_intact_in_tiny_viewports() -> Result<(), Box<dyn Error>> {
    for width in 0..=2 {
        for height in 0..=3 {
            let size = Size::new(width, height);
            let view = empty_block("\u{301}\nXYZ", size);
            let tree = UiTree::new();
            let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
            let prepared = tree.prepare(&view, size, &mut renderer)?;

            assert_eq!(prepared.buffer().size(), size);
            for y in 0..i32::from(height) {
                for x in 0..i32::from(width) {
                    let expected = if y == 0 || y == i32::from(height) - 1 {
                        "+"
                    } else {
                        "|"
                    };
                    assert_eq!(
                        patch_grapheme(prepared.patch(), Point::new(x, y)),
                        Some(expected),
                        "viewport {size:?}, point ({x}, {y})"
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn title_clip_uses_translated_coordinates_and_inherited_scope() -> Result<(), Box<dyn Error>> {
    let size = Size::new(12, 6);
    for title in ["ABC", "ABC\nXYZ"] {
        for offset_y in [0, -1] {
            let block = empty_block(title, Size::new(7, 3));
            let parent = Element::container([block])
                .layout(
                    LayoutStyle::new()
                        .size(Dimension::cells(3), Dimension::cells(3))
                        .flex(0, 0),
                )
                .child_offset(Point::new(-1, offset_y));
            let view = Element::container([parent]).child_offset(Point::new(3, 2));
            let mut tree = UiTree::new();
            let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
            let prepared = tree.prepare(&view, size, &mut renderer)?;

            let expected = if offset_y == 0 {
                [
                    "            ",
                    "            ",
                    "    AB      ",
                    "            ",
                    "   ---      ",
                    "            ",
                ]
            } else {
                [
                    "            ",
                    "            ",
                    "            ",
                    "   ---      ",
                    "            ",
                    "            ",
                ]
            };
            assert_eq!(
                character_rows(prepared.patch()),
                expected,
                "title {title:?}, offset {offset_y}"
            );
            tree.commit(prepared, &mut renderer)?;
            let root = tree
                .node(tree.root().ok_or("missing root")?)
                .ok_or("missing root node")?;
            let parent = tree.node(root.children()[0]).ok_or("missing parent")?;
            let block = tree.node(parent.children()[0]).ok_or("missing block")?;
            assert_eq!(parent.layout(), arborui_core::Rect::new(3, 2, 3, 3));
            assert_eq!(
                block.layout(),
                arborui_core::Rect::new(2, 2 + offset_y, 7, 3)
            );
        }
    }
    Ok(())
}

#[test]
fn multiline_title_preserves_nonempty_child_content() -> Result<(), Box<dyn Error>> {
    let size = Size::new(7, 3);
    let view = Block::new(Element::<()>::text("hello"))
        .title("T\nXYZ")
        .border(crate::BorderSet::Ascii)
        .layout(LayoutStyle::new().size(Dimension::cells(7), Dimension::cells(3)))
        .build();
    let tree = UiTree::new();
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, size, &mut renderer)?;

    assert_eq!(
        character_rows(prepared.patch()),
        ["+ T --+", "|hello|", "+-----+"]
    );
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(1, 1)),
        Some("h")
    );
    Ok(())
}
