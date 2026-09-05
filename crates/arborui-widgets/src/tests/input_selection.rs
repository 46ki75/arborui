use super::*;
use arborui_layout::{Align, FlexDirection, Justify};
use arborui_render::CellContent;

#[test]
fn selected_text_has_a_visible_indication() -> Result<(), Box<dyn Error>> {
    let size = Size::new(6, 1);
    let buffer = TextBuffer::new("abc");
    let view = TextInput::new(&buffer, |updated| updated).build();
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let initial = tree.prepare(&view, size, &mut renderer)?;
    let cursor = initial.patch().cursor;
    let normal = initial.buffer().clone();
    tree.commit(initial, &mut renderer)?;
    let root = tree.root().ok_or("missing input")?;
    let children = tree.node(root).ok_or("missing input")?.children().to_vec();
    let spans = tree
        .node(children[0])
        .ok_or("missing text group")?
        .children()
        .to_vec();

    let outcome = tree.dispatch(
        &view,
        &key(UiKey::Character('a'), KeyModifiers::CONTROL),
        &renderer,
    )?;
    assert!(outcome.handled);
    assert_eq!(outcome.messages.len(), 1);
    let selected = outcome
        .messages
        .into_iter()
        .next()
        .ok_or("missing update")?;
    assert_eq!(selected.text(), buffer.text());
    assert_eq!(selected.cursor(), buffer.cursor());
    assert_eq!(selected.selection().map(|s| s.byte_range()), Some(0..3));
    assert_eq!(buffer, TextBuffer::new("abc"));
    let unchanged = tree.prepare(&view, size, &mut renderer)?;
    assert!(
        unchanged.patch().is_empty(),
        "the application must adopt the update"
    );
    tree.discard(unchanged, &mut renderer);

    let selected_view = TextInput::new(&selected, |updated| updated).build();
    let highlighted = tree.prepare(&selected_view, size, &mut renderer)?;
    assert!(
        !highlighted.patch().runs.is_empty(),
        "Ctrl+A must repaint selection even when text and cursor are unchanged"
    );
    assert_eq!(highlighted.patch().cursor, cursor);
    for (x, cell) in highlighted.buffer().cells().iter().enumerate() {
        assert_eq!(cell.content, normal.cells()[x].content);
        assert_eq!(cell.style.modifiers.contains(Modifier::REVERSED), x < 3);
    }
    let reference = tree.prepare_full(&selected_view, size, &mut renderer)?;
    assert_eq!(highlighted.buffer(), reference.buffer());
    assert_eq!(highlighted.patch(), reference.patch());
    tree.discard(reference, &mut renderer);
    tree.commit(highlighted, &mut renderer)?;
    assert_eq!(tree.node(root).ok_or("missing input")?.children(), children);
    assert_eq!(
        tree.node(children[0])
            .ok_or("missing text group")?
            .children(),
        spans
    );
    let settled = tree.prepare(&selected_view, size, &mut renderer)?;
    assert!(settled.patch().is_empty());
    tree.discard(settled, &mut renderer);

    let outcome = tree.dispatch(
        &selected_view,
        &key(UiKey::End, KeyModifiers::NONE),
        &renderer,
    )?;
    let collapsed = outcome
        .messages
        .into_iter()
        .next()
        .ok_or("missing collapse")?;
    assert_eq!(collapsed.cursor(), selected.cursor());
    assert_eq!(collapsed.selection(), None);
    assert_eq!(selected.selection().map(|s| s.byte_range()), Some(0..3));
    let collapsed_view = TextInput::new(&collapsed, |updated| updated).build();
    let restored = tree.prepare(&collapsed_view, size, &mut renderer)?;
    assert!(!restored.patch().runs.is_empty());
    assert_eq!(restored.patch().cursor, cursor);
    assert_eq!(restored.buffer(), &normal);
    let reference = tree.prepare_full(&collapsed_view, size, &mut renderer)?;
    assert_eq!(restored.buffer(), reference.buffer());
    assert_eq!(restored.patch(), reference.patch());
    tree.discard(reference, &mut renderer);
    tree.commit(restored, &mut renderer)?;
    assert_eq!(tree.node(root).ok_or("missing input")?.children(), children);
    assert_eq!(
        tree.node(children[0])
            .ok_or("missing text group")?
            .children(),
        spans
    );
    let settled = tree.prepare(&collapsed_view, size, &mut renderer)?;
    assert!(settled.patch().is_empty());
    tree.discard(settled, &mut renderer);
    Ok(())
}

