//! Ctrl+C must exit the demo. Raw mode clears `ISIG`, so the interrupt arrives as
//! a key event rather than `SIGINT`, and only the runtime interrupt policy ends it.

use arborui::{Size, TerminalViewport};
use arborui_example_kitty_image::KittyImageDemo;
use arborui_test::{KeyCode, KeyEventKind, KeyModifiers, TestApp};

fn demo() -> KittyImageDemo {
    KittyImageDemo::new(
        "kitty graphics: disabled",
        TerminalViewport::from_cells(Size::new(60, 20)),
    )
    .expect("demo application should build its generated images")
}

#[test]
fn control_c_quits_the_demo() {
    let mut app = TestApp::new(demo(), Size::new(60, 20));
    app.key_with(
        KeyCode::Character('c'),
        KeyModifiers::CONTROL,
        KeyEventKind::Press,
    );
    assert!(app.is_quitting());
}

#[test]
fn q_still_quits_the_demo() {
    let mut app = TestApp::new(demo(), Size::new(60, 20));
    app.key_with(
        KeyCode::Character('q'),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    );
    assert!(app.is_quitting());
}

#[test]
fn plain_c_does_not_quit_the_demo() {
    let mut app = TestApp::new(demo(), Size::new(60, 20));
    app.key_with(
        KeyCode::Character('c'),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    );
    assert!(!app.is_quitting());
}
