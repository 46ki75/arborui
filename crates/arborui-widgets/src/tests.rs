use std::error::Error;

use arborui_core::{CursorVisibility, Point, Size};
use arborui_layout::{Dimension, LayoutStyle};
use arborui_render::{FramePatch, PatchCellContent, Renderer};
use arborui_text::{TextBuffer, TextEdit, TextMovement, WidthPolicy};
use arborui_ui::{
    Element, KeyAction, KeyModifiers, PointerButton, PointerEvent, PointerEventKind, UiEvent,
    UiKey, UiKeyEvent, UiTree,
};

use crate::{Block, Button, ScrollView, TextInput, stack};

fn prepare_and_commit<Message>(
    tree: &mut UiTree,
    view: &Element<'_, Message>,
    size: Size,
    renderer: &mut Renderer,
) -> Result<(), Box<dyn Error>> {
    let prepared = tree.prepare(view, size, renderer)?;
    tree.commit(prepared, renderer)?;
    Ok(())
}

fn patch_grapheme(patch: &FramePatch, point: Point) -> Option<&str> {
    patch.runs.iter().find_map(|run| {
        if run.position.y != point.y || point.x < run.position.x {
            return None;
        }
        let offset = usize::try_from(point.x - run.position.x).ok()?;
        let cell = run.cells.get(offset)?;
        match &cell.content {
            PatchCellContent::Grapheme { text, .. } => Some(text.as_ref()),
            PatchCellContent::Empty | PatchCellContent::Continuation { .. } => None,
        }
    })
}

fn key(key: UiKey, modifiers: KeyModifiers) -> UiEvent {
    UiEvent::Key(UiKeyEvent {
        key,
        modifiers,
        action: KeyAction::Press,
    })
}

fn pointer(kind: PointerEventKind, x: i32, y: i32) -> UiEvent {
    UiEvent::Pointer(PointerEvent {
        kind,
        position: Point::new(x, y),
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn block_paints_border_title_and_inset_content() -> Result<(), Box<dyn Error>> {
    let view = Block::new(Element::<()>::text("x"))
        .title("T")
        .layout(LayoutStyle::new().size(Dimension::cells(7), Dimension::cells(3)))
        .build();
    let tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(7, 3), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(7, 3), &mut renderer)?;

    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(0, 0)),
        Some("┌")
    );
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(2, 0)),
        Some("T")
    );
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(6, 2)),
        Some("┘")
    );
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(1, 1)),
        Some("x")
    );
    Ok(())
}

#[test]
fn stack_preserves_sizes_and_paints_later_children_last() -> Result<(), Box<dyn Error>> {
    let child_layout = LayoutStyle::new().size(Dimension::cells(1), Dimension::cells(1));
    let view = stack([
        Element::<()>::text("A").layout(child_layout),
        Element::text("B").layout(child_layout),
    ])
    .layout(child_layout);
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(1, 1), &mut renderer)?;

    assert_eq!(patch_grapheme(prepared.patch(), Point::ORIGIN), Some("B"));
    tree.commit(prepared, &mut renderer)?;
    let root = tree.root().ok_or("missing stack root")?;
    let children = tree.node(root).ok_or("missing stack node")?.children();
    assert_eq!(children.len(), 2);
    for child in children {
        assert_eq!(
            tree.node(*child)
                .ok_or("missing stack child")?
                .layout()
                .size(),
            Size::new(1, 1)
        );
    }
    Ok(())
}

#[test]
fn button_activates_on_pointer_press_and_keys() -> Result<(), Box<dyn Error>> {
    let view = Button::new("go", || String::from("pressed"))
        .layout(LayoutStyle::new().size(Dimension::cells(2), Dimension::cells(1)))
        .build();
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(2, 1), WidthPolicy::Unicode);
    prepare_and_commit(&mut tree, &view, Size::new(2, 1), &mut renderer)?;

    let down = tree.dispatch(
        &view,
        &pointer(PointerEventKind::Down(PointerButton::Primary), 0, 0),
        &renderer,
    )?;
    assert_eq!(down.messages, ["pressed"]);
    assert!(tree.captured_pointer().is_some());
    let up = tree.dispatch(
        &view,
        &pointer(PointerEventKind::Up(PointerButton::Primary), 5, 0),
        &renderer,
    )?;
    assert!(up.messages.is_empty());
    assert_eq!(tree.captured_pointer(), None);

    let enter = tree.dispatch(&view, &key(UiKey::Enter, KeyModifiers::NONE), &renderer)?;
    let space = tree.dispatch(
        &view,
        &key(UiKey::Character(' '), KeyModifiers::NONE),
        &renderer,
    )?;
    assert_eq!(enter.messages, ["pressed"]);
    assert_eq!(space.messages, ["pressed"]);
    Ok(())
}

#[test]
fn text_input_uses_renderer_width_and_accepts_alt_gr_characters() -> Result<(), Box<dyn Error>> {
    let buffer = TextBuffer::new("·");
    let view = TextInput::new(&buffer, |updated| updated).build();
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(6, 1), WidthPolicy::Cjk);
    prepare_and_commit(&mut tree, &view, Size::new(6, 1), &mut renderer)?;

    let prepared = tree.prepare(&view, Size::new(6, 1), &mut renderer)?;
    assert_eq!(prepared.patch().cursor.position, Point::new(2, 0));
    tree.discard(prepared, &mut renderer);

    let alt_gr = tree.dispatch(
        &view,
        &key(
            UiKey::Character('@'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ),
        &renderer,
    )?;
    assert_eq!(alt_gr.messages.first().map(TextBuffer::text), Some("·@"));
    Ok(())
}