#[test]
fn shift_selection_preserves_unicode_graphemes_in_both_directions() -> Result<(), Box<dyn Error>> {
    let clusters = [
        "L",
        "a\u{301}",
        "\u{1f469}\u{200d}\u{1f4bb}",
        "\u{754c}",
        "\u{b7}",
        "R",
    ];
    for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
        let widths = [
            1,
            1,
            if policy == WidthPolicy::WcWidth { 4 } else { 2 },
            2,
            if policy == WidthPolicy::Cjk { 2 } else { 1 },
            1,
        ];
        for forward in [true, false] {
            let size = Size::new(12, 2);
            let mut buffer = TextBuffer::new(clusters.concat());
            buffer.apply(TextEdit::Move {
                movement: if forward {
                    TextMovement::Home
                } else {
                    TextMovement::End
                },
                extend_selection: false,
            });
            buffer.apply(TextEdit::Move {
                movement: if forward {
                    TextMovement::Right
                } else {
                    TextMovement::Left
                },
                extend_selection: false,
            });
            let anchor = buffer.cursor();
            let mut tree = UiTree::new();
            let mut renderer = Renderer::new(size, policy);
            let view = TextInput::new(&buffer, |updated| updated).build();
            let initial = tree.prepare(&view, size, &mut renderer)?;
            let normal = initial.buffer().clone();
            let mut x = 0;
            for (cluster, width) in clusters.iter().zip(widths) {
                assert_eq!(
                    patch_grapheme(initial.patch(), Point::new(x, 0)),
                    Some(*cluster)
                );
                for offset in 1..width {
                    assert!(matches!(
                        normal
                            .get(Point::new(x + offset, 0))
                            .ok_or("missing continuation")?
                            .content,
                        CellContent::Continuation { offset: actual, .. } if i32::from(actual) == offset
                    ));
                }
                x += width;
            }
            tree.commit(initial, &mut renderer)?;
            drop(view);

            for step in 1..=4 {
                let original = buffer.clone();
                let view = TextInput::new(&buffer, |updated| updated).build();
                let outcome = tree.dispatch(
                    &view,
                    &key(
                        if forward { UiKey::Right } else { UiKey::Left },
                        KeyModifiers::SHIFT,
                    ),
                    &renderer,
                )?;
                assert!(outcome.handled);
                assert_eq!(outcome.messages.len(), 1);
                assert_eq!(buffer, original);
                drop(view);
                buffer = outcome
                    .messages
                    .into_iter()
                    .next()
                    .ok_or("missing selection update")?;
                let range = if forward { 1..1 + step } else { 5 - step..5 };
                let byte_start: usize = clusters[..range.start].iter().map(|s| s.len()).sum();
                let byte_end: usize = clusters[..range.end].iter().map(|s| s.len()).sum();
                let selection = buffer.selection().ok_or("missing selection")?;
                assert_eq!(selection.anchor(), anchor);
                assert_eq!(selection.byte_range(), byte_start..byte_end);
                let columns =
                    widths[..range.start].iter().sum::<i32>()..widths[..range.end].iter().sum();
                let view = TextInput::new(&buffer, |updated| updated).build();
                let frame = tree.prepare(&view, size, &mut renderer)?;
                assert!(!frame.patch().runs.is_empty());
                for y in 0..i32::from(size.height) {
                    for x in 0..i32::from(size.width) {
                        let point = Point::new(x, y);
                        let cell = frame.buffer().get(point).ok_or("missing cell")?;
                        assert_eq!(
                            cell.content,
                            normal.get(point).ok_or("missing normal cell")?.content
                        );
                        assert_eq!(
                            cell.style.modifiers.contains(Modifier::REVERSED),
                            y == 0 && columns.contains(&x),
                            "{policy:?}, forward={forward}, step={step}, {point:?}"
                        );
                    }
                }
                let reference = tree.prepare_full(&view, size, &mut renderer)?;
                assert_eq!(frame.buffer(), reference.buffer());
                assert_eq!(frame.patch(), reference.patch());
                tree.discard(reference, &mut renderer);
                tree.commit(frame, &mut renderer)?;
            }
        }
    }
    Ok(())
}

