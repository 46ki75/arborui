//! Repeated measurement turns must not retain test instrumentation.

use arborui::Application;
use arborui_comparison_collection_lab_ratatui::{
    OVERLAY_RESIZE_STORM, STANDARD_RESIZE_STORM, UNICODE_RESIZE_STORM, UNICODE_RESIZE_STORM_OFFSET,
};
use arborui_example_collection_lab::{
    CollectionLab, CollectionMode, LogAction, LogLab, Message, OverlayAction, OverlayLab,
    TableAction, TableLab, UnicodeAction, UnicodeLab,
};
use arborui_test::{KeyCode, SettleOutcome, Size, TestApp, TestAppOptions};

#[test]
fn persistent_resize_storm_history_is_bounded() {
    for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
        let mut app = measurement_app(CollectionLab::new(mode, 100_000, 8), Size::new(48, 12));
        app.send(Message::SelectActive);
        let initial_frame = app.frame().clone();
        let initial_history = app.frame_patches().len();
        assert_eq!(initial_history, 0);
        for _ in 0..100 {
            for (width, height) in STANDARD_RESIZE_STORM {
                app.resize(Size::new(width, height));
            }
        }
        assert_eq!(app.frame(), &initial_frame);
        assert_eq!(app.application().active_key(), Some(0));
        assert_eq!(app.application().selected_key(), Some(0));
        let retained_cells: usize = app
            .frame_patches()
            .iter()
            .flat_map(|patch| &patch.runs)
            .map(|run| run.cells.len())
            .sum();
        assert_eq!(
            app.frame_patches().len(),
            initial_history,
            "100 storms: {initial_history} -> {} patches, {retained_cells} retained cells",
            app.frame_patches().len(),
        );
    }
}

#[test]
fn table_log_overlay_and_unicode_storm_history_is_bounded() {
    let mut table = measurement_app(TableLab::new(100_000, 48, 12), Size::new(48, 12));
    table.send(TableAction::SelectActive);
    assert_storm_history_bounded(&mut table, &STANDARD_RESIZE_STORM);
    assert_eq!(table.application().model().rows().len(), 100_000);
    assert_eq!(table.application().model().active_key(), Some(0));
    assert_eq!(table.application().model().selected_key(), Some(0));

    let mut log = measurement_app(LogLab::new(100_000, 200_000, 48, 12), Size::new(48, 12));
    log.send(LogAction::PageUp);
    let capacity = log.application().model().records().capacity();
    let offset = log.application().model().scroll_offset();
    assert_storm_history_bounded(&mut log, &STANDARD_RESIZE_STORM);
    assert_eq!(log.application().model().records().len(), 100_000);
    assert_eq!(log.application().model().records().capacity(), capacity);
    assert_eq!(log.application().model().scroll_offset(), offset);
    assert!(!log.application().model().follows_tail());

    let mut overlay = measurement_app(OverlayLab::new(40, 12), Size::new(40, 12));
    overlay.send(OverlayAction::Open);
    overlay.key(KeyCode::Tab);
    assert_storm_history_bounded(&mut overlay, &OVERLAY_RESIZE_STORM);
    assert!(overlay.application().model().dialog_open());
    assert_eq!(overlay.application().model().confirmations(), 0);
    assert_eq!(overlay.application().model().background_activations(), 0);

    let mut unicode = measurement_app(UnicodeLab::new(36, 10), Size::new(36, 10));
    for _ in 0..UNICODE_RESIZE_STORM_OFFSET {
        unicode.send(UnicodeAction::ShiftRight);
    }
    let capacities = unicode
        .application()
        .model()
        .rows()
        .iter()
        .map(String::capacity)
        .collect::<Vec<_>>();
    assert_storm_history_bounded(&mut unicode, &UNICODE_RESIZE_STORM);
    assert_eq!(
        unicode.application().model().offset(),
        UNICODE_RESIZE_STORM_OFFSET
    );
    assert_eq!(
        unicode
            .application()
            .model()
            .rows()
            .iter()
            .map(String::capacity)
            .collect::<Vec<_>>(),
        capacities
    );
}