#[test]
fn text_input_scrolls_horizontally_to_keep_cursor_visible() -> Result<(), Box<dyn Error>> {
    let buffer = TextBuffer::new("abcdef");
    let view = TextInput::new(&buffer, |updated| updated)
        .layout(LayoutStyle::new().size(Dimension::cells(3), Dimension::cells(1)))
        .build();
    let tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(3, 1), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(3, 1), &mut renderer)?;

    assert_eq!(prepared.patch().cursor.position, Point::new(2, 0));
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(0, 0)),
        Some("e")
    );
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(1, 0)),
        Some("f")
    );
    Ok(())
}

#[test]
fn intrinsic_text_starts_in_its_content_box() -> Result<(), Box<dyn Error>> {
    let view = Element::<()>::text("x").layout(LayoutStyle {
        padding: arborui_core::Insets::all(1),
        ..LayoutStyle::default()
    });
    let tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(3, 3), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(3, 3), &mut renderer)?;

    assert_eq!(patch_grapheme(prepared.patch(), Point::ORIGIN), None);
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(1, 1)),
        Some("x")
    );
    Ok(())
}

#[test]
fn auto_sized_stack_uses_its_first_layer_for_layout() -> Result<(), Box<dyn Error>> {
    let view = crate::column([stack([
        Element::<()>::text("A"),
        Element::text("B").fill_background(false),
    ])]);
    let mut renderer = Renderer::new(Size::new(3, 2), WidthPolicy::Unicode);
    let tree = UiTree::new();
    let prepared = tree.prepare(&view, Size::new(3, 2), &mut renderer)?;

    assert_eq!(patch_grapheme(prepared.patch(), Point::ORIGIN), Some("B"));
    Ok(())
}

#[test]
fn text_input_edits_unicode_and_places_cursor_by_display_width() -> Result<(), Box<dyn Error>> {
    let mut buffer = TextBuffer::new("a👩‍💻界");
    buffer.apply(TextEdit::Move {
        movement: TextMovement::Home,
        extend_selection: false,
    });
    buffer.apply(TextEdit::Move {
        movement: TextMovement::Right,
        extend_selection: false,
    });
    let view = TextInput::new(&buffer, |updated| updated).build();
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(8, 1), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(8, 1), &mut renderer)?;

    assert_eq!(
        prepared.patch().cursor.visibility,
        CursorVisibility::Visible
    );
    assert_eq!(prepared.patch().cursor.position, Point::new(1, 0));
    tree.commit(prepared, &mut renderer)?;
    let outcome = tree.dispatch(&view, &key(UiKey::Delete, KeyModifiers::NONE), &renderer)?;
    let updated = outcome.messages.first().ok_or("missing text update")?;
    assert_eq!(updated.text(), "a界");
    assert_eq!(updated.cursor().get(), 1);

    let no_op_buffer = TextBuffer::new("");
    let no_op_view = TextInput::new(&no_op_buffer, |updated| updated).build();
    prepare_and_commit(&mut tree, &no_op_view, Size::new(8, 1), &mut renderer)?;
    let no_op = tree.dispatch(
        &no_op_view,
        &key(UiKey::Backspace, KeyModifiers::NONE),
        &renderer,
    )?;
    assert!(no_op.messages.is_empty());
    Ok(())
}

#[test]
fn controlled_scroll_translates_clips_and_emits_signed_deltas() -> Result<(), Box<dyn Error>> {
    let content = Element::text("abc").layout(
        LayoutStyle::new()
            .size(Dimension::cells(3), Dimension::cells(1))
            .flex(0, 0),
    );
    let view = ScrollView::new(Point::new(1, 0), content)
        .on_scroll(|delta| delta)
        .layout(LayoutStyle::new().size(Dimension::cells(2), Dimension::cells(1)))
        .build();
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(Size::new(2, 1), WidthPolicy::Unicode);
    let prepared = tree.prepare(&view, Size::new(2, 1), &mut renderer)?;

    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(0, 0)),
        Some("b")
    );
    assert_eq!(
        patch_grapheme(prepared.patch(), Point::new(1, 0)),
        Some("c")
    );
    tree.commit(prepared, &mut renderer)?;
    let root = tree.root().ok_or("missing scroll root")?;
    let child = *tree
        .node(root)
        .ok_or("missing scroll node")?
        .children()
        .first()
        .ok_or("missing scroll child")?;
    assert_eq!(
        tree.node(child).ok_or("missing scroll child")?.layout().x,
        -1
    );

    let vertical = tree.dispatch(
        &view,
        &pointer(PointerEventKind::Scroll(-2), 0, 0),
        &renderer,
    )?;
    let horizontal = tree.dispatch(
        &view,
        &pointer(PointerEventKind::ScrollHorizontal(3), 0, 0),
        &renderer,
    )?;
    assert_eq!(vertical.messages, [Point::new(0, -2)]);
    assert_eq!(horizontal.messages, [Point::new(3, 0)]);
    Ok(())
}
