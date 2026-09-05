use std::error::Error;

use arborui_core::{Point, Size};
use arborui_layout::{Dimension, LayoutStyle};
use arborui_render::Renderer;
use arborui_text::WidthPolicy;
use arborui_ui::{
    Element, EventPhase, Key, KeyModifiers, PointerButton, PointerEventKind, UiEvent, UiKey, UiTree,
};

use super::{key, pointer, prepare_and_commit};
use crate::{Block, Button, Dialog, column, stack};

fn focusable_panel() -> Element<'static, &'static str> {
    Block::new(
        Button::new("confirm", || "confirm")
            .layout(LayoutStyle::new().size(Dimension::cells(7), Dimension::cells(1)))
            .build()
            .key("confirm"),
    )
    .layout(LayoutStyle::new().size(Dimension::cells(11), Dimension::cells(3)))
    .build()
    .key("panel")
    .focusable(true)
}

fn view(panel: Option<Element<'static, &'static str>>) -> Element<'static, &'static str> {
    let mut layers = vec![
        column(["first", "background"].map(|name| {
            Button::new(name, move || name)
                .layout(LayoutStyle::new().size(Dimension::percent(100), Dimension::cells(1)))
                .build()
                .key(name)
        }))
        .key("background-controls"),
    ];
    if let Some(panel) = panel {
        layers.push(Dialog::new(panel, || "dismiss").build().key("dialog"));
    }
    stack(layers).layout(LayoutStyle::new().size(Dimension::percent(100), Dimension::percent(100)))
}

fn focused_key(tree: &UiTree) -> Option<Key> {
    tree.focused()
        .and_then(|node| tree.node(node))
        .and_then(|node| node.key())
        .cloned()
}

#[test]
fn clicking_focusable_dialog_panel_focuses_it() -> Result<(), Box<dyn Error>> {
    for caller_prevents_default in [None, Some(false), Some(true)] {
        let mut panel = focusable_panel();
        if let Some(prevent_default) = caller_prevents_default {
            panel = panel.on_event(EventPhase::Target, move |event, context| {
                if matches!(event, UiEvent::Pointer(_)) {
                    context.emit("panel");
                    if prevent_default {
                        context.prevent_default();
                    }
                }
            });
        }
        let view = view(Some(panel)).on_event(EventPhase::Bubble, |event, context| {
            if matches!(event, UiEvent::Pointer(_)) {
                context.emit("bubble");
            }
        });
        let size = Size::new(21, 7);
        let mut tree = UiTree::new();
        let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
        prepare_and_commit(&mut tree, &view, size, &mut renderer)?;

        assert_eq!(focused_key(&tree), Some(Key::from("panel")));
        let panel = tree.focused().ok_or("missing panel focus")?;
        let tab = tree.dispatch(&view, &key(UiKey::Tab, KeyModifiers::NONE), &renderer)?;
        assert!(!tab.default_prevented);
        assert_eq!(focused_key(&tree), Some(Key::from("confirm")));

        // This blank cell is inside the panel, to the right of its child button.
        let exposed = Point::new(14, 3);
        assert_eq!(tree.hit_test(renderer.hit_map(), exposed), Some(panel));
        let down = tree.dispatch(
            &view,
            &pointer(
                PointerEventKind::Down(PointerButton::Primary),
                exposed.x,
                exposed.y,
            ),
            &renderer,
        )?;
        assert_eq!(down.target, Some(panel));
        let expected_messages = if caller_prevents_default.is_some() {
            vec!["panel", "bubble"]
        } else {
            vec!["bubble"]
        };
        assert_eq!(down.messages, expected_messages);
        assert!(down.handled);
        assert!(!down.propagation_stopped);
        assert_eq!(
            down.default_prevented,
            caller_prevents_default == Some(true)
        );
        let expected_focus = if caller_prevents_default == Some(true) {
            "confirm"
        } else {
            "panel"
        };
        assert_eq!(focused_key(&tree), Some(Key::from(expected_focus)));
        assert_eq!(tree.captured_pointer(), None);
    }
    Ok(())
}