#[test]
fn selection_clips_wide_spans_without_highlighting_the_spare_cursor_cell()
-> Result<(), Box<dyn Error>> {
    let wide = "\u{754c}";
    let emoji = "\u{1f469}\u{200d}\u{1f4bb}";
    // Empty strings are clipped/unused cells; "~" denotes a wide continuation.
    for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
        let cases: &[(bool, &[&str], usize)] = if policy == WidthPolicy::WcWidth {
            &[
                (false, &["", emoji, "~", "~", "~", "b", ""], 6),
                (false, &["", "b", ""], 2),
                (false, &[emoji, "~", "~", "~", "b", ""], 5),
                (false, &[""], 0),
                (true, &["a", ""], 2),
                (true, &["a", wide, "~", ""], 4),
                (true, &["a", wide, "~", emoji, "~", "~", "~", "b", ""], 8),
            ]
        } else {
            &[
                (false, &["", emoji, "~", "b", ""], 4),
                (false, &["", "b", ""], 2),
                (false, &[emoji, "~", "b", ""], 3),
                (false, &[""], 0),
                (true, &["a", ""], 2),
                (true, &["a", wide, "~", ""], 4),
                (true, &["a", wide, "~", emoji, "~", "b", ""], 6),
            ]
        };
        for &(backward, expected, selected_columns) in cases {
            let size = Size::new(u16::try_from(expected.len())?, 1);
            let buffer = TextBuffer::new(format!("a{wide}{emoji}b"));
            let view = TextInput::new(&buffer, |updated| updated).build();
            let mut tree = UiTree::new();
            let mut renderer = Renderer::new(size, policy);
            prepare_and_commit(&mut tree, &view, size, &mut renderer)?;
            let event = if backward {
                key(UiKey::Home, KeyModifiers::SHIFT)
            } else {
                key(UiKey::Character('a'), KeyModifiers::CONTROL)
            };
            let outcome = tree.dispatch(&view, &event, &renderer)?;
            let selected = outcome
                .messages
                .into_iter()
                .next()
                .ok_or("missing selection update")?;
            assert_eq!(
                selected.selection().map(|s| s.byte_range()),
                Some(0..buffer.text().len())
            );
            assert_eq!(buffer.selection(), None);
            let view = TextInput::new(&selected, |updated| updated).build();
            let frame = tree.prepare(&view, size, &mut renderer)?;
            tree.commit(frame, &mut renderer)?;

            // Commit newly visible graphemes before comparison so IDs are shared.
            // Force a physical repaint so every grapheme is available by value.
            renderer.invalidate();
            let full = tree.prepare_full(&view, size, &mut renderer)?;
            assert_eq!(renderer.current(), full.buffer());
            full.patch().validate_for_width_policy(policy)?;
            for (x, expected) in expected.iter().enumerate() {
                let point = Point::new(i32::try_from(x)?, 0);
                let cell = full.buffer().get(point).ok_or("missing cell")?;
                assert_eq!(
                    cell.style.modifiers.contains(Modifier::REVERSED),
                    x < selected_columns,
                    "{policy:?}, backward={backward}, width={}, x={x}",
                    size.width
                );
                match *expected {
                    "" => assert_eq!(cell.content, CellContent::Empty),
                    "~" => assert!(matches!(cell.content, CellContent::Continuation { .. })),
                    text => assert_eq!(patch_grapheme(full.patch(), point), Some(text)),
                }
            }
            tree.discard(full, &mut renderer);
        }
    }
    Ok(())
}