#[test]
fn navigation_and_reset_turns_do_not_retain_history() {
    for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
        let mut app = measurement_app(CollectionLab::new(mode, 1_000, 8), Size::new(48, 12));
        app.send(Message::Down);
        app.send(Message::SelectActive);
        app.send(Message::Home);
        let initial = app.frame().clone();
        for _ in 0..100 {
            for message in [
                Message::Down,
                Message::Up,
                Message::PageDown,
                Message::Home,
                Message::End,
                Message::Home,
                Message::SelectActive,
                Message::Down,
                Message::SelectActive,
                Message::Home,
                Message::Reverse,
                Message::Reverse,
            ] {
                app.send(message);
                assert_no_history(&app);
            }
            assert_eq!(app.frame(), &initial);
            assert_eq!(app.application().active_key(), Some(0));
            assert_eq!(app.application().selected_key(), Some(1));
            assert!(app.application().constructed_rows() <= 12);
        }
    }

    let mut table = measurement_app(TableLab::new(1_000, 48, 12), Size::new(48, 12));
    for revision in 1..=100 {
        for action in [
            TableAction::Down,
            TableAction::Up,
            TableAction::PageDown,
            TableAction::Home,
            TableAction::SelectActive,
            TableAction::Down,
            TableAction::SelectActive,
            TableAction::Home,
            TableAction::BackgroundUpdate { key: 0, revision },
            TableAction::BackgroundUpdate { key: 999, revision },
        ] {
            table.send(action);
            assert_no_history(&table);
        }
        assert_eq!(table.application().model().rows().len(), 1_000);
        assert_eq!(table.application().model().active_key(), Some(0));
        assert!(table.application().constructed_rows() <= 12);
    }

    // Appending before eviction grows the source deque once; warm that high-water
    // capacity, then prove neither source history nor instrumentation keeps growing.
    let mut log = measurement_app(LogLab::new(128, 128, 48, 12), Size::new(48, 12));
    log.send(LogAction::Append {
        count: 1,
        generation: 0,
    });
    let capacity = log.application().model().records().capacity();
    for generation in 1..=100 {
        for action in [
            LogAction::Up,
            LogAction::Down,
            LogAction::PageUp,
            LogAction::End,
            LogAction::Append {
                count: 1,
                generation,
            },
            LogAction::PageUp,
            LogAction::Append {
                count: 1,
                generation,
            },
            LogAction::End,
        ] {
            log.send(action);
            assert_no_history(&log);
            assert_eq!(log.application().model().records().len(), 128);
            assert_eq!(log.application().model().records().capacity(), capacity);
            assert!(log.application().constructed_rows() <= 12);
        }
    }
}

#[test]
fn focus_and_unicode_reset_turns_do_not_retain_history() {
    let mut overlay = measurement_app(OverlayLab::new(40, 12), Size::new(40, 12));
    let initial = overlay.frame().clone();
    let focus = overlay.focused_key();
    for _ in 0..100 {
        overlay.send(OverlayAction::Open);
        overlay.key(KeyCode::Tab);
        overlay.key(KeyCode::Tab);
        overlay.resize(Size::new(44, 14));
        overlay.resize(Size::new(40, 12));
        overlay.send(OverlayAction::Cancel);
        assert_no_history(&overlay);
        assert_eq!(overlay.frame(), &initial);
        assert_eq!(overlay.focused_key(), focus);
        assert!(!overlay.application().model().dialog_open());
    }

    let mut unicode = measurement_app(UnicodeLab::new(36, 10), Size::new(36, 10));
    for _ in 0..15 {
        unicode.send(UnicodeAction::ShiftRight);
    }
    // Warm both replacement strings before checking their steady-state capacity.
    unicode.send(UnicodeAction::ReplaceWide);
    unicode.send(UnicodeAction::ReplaceWide);
    let initial = unicode.frame().clone();
    let capacities = unicode
        .application()
        .model()
        .rows()
        .iter()
        .map(String::capacity)
        .collect::<Vec<_>>();
    for _ in 0..100 {
        for action in [
            UnicodeAction::ShiftRight,
            UnicodeAction::ShiftLeft,
            UnicodeAction::ReplaceWide,
            UnicodeAction::ReplaceWide,
        ] {
            unicode.send(action);
            assert_no_history(&unicode);
        }
        unicode.resize(Size::new(30, 10));
        unicode.resize(Size::new(36, 10));
        assert_no_history(&unicode);
        assert_eq!(unicode.frame(), &initial);
        assert_eq!(unicode.application().model().offset(), 15);
        assert_eq!(
            unicode
                .application()
                .model()
                .rows()
                .iter()
                .map(String::capacity)
                .collect::<Vec<_>>(),
            capacities
        );
    }
}

fn measurement_app<A: Application>(application: A, size: Size) -> TestApp<A> {
    let app = TestApp::with_options(
        application,
        size,
        TestAppOptions {
            record_patches: false,
            ..TestAppOptions::default()
        },
    );
    assert_no_history(&app);
    app
}

fn assert_no_history<A: Application>(app: &TestApp<A>) {
    assert!(app.frame_patches().is_empty());
    assert!(app.last_frame_patch().is_none());
    let size = app.frame().size();
    assert_eq!(
        app.frame().cells().len(),
        usize::from(size.width) * usize::from(size.height)
    );
}

fn assert_storm_history_bounded<A: Application>(app: &mut TestApp<A>, storm: &[(u16, u16)]) {
    let initial = app.frame().clone();
    let focus = app.focused_key();
    for _ in 0..100 {
        for &(width, height) in storm {
            let report = app.resize(Size::new(width, height));
            assert_eq!(report.outcome, SettleOutcome::Settled);
            assert_no_history(app);
        }
        assert_eq!(app.frame(), &initial);
        assert_eq!(app.focused_key(), focus);
    }
}
