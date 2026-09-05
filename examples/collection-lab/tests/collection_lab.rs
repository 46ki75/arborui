//! Application evidence through the public ArborUI facades.

use arborui_example_collection_lab::{
    CollectionLab, CollectionMode, LogAction, LogLab, Message, OVERLAY_CANCEL_KEY,
    OVERLAY_CONFIRM_KEY, OVERLAY_OPEN_KEY, OverlayAction, OverlayLab, TableAction, TableLab,
    UnicodeLab,
};
use arborui_test::{Key, KeyCode, Point, Size, TestApp, TestCellContent};

#[test]
fn unicode_shift_omits_a_grapheme_cut_by_the_left_clip() {
    let mut application = TestApp::new(UnicodeLab::new(36, 10), Size::new(36, 10));

    for _ in 0..16 {
        application.key(KeyCode::Right);
    }

    assert_eq!(application.application().model().offset(), 16);
    assert_eq!(
        application
            .frame()
            .cell(Point::new(1, 3))
            .map(|cell| &cell.content),
        Some(&TestCellContent::Empty)
    );
    assert!(matches!(
        application.frame().cell(Point::new(2, 3)).map(|cell| &cell.content),
        Some(TestCellContent::Grapheme { text, width: 2 }) if text.as_ref() == "京"
    ));
    assert_eq!(
        application
            .frame()
            .cell(Point::new(3, 3))
            .map(|cell| &cell.content),
        Some(&TestCellContent::Continuation { offset: 1 })
    );
}

#[test]
fn overlay_traps_focus_and_restores_the_open_control() {
    let mut application = TestApp::new(OverlayLab::new(40, 12), Size::new(40, 12));
    assert_eq!(application.focused_key(), Some(Key::from(OVERLAY_OPEN_KEY)));

    application.key(KeyCode::Enter);
    assert!(application.application().model().dialog_open());
    assert_eq!(
        application.focused_key(),
        Some(Key::from(OVERLAY_CONFIRM_KEY))
    );
    application.key(KeyCode::Tab);
    assert_eq!(
        application.focused_key(),
        Some(Key::from(OVERLAY_CANCEL_KEY))
    );
    application.key(KeyCode::Tab);
    assert_eq!(
        application.focused_key(),
        Some(Key::from(OVERLAY_CONFIRM_KEY))
    );

    application.key(KeyCode::Escape);
    assert!(!application.application().model().dialog_open());
    assert_eq!(application.focused_key(), Some(Key::from(OVERLAY_OPEN_KEY)));
}

#[test]
fn overlay_scrim_blocks_the_background_control() {
    let mut application = TestApp::new(OverlayLab::new(40, 12), Size::new(40, 12));
    application.send(OverlayAction::Open);

    application.click(Point::new(4, 5));

    assert!(application.application().model().dialog_open());
    assert_eq!(
        application.application().model().background_activations(),
        0
    );
}

#[test]
fn scrolling_log_is_bounded_and_paused_appends_preserve_the_view() {
    let mut application =
        TestApp::new(LogLab::new(1_000_000, 1_000_000, 48, 12), Size::new(48, 12));
    assert_eq!(application.application().constructed_rows(), 10);
    assert!(application.frame().characters().contains("Δelta"));

    application.send(LogAction::PageUp);
    let before = application.frame().characters();
    application.send(LogAction::Append {
        count: 4,
        generation: 1,
    });

    assert!(!application.application().model().follows_tail());
    assert_eq!(application.application().model().records().len(), 1_000_000);
    assert_eq!(application.application().model().generation(), 1);
    assert_eq!(application.frame().characters(), before);
    assert_eq!(application.application().constructed_rows(), 12);
}

#[test]
fn scrolling_log_keyboard_append_advances_the_producer_generation() {
    let mut application = TestApp::new(LogLab::new(100, 110, 48, 12), Size::new(48, 12));

    application.key(KeyCode::Character('a'));

    assert_eq!(application.application().model().records().len(), 101);
    assert_eq!(application.application().model().generation(), 1);
    assert!(application.application().model().follows_tail());
}