#[test]
fn focusable_dialog_panel_preserves_modal_controls() -> Result<(), Box<dyn Error>> {
    let size = Size::new(21, 7);
    let base = view(None);
    let mut tree = UiTree::new();
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    prepare_and_commit(&mut tree, &base, size, &mut renderer)?;
    assert_eq!(focused_key(&tree), Some(Key::from("first")));
    let _ = tree.dispatch(&base, &key(UiKey::Tab, KeyModifiers::NONE), &renderer)?;
    assert_eq!(focused_key(&tree), Some(Key::from("background")));
    let previous_focus = tree.focused();
    assert_eq!(
        tree.hit_test(renderer.hit_map(), Point::new(0, 1)),
        previous_focus
    );

    let modal = view(Some(focusable_panel()));
    prepare_and_commit(&mut tree, &modal, size, &mut renderer)?;
    let scope = tree
        .active_focus_scope()
        .ok_or("missing dialog focus scope")?;
    assert_ne!(Some(scope), tree.root());
    assert!(
        tree.node(scope)
            .ok_or("missing dialog node")?
            .is_pointer_modal()
    );
    assert_eq!(focused_key(&tree), Some(Key::from("panel")));

    for (modifiers, expected) in [
        (KeyModifiers::NONE, "confirm"),
        (KeyModifiers::NONE, "panel"),
        (KeyModifiers::SHIFT, "confirm"),
        (KeyModifiers::SHIFT, "panel"),
    ] {
        let outcome = tree.dispatch(&modal, &key(UiKey::Tab, modifiers), &renderer)?;
        assert!(!outcome.default_prevented);
        assert!(outcome.messages.is_empty());
        assert_eq!(focused_key(&tree), Some(Key::from(expected)));
        assert_eq!(tree.active_focus_scope(), Some(scope));
    }

    let child = tree
        .hit_test(renderer.hit_map(), Point::new(6, 3))
        .ok_or("missing child hit")?;
    assert_eq!(
        tree.node(child).and_then(|node| node.key()),
        Some(&Key::from("confirm"))
    );
    let down = tree.dispatch(
        &modal,
        &pointer(PointerEventKind::Down(PointerButton::Primary), 6, 3),
        &renderer,
    )?;
    assert_eq!(down.target, Some(child));
    assert_eq!(down.messages, ["confirm"]);
    assert!(down.handled);
    assert!(!down.default_prevented);
    assert_eq!(tree.focused(), Some(child));
    assert_eq!(tree.captured_pointer(), Some(child));
    let up = tree.dispatch(
        &modal,
        &pointer(PointerEventKind::Up(PointerButton::Primary), 6, 3),
        &renderer,
    )?;
    assert_eq!(up.target, Some(child));
    assert!(up.messages.is_empty());
    assert_eq!(tree.captured_pointer(), None);
    let enter = tree.dispatch(&modal, &key(UiKey::Enter, KeyModifiers::NONE), &renderer)?;
    assert_eq!(enter.messages, ["confirm"]);

    for (kind, x, y, capture) in [
        (
            PointerEventKind::Down(PointerButton::Primary),
            0,
            1,
            Some(scope),
        ),
        (
            PointerEventKind::Drag(PointerButton::Primary),
            6,
            3,
            Some(scope),
        ),
        (PointerEventKind::Up(PointerButton::Primary), 6, 3, None),
        (PointerEventKind::Scroll(1), 0, 1, None),
    ] {
        let outside = tree.dispatch(&modal, &pointer(kind, x, y), &renderer)?;
        assert_eq!(outside.target, Some(scope));
        assert!(outside.messages.is_empty());
        assert!(outside.handled);
        assert!(outside.default_prevented);
        assert_eq!(tree.captured_pointer(), capture);
        assert_eq!(tree.focused(), Some(child));
    }

    let escape = tree.dispatch(&modal, &key(UiKey::Escape, KeyModifiers::NONE), &renderer)?;
    assert_eq!(escape.messages, ["dismiss"]);
    assert!(escape.handled);
    assert!(escape.default_prevented);
    assert!(escape.propagation_stopped);

    prepare_and_commit(&mut tree, &base, size, &mut renderer)?;
    assert_eq!(tree.active_focus_scope(), tree.root());
    assert_eq!(tree.focused(), previous_focus);
    assert_eq!(focused_key(&tree), Some(Key::from("background")));
    let background = tree.dispatch(
        &base,
        &pointer(PointerEventKind::Down(PointerButton::Primary), 0, 1),
        &renderer,
    )?;
    assert_eq!(background.target, previous_focus);
    assert_eq!(background.messages, ["background"]);
    Ok(())
}