#[test]
fn selection_style_inherits_resolved_colors_and_adds_modifiers() -> Result<(), Box<dyn Error>> {
    let mut buffer = TextBuffer::new("a\u{1f469}\u{200d}\u{1f4bb}b");
    buffer.apply(TextEdit::Move {
        movement: TextMovement::Home,
        extend_selection: false,
    });
    buffer.apply(TextEdit::Move {
        movement: TextMovement::Right,
        extend_selection: false,
    });
    buffer.apply(TextEdit::Move {
        movement: TextMovement::Right,
        extend_selection: true,
    });
    let size = Size::new(10, 3);
    for focused in [true, false] {
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
        for selection_style in [
            None,
            Some(
                Style::new()
                    .foreground(Color::Black)
                    .background(Color::White)
                    .underline_color(Color::Red)
                    .add_modifiers(Modifier::DIM),
            ),
            Some(Style::DEFAULT),
        ] {
            let mut input = TextInput::new(&buffer, |updated| updated)
                .style(
                    Style::new()
                        .background(Color::Black)
                        .add_modifiers(Modifier::ITALIC),
                )
                .layout(LayoutStyle::new().size(Dimension::cells(8), Dimension::cells(3)));
            if let Some(style) = selection_style {
                input = input.selection_style(style);
            }
            let view = Element::container([
                input.build().key("input").focus_style(
                    Style::new()
                        .foreground(Color::Green)
                        .add_modifiers(Modifier::UNDERLINED),
                ),
                Element::container([])
                    .key("other")
                    .focusable(true)
                    .layout(LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(1))),
            ])
            .style(
                Style::new()
                    .foreground(Color::Red)
                    .background(Color::Blue)
                    .underline_color(Color::Yellow)
                    .add_modifiers(Modifier::BOLD),
            );
            if tree.is_empty() {
                prepare_and_commit(&mut tree, &view, size, &mut renderer)?;
                tree.focus_key(&Key::from(if focused { "input" } else { "other" }))?;
            }
            let frame = tree.prepare(&view, size, &mut renderer)?;
            let normal = Style::new()
                .foreground(if focused { Color::Green } else { Color::Red })
                .background(Color::Black)
                .underline_color(Color::Yellow)
                .add_modifiers(
                    Modifier::BOLD
                        | Modifier::ITALIC
                        | if focused {
                            Modifier::UNDERLINED
                        } else {
                            Modifier::EMPTY
                        },
                );
            let overlay = selection_style.unwrap_or(Style::new().add_modifiers(Modifier::REVERSED));
            let selected = Style {
                foreground: overlay.foreground.or(normal.foreground),
                background: overlay.background.or(normal.background),
                underline_color: overlay.underline_color.or(normal.underline_color),
                modifiers: normal.modifiers | overlay.modifiers,
            };
            for y in 0..3 {
                for x in 0..8 {
                    assert_eq!(
                        frame
                            .buffer()
                            .get(Point::new(x, y))
                            .ok_or("missing styled cell")?
                            .style,
                        if y == 0 && (1..3).contains(&x) {
                            selected
                        } else {
                            normal
                        }
                    );
                }
            }
            let reference = tree.prepare_full(&view, size, &mut renderer)?;
            assert_eq!(frame.buffer(), reference.buffer());
            assert_eq!(frame.patch(), reference.patch());
            tree.discard(reference, &mut renderer);
            tree.commit(frame, &mut renderer)?;
        }
    }
    Ok(())
}

#[test]
fn editing_replaces_only_the_controlled_selected_span() -> Result<(), Box<dyn Error>> {
    let mut buffer = TextBuffer::new("La\u{301}\u{1f469}\u{200d}\u{1f4bb}R");
    buffer.apply(TextEdit::Move {
        movement: TextMovement::Home,
        extend_selection: false,
    });
    buffer.apply(TextEdit::Move {
        movement: TextMovement::Right,
        extend_selection: false,
    });
    for _ in 0..2 {
        buffer.apply(TextEdit::Move {
            movement: TextMovement::Right,
            extend_selection: true,
        });
    }
    let original = buffer.clone();
    for (event, expected, cursor) in [
        (key(UiKey::Character('x'), KeyModifiers::NONE), "LxR", 2),
        (UiEvent::Text(String::from("x")), "LxR", 2),
        (UiEvent::Paste(String::from("x")), "LxR", 2),
        (key(UiKey::Backspace, KeyModifiers::NONE), "LR", 1),
        (key(UiKey::Delete, KeyModifiers::NONE), "LR", 1),
    ] {
        let size = Size::new(8, 1);
        let view = TextInput::new(&buffer, |updated| updated).build();
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view, size, &mut renderer)?;
        for (x, cell) in renderer.current().cells().iter().enumerate() {
            assert_eq!(
                cell.style.modifiers.contains(Modifier::REVERSED),
                (1..4).contains(&x)
            );
        }
        let outcome = tree.dispatch(&view, &event, &renderer)?;
        assert!(outcome.handled);
        assert_eq!(outcome.messages.len(), 1);
        let updated = outcome
            .messages
            .into_iter()
            .next()
            .ok_or("missing replacement")?;
        assert_eq!(updated.text(), expected);
        assert_eq!(updated.cursor().get(), cursor);
        assert_eq!(updated.selection(), None);
        assert_eq!(buffer, original);
        let view = TextInput::new(&updated, |updated| updated).build();
        let frame = tree.prepare(&view, size, &mut renderer)?;
        assert!(
            frame
                .buffer()
                .cells()
                .iter()
                .all(|cell| cell.style == Style::DEFAULT)
        );
        tree.commit(frame, &mut renderer)?;
        let reference = tree.prepare_full(&view, size, &mut renderer)?;
        assert_eq!(renderer.current(), reference.buffer());
        assert!(reference.patch().is_empty());
        tree.discard(reference, &mut renderer);
    }
    Ok(())
}