#[test]
fn table_keeps_selection_through_updates_and_resize() {
    let mut application = TestApp::new(TableLab::new(100_000, 48, 12), Size::new(48, 12));

    application.send(TableAction::PageDown);
    application.send(TableAction::Down);
    application.send(TableAction::SelectActive);
    application.send(TableAction::BackgroundUpdate {
        key: 8,
        revision: 1,
    });
    application.send(TableAction::BackgroundUpdate {
        key: 99_999,
        revision: 2,
    });
    application.resize(Size::new(34, 9));

    assert_eq!(application.application().model().active_key(), Some(8));
    assert_eq!(application.application().model().selected_key(), Some(8));
    assert_eq!(application.application().model().generation(), 2);
    assert_eq!(
        application.application().model().rows()[8].status(),
        "updating"
    );
    assert_eq!(application.application().constructed_rows(), 8);
    assert!(application.frame().characters().contains("Krakó"));
}

#[test]
fn fixed_construction_and_tree_size_are_independent_of_item_count() {
    let small = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 100, 8),
        Size::new(48, 12),
    );
    let large = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 1_000_000, 8),
        Size::new(48, 12),
    );

    assert_eq!(small.application().constructed_rows(), 10);
    assert_eq!(large.application().constructed_rows(), 10);
    assert_eq!(small.ui_tree().len(), large.ui_tree().len());
    assert_eq!(
        large.focused_key(),
        Some(Key::from("collection")),
        "frame:\n{}",
        large.frame()
    );
}

#[test]
fn selection_and_focus_survive_unmounting_and_reorder_by_stable_key() {
    let mut app = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 100, 6),
        Size::new(40, 10),
    );

    app.key(KeyCode::Enter);
    assert_eq!(app.application().selected_key(), Some(0));
    app.key(KeyCode::End);
    assert_eq!(app.application().active_key(), Some(99));
    assert_eq!(app.application().selected_key(), Some(0));
    assert!(app.application().scroll_offset() > 0);
    app.send(Message::Reverse);
    assert_eq!(app.application().active_key(), Some(99));
    assert_eq!(app.application().selected_key(), Some(0));
    assert_eq!(app.focused_key(), Some(Key::from("collection")));
    app.key(KeyCode::Home);
    assert_eq!(app.application().active_key(), Some(99));
    assert_eq!(app.application().selected_key(), Some(0));
}

#[test]
fn startup_end_uses_terminal_viewport_before_resize() {
    let size = Size::new(48, 12);
    let mut app = TestApp::new(CollectionLab::from_terminal_size(size), size);

    app.key(KeyCode::End);

    assert_eq!(app.application().active_key(), Some(99_999));
    let startup_viewport = app.application().viewport_height();
    let startup_scroll = app.application().scroll_offset();
    let startup_range = app.application().visible_range();
    let startup_frame = app.frame().clone();

    app.resize(size);

    assert_eq!(app.application().active_key(), Some(99_999));
    assert_eq!(app.application().viewport_height(), 8);
    assert_eq!(app.application().scroll_offset(), 99_992);
    assert!(app.frame().characters().contains("Item 099999"));
    assert!(
        startup_frame.characters().contains("Item 099999"),
        "End must reveal the active item before resize: viewport={startup_viewport}, scroll={startup_scroll}"
    );
    assert_eq!(startup_viewport, app.application().viewport_height());
    assert_eq!(startup_scroll, app.application().scroll_offset());
    assert_eq!(startup_range, app.application().visible_range());
    assert_eq!(&startup_frame, app.frame());
}

#[test]
fn resize_recomputes_the_bounded_window() {
    let mut app = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 10_000, 4),
        Size::new(40, 8),
    );
    assert_eq!(app.application().constructed_rows(), 6);

    app.resize(Size::new(40, 14));

    assert_eq!(app.application().viewport_height(), 10);
    assert_eq!(app.application().constructed_rows(), 12);
}

#[test]
fn pointer_selection_maps_a_visible_row_to_its_stable_key() {
    let mut app = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 10_000, 6),
        Size::new(40, 10),
    );

    app.click(Point::new(3, 4));

    assert_eq!(app.application().selected_key(), Some(2));
    assert_eq!(app.application().active_key(), Some(2));
    assert_eq!(app.focused_key(), Some(Key::from("collection")));
}

#[test]
fn variable_rows_render_from_cached_measurements() {
    let mut app = TestApp::new(
        CollectionLab::new(CollectionMode::Variable, 100_000, 7),
        Size::new(38, 11),
    );

    app.key(KeyCode::PageDown);
    app.key(KeyCode::Down);
    app.key(KeyCode::Enter);

    assert_eq!(
        app.application().selected_key(),
        app.application().active_key()
    );
    assert!(app.application().constructed_rows() < 10);
    insta::assert_snapshot!("collection_lab_variable", app.frame());
}
