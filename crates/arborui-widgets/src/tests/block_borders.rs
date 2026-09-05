use super::*;
use arborui_core::{Insets, Rect};

use crate::BorderSet;

const ASCII_FRAME: &str = "+-----+\n|x    |\n+-----+\n";
const UNICODE_FRAME: &str = concat!(
    "\u{250c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}\n",
    "\u{2502}x    \u{2502}\n",
    "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}\n",
);

fn assert_frame(
    patch: &FramePatch,
    policy: WidthPolicy,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    patch.validate_for_width_policy(policy)?;
    assert!(patch.full_repaint);
    let mut actual = String::new();
    for y in 0..i32::from(patch.size.height) {
        for x in 0..i32::from(patch.size.width) {
            actual.push_str(patch_grapheme(patch, Point::new(x, y)).unwrap_or(" "));
        }
        actual.push('\n');
    }
    assert_eq!(actual, expected);
    Ok(())
}

#[test]
fn cjk_block_preserves_a_complete_border() -> Result<(), Box<dyn Error>> {
    let size = Size::new(7, 3);
    let view = Block::new(Element::<()>::text("x"))
        .layout(LayoutStyle::new().size(Dimension::cells(7), Dimension::cells(3)))
        .build();
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(size, WidthPolicy::Cjk);
    let prepared = tree.prepare(&view, size, &mut renderer)?;

    assert_frame(prepared.patch(), WidthPolicy::Cjk, ASCII_FRAME)?;
    for point in [
        Point::new(0, 0),
        Point::new(6, 0),
        Point::new(0, 2),
        Point::new(6, 2),
    ] {
        assert_eq!(patch_grapheme(prepared.patch(), point), Some("+"));
    }
    for x in [0, 6] {
        assert_eq!(
            patch_grapheme(prepared.patch(), Point::new(x, 1)),
            Some("|")
        );
    }
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(1, 1)),
        Some("x")
    );

    tree.commit(prepared, &mut renderer)?;
    let root = tree
        .node(tree.root().ok_or("missing block root")?)
        .ok_or("missing block")?;
    let child = tree.node(root.children()[0]).ok_or("missing content")?;
    assert_eq!(child.layout(), Rect::new(1, 1, 1, 1));
    Ok(())
}

#[test]
fn border_sets_respect_all_width_policies() -> Result<(), Box<dyn Error>> {
    let size = Size::new(7, 3);
    for border in [BorderSet::Unicode, BorderSet::Ascii] {
        for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
            let view = Block::new(Element::<()>::text("x"))
                .border(border)
                .layout(LayoutStyle::new().size(Dimension::cells(7), Dimension::cells(3)))
                .build();
            let tree = UiTree::new();
            let mut renderer = Renderer::new(size, policy);
            let prepared = tree.prepare(&view, size, &mut renderer)?;
            let expected = if border == BorderSet::Ascii || policy == WidthPolicy::Cjk {
                ASCII_FRAME
            } else {
                UNICODE_FRAME
            };
            assert_frame(prepared.patch(), policy, expected)?;
        }
    }
    Ok(())
}

#[test]
fn cjk_fallback_preserves_padding_and_styles() -> Result<(), Box<dyn Error>> {
    let size = Size::new(7, 5);
    let border_style = Style::new().foreground(Color::Red);
    let view = Block::new(Element::<()>::text("x"))
        .padding(Insets::all(1))
        .style(Style::new().background(Color::Blue))
        .border_style(border_style)
        .layout(LayoutStyle::new().size(Dimension::cells(7), Dimension::cells(5)))
        .build();
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(size, WidthPolicy::Cjk);
    let prepared = tree.prepare(&view, size, &mut renderer)?;

    assert_frame(
        prepared.patch(),
        WidthPolicy::Cjk,
        "+-----+\n|     |\n| x   |\n|     |\n+-----+\n",
    )?;
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(2, 2)),
        Some("x")
    );
    for y in 0..5 {
        for x in 0..7 {
            let cell = prepared
                .buffer()
                .get(Point::new(x, y))
                .ok_or("missing cell")?;
            if x == 0 || x == 6 || y == 0 || y == 4 {
                assert_eq!(cell.style, border_style);
            } else {
                assert_eq!(cell.style.background, Some(Color::Blue));
            }
        }
    }
    tree.commit(prepared, &mut renderer)?;
    let root = tree
        .node(tree.root().ok_or("missing block root")?)
        .ok_or("missing block")?;
    let child = tree.node(root.children()[0]).ok_or("missing content")?;
    assert_eq!(child.layout(), Rect::new(2, 2, 1, 1));
    Ok(())
}

#[test]
fn same_block_view_reselects_glyphs_after_width_policy_changes() -> Result<(), Box<dyn Error>> {
    let size = Size::new(7, 3);
    let view = Block::new(Element::<()>::text("x"))
        .layout(LayoutStyle::new().size(Dimension::cells(7), Dimension::cells(3)))
        .build();
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    for policy in [
        WidthPolicy::Unicode,
        WidthPolicy::Cjk,
        WidthPolicy::WcWidth,
        WidthPolicy::Cjk,
        WidthPolicy::Unicode,
    ] {
        renderer.set_width_policy(policy);
        let prepared = tree.prepare(&view, size, &mut renderer)?;
        let expected = if policy == WidthPolicy::Cjk {
            ASCII_FRAME
        } else {
            UNICODE_FRAME
        };
        assert_frame(prepared.patch(), renderer.width_policy(), expected)?;
        tree.commit(prepared, &mut renderer)?;
    }
    Ok(())
}

#[test]
fn tiny_block_borders_stay_within_frame_bounds() -> Result<(), Box<dyn Error>> {
    for border in [BorderSet::Unicode, BorderSet::Ascii] {
        for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
            for width in 0..=2 {
                for height in 0..=2 {
                    let size = Size::new(width, height);
                    let view = Block::new(Element::<()>::text(""))
                        .border(border)
                        .layout(
                            LayoutStyle::new()
                                .size(Dimension::cells(width), Dimension::cells(height)),
                        )
                        .build();
                    let tree = UiTree::new();
                    let mut renderer = Renderer::new(size, policy);
                    let prepared = tree.prepare(&view, size, &mut renderer)?;
                    prepared
                        .patch()
                        .validate_for_width_policy(renderer.width_policy())?;
                    assert_eq!(prepared.buffer().size(), size);
                    for y in 0..i32::from(height) {
                        for x in 0..i32::from(width) {
                            let expected =
                                if border == BorderSet::Ascii || policy == WidthPolicy::Cjk {
                                    "+"
                                } else {
                                    match (x, y) {
                                        (0, 0) => "\u{250c}",
                                        (1, 0) => "\u{2510}",
                                        (0, 1) => "\u{2514}",
                                        _ => "\u{2518}",
                                    }
                                };
                            assert_eq!(
                                patch_grapheme(prepared.patch(), Point::new(x, y)),
                                Some(expected)
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