#[test]
fn selection_group_preserves_the_original_text_leaf_geometry() -> Result<(), Box<dyn Error>> {
    let size = Size::new(18, 6);
    for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
        for text in ["", "a\u{301}\u{1f469}\u{200d}\u{1f4bb}\u{754c}\u{b7}z"] {
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
                        for gap in [0, 2] {
                            for dimensions in [
                                (Dimension::Auto, Dimension::Auto),
                                (Dimension::cells(15), Dimension::cells(5)),
                                (Dimension::cells(3), Dimension::cells(1)),
                            ] {
                                let layout = LayoutStyle {
                                    width: dimensions.0,
                                    height: dimensions.1,
                                    direction,
                                    align,
                                    justify,
                                    gap,
                                    ..LayoutStyle::default()
                                };
                                let mut buffer = TextBuffer::new(text);
                                buffer.apply(TextEdit::Move {
                                    movement: TextMovement::Home,
                                    extend_selection: false,
                                });
                                let mut tree = UiTree::new();
                                let mut renderer = Renderer::new(size, policy);
                                let mut original_geometry = None;
                                let mut original_content = None;
                                for selection in [0, 1, 2, 0] {
                                    buffer.apply(TextEdit::Move {
                                        movement: TextMovement::Home,
                                        extend_selection: false,
                                    });
                                    if selection != 0 {
                                        buffer.apply(TextEdit::Move {
                                            movement: if selection == 1 {
                                                TextMovement::Right
                                            } else {
                                                TextMovement::End
                                            },
                                            extend_selection: false,
                                        });
                                        buffer.apply(TextEdit::Move {
                                            movement: TextMovement::Home,
                                            extend_selection: true,
                                        });
                                    }
                                    // Isolate the group's natural geometry from cursor-driven scrolling,
                                    // matching the original text leaf's unshifted layout below.
                                    let input = TextInput::new(&buffer, |updated| updated)
                                        .layout(layout)
                                        .build()
                                        .child_offset(Point::ORIGIN);
                                    let legacy = Element::<TextBuffer>::custom(
                                        "text-input",
                                        [
                                            Element::text(text),
                                            Element::container([]).layout(
                                                LayoutStyle::new()
                                                    .size(Dimension::cells(1), Dimension::cells(1)),
                                            ),
                                        ],
                                    )
                                    .layout(input.layout_style());
                                    let legacy_view = Element::container([legacy]);
                                    let mut legacy_tree = UiTree::new();
                                    let legacy_frame = legacy_tree.prepare_full(
                                        &legacy_view,
                                        size,
                                        &mut renderer,
                                    )?;
                                    let legacy_content: Vec<_> = legacy_frame
                                        .buffer()
                                        .cells()
                                        .iter()
                                        .map(|cell| cell.content)
                                        .collect();
                                    legacy_tree.commit(legacy_frame, &mut renderer)?;
                                    let legacy_root =
                                        legacy_tree.root().ok_or("missing legacy root")?;
                                    let legacy_input = legacy_tree
                                        .node(legacy_root)
                                        .ok_or("missing legacy root")?
                                        .children()[0];
                                    let legacy_input = legacy_tree
                                        .node(legacy_input)
                                        .ok_or("missing legacy input")?;
                                    let legacy_text = legacy_tree
                                        .node(legacy_input.children()[0])
                                        .ok_or("missing legacy text")?;
                                    let legacy_slot = legacy_tree
                                        .node(legacy_input.children()[1])
                                        .ok_or("missing legacy slot")?;
                                    let geometry = (
                                        legacy_input.layout(),
                                        legacy_text.layout(),
                                        legacy_slot.layout(),
                                    );

                                    let view = Element::container([input]);
                                    prepare_and_commit(&mut tree, &view, size, &mut renderer)?;
                                    let root = tree.root().ok_or("missing root")?;
                                    let input = tree
                                        .node(tree.node(root).ok_or("missing root")?.children()[0])
                                        .ok_or("missing input")?;
                                    assert_eq!(input.children().len(), 2);
                                    let group = tree
                                        .node(input.children()[0])
                                        .ok_or("missing text group")?;
                                    let slot = tree
                                        .node(input.children()[1])
                                        .ok_or("missing cursor slot")?;
                                    assert_eq!(group.children().len(), 3);
                                    assert_eq!(
                                        (input.layout(), group.layout(), slot.layout()),
                                        geometry,
                                        "{policy:?}, {layout:?}, selection={selection}, text={text:?}"
                                    );
                                    assert_eq!(
                                        *original_geometry.get_or_insert(geometry),
                                        geometry
                                    );
                                    let content: Vec<_> = renderer
                                        .current()
                                        .cells()
                                        .iter()
                                        .map(|cell| cell.content)
                                        .collect();
                                    assert_eq!(content, legacy_content);
                                    assert_eq!(
                                        *original_content.get_or_insert(content.clone()),
                                        content
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
