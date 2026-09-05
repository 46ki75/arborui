//! Matched semantic and character-frame contracts, plus complete overlay cell styles.

#[path = "support/overlay_cells.rs"]
mod overlay_cells;

use arborui_example_collection_lab::{
    CollectionLab, CollectionMode, LogAction, LogLab, Message, OVERLAY_BACKGROUND_KEY,
    OVERLAY_CANCEL_KEY, OVERLAY_CONFIRM_KEY, OVERLAY_OPEN_KEY, OverlayAction, OverlayLab,
    TableAction, TableLab, UnicodeAction, UnicodeLab,
};
use arborui_test::{Key, KeyCode, KeyEventKind, KeyModifiers, Size, TestApp};
use ratatui::{Terminal, backend::TestBackend};

use arborui_comparison_collection_lab_ratatui::{
    ComparisonAction, CountingBackend, LogSemanticState, OVERLAY_RESIZE_STORM, OverlayFocus,
    OverlaySemanticState, RatatuiCollectionLab, RatatuiLogLab, RatatuiOverlayLab, RatatuiTableLab,
    RatatuiUnicodeLab, STANDARD_RESIZE_STORM, SemanticState, TableSemanticState,
    UNICODE_RESIZE_STORM, UNICODE_RESIZE_STORM_OFFSET, UnicodeSemanticState, draw_overlay_terminal,
    draw_terminal, draw_test_frame, draw_test_log_frame, draw_test_overlay_frame,
    draw_test_table_frame, draw_test_unicode_frame,
};

#[test]
fn resize_storm_matches_semantics_and_characters_after_every_frame() {
    for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
        let mut arborui = TestApp::new(CollectionLab::new(mode, 100_000, 8), Size::new(48, 12));
        let mut ratatui = RatatuiCollectionLab::new(mode, 100_000, 48, 12);
        let mut terminal =
            Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
        apply_arborui(&mut arborui, ComparisonAction::SelectActive);
        ratatui.apply(ComparisonAction::SelectActive);
        let baseline = arborui_state(&arborui);
        for _ in 0..2 {
            for (width, height) in STANDARD_RESIZE_STORM {
                apply_arborui(&mut arborui, ComparisonAction::Resize { width, height });
                ratatui.apply(ComparisonAction::Resize { width, height });
                terminal.backend_mut().resize(width, height);
                let frame = draw_test_frame(&mut terminal, &mut ratatui)
                    .expect("collection frame must draw");
                assert_eq!(arborui.frame().size(), Size::new(width, height));
                assert_eq!(arborui_state(&arborui), ratatui.semantic_state());
                assert_eq!(arborui.frame().characters(), frame);
            }
            assert_eq!(arborui_state(&arborui), baseline);
        }
    }

    let mut arborui = TestApp::new(TableLab::new(100_000, 48, 12), Size::new(48, 12));
    let mut ratatui = RatatuiTableLab::new(100_000, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
    apply_arborui_table(&mut arborui, TableAction::SelectActive);
    ratatui.apply(TableAction::SelectActive);
    let baseline = arborui_table_state(&arborui);
    for _ in 0..2 {
        for (width, height) in STANDARD_RESIZE_STORM {
            let action = TableAction::Resize { width, height };
            apply_arborui_table(&mut arborui, action);
            ratatui.apply(action);
            terminal.backend_mut().resize(width, height);
            assert_eq!(arborui.frame().size(), Size::new(width, height));
            assert_table_frame(&arborui, &mut ratatui, &mut terminal);
        }
        assert_eq!(arborui_table_state(&arborui), baseline);
    }

    let mut arborui = TestApp::new(LogLab::new(100_000, 200_000, 48, 12), Size::new(48, 12));
    let mut ratatui = RatatuiLogLab::new(100_000, 200_000, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
    apply_arborui_log(&mut arborui, LogAction::PageUp);
    ratatui.apply(LogAction::PageUp);
    let baseline = arborui_log_state(&arborui);
    for _ in 0..2 {
        for (width, height) in STANDARD_RESIZE_STORM {
            let action = LogAction::Resize { width, height };
            apply_arborui_log(&mut arborui, action);
            ratatui.apply(action);
            terminal.backend_mut().resize(width, height);
            assert_eq!(arborui.frame().size(), Size::new(width, height));
            assert_log_frame(&arborui, &mut ratatui, &mut terminal);
            assert!(!ratatui.semantic_state().follows_tail);
        }
        assert_eq!(arborui_log_state(&arborui), baseline);
    }

    let mut arborui = TestApp::new(OverlayLab::new(40, 12), Size::new(40, 12));
    let mut ratatui = RatatuiOverlayLab::new(40, 12);
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("test terminal must open");
    arborui.key(KeyCode::Enter);
    arborui.key(KeyCode::Tab);
    ratatui.apply(OverlayAction::Open);
    ratatui.focus_next();
    let baseline = arborui_overlay_state(&arborui);
    for _ in 0..2 {
        for (width, height) in OVERLAY_RESIZE_STORM {
            arborui.resize(Size::new(width, height));
            ratatui.apply(OverlayAction::Resize { width, height });
            terminal.backend_mut().resize(width, height);
            assert_eq!(arborui.frame().size(), Size::new(width, height));
            assert_overlay_frame(&arborui, &ratatui, &mut terminal);
            assert_eq!(ratatui.semantic_state().focus, OverlayFocus::Cancel);
        }
        assert_eq!(arborui_overlay_state(&arborui), baseline);
    }

    let mut arborui = TestApp::new(UnicodeLab::new(36, 10), Size::new(36, 10));
    let mut ratatui = RatatuiUnicodeLab::new(36, 10);
    let mut terminal = Terminal::new(TestBackend::new(36, 10)).expect("test terminal must open");
    for _ in 0..UNICODE_RESIZE_STORM_OFFSET {
        arborui.send(UnicodeAction::ShiftRight);
        ratatui.apply(UnicodeAction::ShiftRight);
    }
    let baseline = arborui_unicode_state(&arborui);
    for _ in 0..2 {
        for (width, height) in UNICODE_RESIZE_STORM {
            arborui.resize(Size::new(width, height));
            ratatui.apply(UnicodeAction::Resize { width, height });
            terminal.backend_mut().resize(width, height);
            assert_eq!(arborui.frame().size(), Size::new(width, height));
            assert_unicode_frame(&arborui, &ratatui, &mut terminal);
        }
        assert_eq!(arborui_unicode_state(&arborui), baseline);
    }
}

#[test]
fn canonical_unicode_trace_matches_semantics_and_characters() {
    let mut arborui = TestApp::new(UnicodeLab::new(36, 10), Size::new(36, 10));
    let mut ratatui = RatatuiUnicodeLab::new(36, 10);
    let mut terminal = Terminal::new(TestBackend::new(36, 10)).expect("test terminal must open");

    assert_unicode_frame(&arborui, &ratatui, &mut terminal);
    for _ in 0..16 {
        arborui.send(UnicodeAction::ShiftRight);
        ratatui.apply(UnicodeAction::ShiftRight);
        assert_unicode_frame(&arborui, &ratatui, &mut terminal);
    }

    arborui.send(UnicodeAction::ReplaceWide);
    ratatui.apply(UnicodeAction::ReplaceWide);
    assert_unicode_frame(&arborui, &ratatui, &mut terminal);

    arborui.resize(Size::new(30, 10));
    ratatui.apply(UnicodeAction::Resize {
        width: 30,
        height: 10,
    });
    terminal.backend_mut().resize(30, 10);
    assert_unicode_frame(&arborui, &ratatui, &mut terminal);

    arborui.resize(Size::new(42, 12));
    ratatui.apply(UnicodeAction::Resize {
        width: 42,
        height: 12,
    });
    terminal.backend_mut().resize(42, 12);
    assert_unicode_frame(&arborui, &ratatui, &mut terminal);
}

#[test]
fn canonical_overlay_trace_matches_semantics_characters_and_styles() {
    let mut arborui = TestApp::new(OverlayLab::new(40, 12), Size::new(40, 12));
    let mut ratatui = RatatuiOverlayLab::new(40, 12);
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("test terminal must open");

    assert_overlay_frame(&arborui, &ratatui, &mut terminal);

    for _ in 0..2 {
        arborui.key(KeyCode::Tab);
        ratatui.focus_next();
        assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    }
    for _ in 0..2 {
        arborui.key_with(KeyCode::Tab, KeyModifiers::SHIFT, KeyEventKind::Press);
        ratatui.focus_previous();
        assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    }

    arborui.key(KeyCode::Enter);
    ratatui.apply(OverlayAction::Open);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);

    arborui.key(KeyCode::Tab);
    ratatui.focus_next();
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    arborui.key(KeyCode::Tab);
    ratatui.focus_next();
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    arborui.key_with(KeyCode::Tab, KeyModifiers::SHIFT, KeyEventKind::Press);
    ratatui.focus_previous();
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);

    for _ in 0..2 {
        for (width, height) in OVERLAY_RESIZE_STORM {
            arborui.resize(Size::new(width, height));
            ratatui.apply(OverlayAction::Resize { width, height });
            terminal.backend_mut().resize(width, height);
            assert_overlay_frame(&arborui, &ratatui, &mut terminal);
        }
        arborui.key_with(KeyCode::Tab, KeyModifiers::SHIFT, KeyEventKind::Press);
        ratatui.focus_previous();
        assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    }

    assert_eq!(arborui.application().model().background_activations(), 0);
    assert_eq!(ratatui.model().background_activations(), 0);

    arborui.key(KeyCode::Escape);
    ratatui.apply(OverlayAction::Cancel);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);

    arborui.key(KeyCode::Enter);
    ratatui.apply(OverlayAction::Open);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    arborui.key(KeyCode::Enter);
    ratatui.apply(OverlayAction::Confirm);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    assert_eq!(ratatui.semantic_state().confirmations, 1);

    arborui.resize(Size::new(44, 14));
    ratatui.apply(OverlayAction::Resize {
        width: 44,
        height: 14,
    });
    terminal.backend_mut().resize(44, 14);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);

    arborui.key(KeyCode::Tab);
    ratatui.focus_next();
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    arborui.resize(Size::new(40, 12));
    ratatui.apply(OverlayAction::Resize {
        width: 40,
        height: 12,
    });
    terminal.backend_mut().resize(40, 12);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    arborui.key(KeyCode::Enter);
    ratatui.apply(OverlayAction::ActivateBackground);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    assert_eq!(ratatui.semantic_state().background_activations, 1);
    arborui.key(KeyCode::Tab);
    ratatui.focus_next();
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);

    arborui.key(KeyCode::Enter);
    ratatui.apply(OverlayAction::Open);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    arborui.resize(Size::new(44, 14));
    ratatui.apply(OverlayAction::Resize {
        width: 44,
        height: 14,
    });
    terminal.backend_mut().resize(44, 14);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    arborui.key(KeyCode::Tab);
    ratatui.focus_next();
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    arborui.key(KeyCode::Enter);
    ratatui.apply(OverlayAction::Cancel);
    assert_overlay_frame(&arborui, &ratatui, &mut terminal);
    assert_eq!(ratatui.semantic_state().focus, OverlayFocus::Open);
    assert_eq!(ratatui.semantic_state().confirmations, 1);
}

#[test]
fn overlay_focus_next_changes_twenty_styled_cells_on_both_sides() {
    let mut arborui = TestApp::new(OverlayLab::new(40, 12), Size::new(40, 12));
    let mut ratatui = RatatuiOverlayLab::new(40, 12);
    let mut terminal =
        Terminal::new(CountingBackend::new(40, 12)).expect("counting terminal must open");
    arborui.key(KeyCode::Enter);
    ratatui.apply(OverlayAction::Open);
    draw_overlay_terminal(&mut terminal, &ratatui).expect("open frame must draw");
    terminal.backend_mut().reset_counts();
    let previous = arborui.frame().clone();
    let patch_count = arborui.frame_patches().len();

    arborui.key(KeyCode::Tab);
    ratatui.focus_next();
    draw_overlay_terminal(&mut terminal, &ratatui).expect("focus frame must draw");

    assert_eq!(arborui_overlay_state(&arborui), ratatui.semantic_state());
    assert_eq!(ratatui.semantic_state().focus, OverlayFocus::Cancel);
    assert_eq!(previous.characters(), arborui.frame().characters());
    let changed_cells = previous
        .cells()
        .iter()
        .zip(arborui.frame().cells())
        .filter(|(previous, current)| {
            overlay_cells::arborui_cell(previous) != overlay_cells::arborui_cell(current)
        })
        .count();
    assert_eq!(changed_cells, 20);
    let submitted_cells = arborui.frame_patches()[patch_count..]
        .iter()
        .flat_map(|patch| &patch.runs)
        .map(|run| run.cells.len())
        .sum::<usize>();
    assert_eq!(submitted_cells, 20);
    assert_eq!(
        terminal.backend().changed_cells(),
        changed_cells,
        "Ratatui focus-next must submit the same 20 visibly changed cells as ArborUI"
    );
    assert_eq!(terminal.backend().draws(), 1);
    assert_eq!(terminal.backend().flushes(), 1);
}

#[test]
fn canonical_scrolling_log_trace_matches_semantics_and_characters() {
    let mut arborui = TestApp::new(LogLab::new(100, 110, 48, 12), Size::new(48, 12));
    let mut ratatui = RatatuiLogLab::new(100, 110, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");

    assert_log_frame(&arborui, &mut ratatui, &mut terminal);
    for action in [
        LogAction::PageUp,
        LogAction::Append {
            count: 2,
            generation: 1,
        },
        LogAction::End,
        LogAction::Append {
            count: 12,
            generation: 2,
        },
        LogAction::Resize {
            width: 38,
            height: 10,
        },
        LogAction::Home,
        LogAction::Down,
    ] {
        apply_arborui_log(&mut arborui, action);
        ratatui.apply(action);
        if let LogAction::Resize { width, height } = action {
            terminal.backend_mut().resize(width, height);
        }
        assert_log_frame(&arborui, &mut ratatui, &mut terminal);
    }

    assert_eq!(ratatui.semantic_state().retained_records, 110);
    assert_eq!(ratatui.semantic_state().generation, 2);
    assert!(!ratatui.semantic_state().follows_tail);
}

#[test]
fn scrolling_log_construction_is_bounded_at_one_million_records() {
    let arborui = TestApp::new(LogLab::new(1_000_000, 1_000_000, 48, 12), Size::new(48, 12));
    let mut ratatui = RatatuiLogLab::new(1_000_000, 1_000_000, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
    let frame = draw_test_log_frame(&mut terminal, &mut ratatui)
        .expect("Ratatui scrolling-log frame must draw");

    assert_eq!(arborui.application().constructed_rows(), 10);
    assert_eq!(ratatui.semantic_state().constructed_rows, 10);
    assert_eq!(arborui_log_state(&arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), frame);
    assert!(frame.contains("Δelta"));
}

#[test]
fn canonical_table_trace_matches_semantics_and_characters() {
    let mut arborui = TestApp::new(TableLab::new(100_000, 48, 12), Size::new(48, 12));
    let mut ratatui = RatatuiTableLab::new(100_000, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");

    assert_table_frame(&arborui, &mut ratatui, &mut terminal);
    for action in [
        TableAction::PageDown,
        TableAction::Down,
        TableAction::SelectActive,
        TableAction::BackgroundUpdate {
            key: 8,
            revision: 1,
        },
        TableAction::BackgroundUpdate {
            key: 99_999,
            revision: 2,
        },
        TableAction::Resize {
            width: 34,
            height: 9,
        },
        TableAction::Resize {
            width: 64,
            height: 15,
        },
    ] {
        apply_arborui_table(&mut arborui, action);
        ratatui.apply(action);
        if let TableAction::Resize { width, height } = action {
            terminal.backend_mut().resize(width, height);
        }
        assert_table_frame(&arborui, &mut ratatui, &mut terminal);
    }

    assert_eq!(ratatui.semantic_state().selected_key, Some(8));
    assert_eq!(ratatui.semantic_state().generation, 2);
    assert_eq!(ratatui.model().rows()[8].revision(), 1);
    assert_eq!(ratatui.model().rows()[99_999].revision(), 2);
}

#[test]
fn table_construction_is_bounded_and_unicode_is_visible() {
    let arborui = TestApp::new(TableLab::new(1_000_000, 64, 12), Size::new(64, 12));
    let mut ratatui = RatatuiTableLab::new(1_000_000, 64, 12);
    let mut terminal = Terminal::new(TestBackend::new(64, 12)).expect("test terminal must open");
    let frame =
        draw_test_table_frame(&mut terminal, &mut ratatui).expect("Ratatui table frame must draw");

    assert_eq!(arborui.application().constructed_rows(), 9);
    assert_eq!(ratatui.semantic_state().constructed_rows, 9);
    assert_eq!(arborui_table_state(&arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), frame);
    assert!(frame.contains("München"));
}

#[test]
fn canonical_variable_trace_has_matching_semantics_and_characters() {
    let size = Size::new(38, 11);
    let mut arborui = TestApp::new(
        CollectionLab::new(CollectionMode::Variable, 100_000, 7),
        size,
    );
    let mut ratatui = RatatuiCollectionLab::new(CollectionMode::Variable, 100_000, 38, 11);
    let mut terminal = Terminal::new(TestBackend::new(38, 11)).expect("test terminal must open");

    for action in [
        ComparisonAction::PageDown,
        ComparisonAction::Down,
        ComparisonAction::SelectActive,
    ] {
        apply_arborui(&mut arborui, action);
        ratatui.apply(action);
    }
    let ratatui_frame =
        draw_test_frame(&mut terminal, &mut ratatui).expect("Ratatui frame must draw");

    assert_eq!(arborui_state(&arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), ratatui_frame);
}

#[test]
fn isolated_scenarios_have_matching_semantics_and_characters() {
    for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
        for action in [
            ComparisonAction::PageDown,
            ComparisonAction::End,
            ComparisonAction::SelectActive,
            ComparisonAction::Reverse,
            ComparisonAction::Resize {
                width: 48,
                height: 16,
            },
        ] {
            assert_isolated_scenario(mode, action);
        }
    }
}

#[test]
fn construction_is_bounded_at_one_million_rows() {
    let size = Size::new(48, 12);
    let arborui = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 1_000_000, 8),
        size,
    );
    let mut ratatui = RatatuiCollectionLab::new(CollectionMode::Fixed, 1_000_000, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");

    draw_test_frame(&mut terminal, &mut ratatui).expect("Ratatui frame must draw");

    assert_eq!(arborui.application().constructed_rows(), 10);
    assert_eq!(ratatui.semantic_state().constructed_rows, 10);
    assert_eq!(arborui_state(&arborui), ratatui.semantic_state());
}

#[test]
fn stable_identity_survives_unmount_and_reverse() {
    let size = Size::new(40, 10);
    let mut arborui = TestApp::new(CollectionLab::new(CollectionMode::Fixed, 100, 6), size);
    let mut ratatui = RatatuiCollectionLab::new(CollectionMode::Fixed, 100, 40, 10);
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("test terminal must open");

    for action in [
        ComparisonAction::SelectActive,
        ComparisonAction::End,
        ComparisonAction::Reverse,
        ComparisonAction::Home,
    ] {
        apply_arborui(&mut arborui, action);
        ratatui.apply(action);
    }
    draw_test_frame(&mut terminal, &mut ratatui).expect("Ratatui frame must draw");

    assert_eq!(arborui_state(&arborui), ratatui.semantic_state());
    assert_eq!(ratatui.semantic_state().active_key, Some(99));
    assert_eq!(ratatui.semantic_state().selected_key, Some(0));
}

#[test]
fn unchanged_redraw_has_no_logical_output_and_idle_does_no_work() {
    let mut arborui = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 1_000_000, 8),
        Size::new(48, 12),
    );
    let mut application = RatatuiCollectionLab::new(CollectionMode::Fixed, 1_000_000, 48, 12);
    let mut terminal =
        Terminal::new(CountingBackend::new(48, 12)).expect("counting terminal must open");
    draw_terminal(&mut terminal, &mut application).expect("initial frame must draw");
    terminal.backend_mut().reset_counts();
    let patch_count = arborui.frame_patches().len();

    arborui.send(Message::Home);
    application.apply(ComparisonAction::Home);
    draw_terminal(&mut terminal, &mut application).expect("unchanged frame must draw");

    assert_eq!(arborui.frame_patches().len(), patch_count);
    assert_eq!(terminal.backend().changed_cells(), 0);
    assert_eq!(terminal.backend().draws(), 1);
    assert_eq!(terminal.backend().flushes(), 1);
    terminal.backend_mut().reset_counts();

    assert_eq!(terminal.backend().changed_cells(), 0);
    assert_eq!(terminal.backend().draws(), 0);
    assert_eq!(terminal.backend().flushes(), 0);
}

#[test]
fn one_row_navigation_reports_logical_output_work() {
    let mut arborui = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 1_000_000, 8),
        Size::new(48, 12),
    );
    let mut ratatui = RatatuiCollectionLab::new(CollectionMode::Fixed, 1_000_000, 48, 12);
    let mut terminal =
        Terminal::new(CountingBackend::new(48, 12)).expect("counting terminal must open");
    draw_terminal(&mut terminal, &mut ratatui).expect("initial frame must draw");
    terminal.backend_mut().reset_counts();

    let patch_count = arborui.frame_patches().len();
    arborui.send(Message::Down);
    ratatui.apply(ComparisonAction::Down);
    draw_terminal(&mut terminal, &mut ratatui).expect("updated frame must draw");

    let patch = &arborui.frame_patches()[patch_count];
    let arborui_cells = patch.runs.iter().map(|run| run.cells.len()).sum::<usize>();
    assert!(arborui_cells > 0);
    assert!(terminal.backend().changed_cells() > 0);
    assert_eq!(terminal.backend().draws(), 1);
    assert_eq!(terminal.backend().flushes(), 1);
}

#[test]
fn resize_recomputes_the_same_window() {
    let mut arborui = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 10_000, 4),
        Size::new(40, 8),
    );
    let mut ratatui = RatatuiCollectionLab::new(CollectionMode::Fixed, 10_000, 40, 8);
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("test terminal must open");
    draw_test_frame(&mut terminal, &mut ratatui).expect("initial frame must draw");

    apply_arborui(
        &mut arborui,
        ComparisonAction::Resize {
            width: 40,
            height: 14,
        },
    );
    ratatui.apply(ComparisonAction::Resize {
        width: 40,
        height: 14,
    });
    terminal.backend_mut().resize(40, 14);
    let ratatui_frame =
        draw_test_frame(&mut terminal, &mut ratatui).expect("resized frame must draw");

    assert_eq!(arborui_state(&arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), ratatui_frame);
}

fn assert_isolated_scenario(mode: CollectionMode, action: ComparisonAction) {
    let initial_size = Size::new(48, 12);
    let mut arborui = TestApp::new(CollectionLab::new(mode, 100_000, 8), initial_size);
    let mut ratatui = RatatuiCollectionLab::new(mode, 100_000, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
    let initial_ratatui =
        draw_test_frame(&mut terminal, &mut ratatui).expect("initial frame must draw");

    assert_eq!(arborui_state(&arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), initial_ratatui);

    apply_arborui(&mut arborui, action);
    ratatui.apply(action);
    if let ComparisonAction::Resize { width, height } = action {
        terminal.backend_mut().resize(width, height);
    }
    let ratatui_frame =
        draw_test_frame(&mut terminal, &mut ratatui).expect("scenario frame must draw");

    assert_eq!(arborui_state(&arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), ratatui_frame);
}

fn arborui_state(app: &TestApp<CollectionLab>) -> SemanticState {
    SemanticState {
        active_key: app.application().active_key(),
        selected_key: app.application().selected_key(),
        scroll_offset: app.application().scroll_offset(),
        viewport_height: app.application().viewport_height(),
        visible_range: app.application().visible_range(),
        constructed_rows: app.application().constructed_rows(),
    }
}

fn apply_arborui(app: &mut TestApp<CollectionLab>, action: ComparisonAction) {
    let message = match action {
        ComparisonAction::Up => Message::Up,
        ComparisonAction::Down => Message::Down,
        ComparisonAction::Home => Message::Home,
        ComparisonAction::End => Message::End,
        ComparisonAction::PageUp => Message::PageUp,
        ComparisonAction::PageDown => Message::PageDown,
        ComparisonAction::SelectActive => Message::SelectActive,
        ComparisonAction::ToggleMode => Message::ToggleMode,
        ComparisonAction::Reverse => Message::Reverse,
        ComparisonAction::Resize { width, height } => {
            app.resize(Size::new(width, height));
            return;
        }
    };
    app.send(message);
}

fn assert_table_frame(
    arborui: &TestApp<TableLab>,
    ratatui: &mut RatatuiTableLab,
    terminal: &mut Terminal<TestBackend>,
) {
    let frame = draw_test_table_frame(terminal, ratatui).expect("Ratatui table frame must draw");
    assert_eq!(arborui_table_state(arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), frame);
}

fn arborui_table_state(app: &TestApp<TableLab>) -> TableSemanticState {
    TableSemanticState {
        active_key: app.application().model().active_key(),
        selected_key: app.application().model().selected_key(),
        scroll_offset: app.application().model().scroll_offset(),
        viewport_height: app.application().model().viewport_height(),
        visible_range: app.application().model().visible_range(),
        constructed_rows: app.application().constructed_rows(),
        generation: app.application().model().generation(),
    }
}

fn apply_arborui_table(app: &mut TestApp<TableLab>, action: TableAction) {
    if let TableAction::Resize { width, height } = action {
        app.resize(Size::new(width, height));
    } else {
        app.send(action);
    }
}

fn assert_log_frame(
    arborui: &TestApp<LogLab>,
    ratatui: &mut RatatuiLogLab,
    terminal: &mut Terminal<TestBackend>,
) {
    let frame =
        draw_test_log_frame(terminal, ratatui).expect("Ratatui scrolling-log frame must draw");
    assert_eq!(arborui_log_state(arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), frame);
}

fn arborui_log_state(app: &TestApp<LogLab>) -> LogSemanticState {
    LogSemanticState {
        scroll_offset: app.application().model().scroll_offset(),
        follows_tail: app.application().model().follows_tail(),
        viewport_height: app.application().model().viewport_height(),
        visible_range: app.application().model().visible_range(),
        constructed_rows: app.application().constructed_rows(),
        retained_records: app.application().model().records().len(),
        generation: app.application().model().generation(),
    }
}

fn apply_arborui_log(app: &mut TestApp<LogLab>, action: LogAction) {
    if let LogAction::Resize { width, height } = action {
        app.resize(Size::new(width, height));
    } else {
        app.send(action);
    }
}

fn assert_overlay_frame(
    arborui: &TestApp<OverlayLab>,
    ratatui: &RatatuiOverlayLab,
    terminal: &mut Terminal<TestBackend>,
) {
    let frame =
        draw_test_overlay_frame(terminal, ratatui).expect("Ratatui overlay frame must draw");
    assert_eq!(arborui_overlay_state(arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), frame);
    let expected = arborui.frame().cells();
    let actual = &terminal.backend().buffer().content;
    assert_eq!(expected.len(), actual.len());
    let differences = expected
        .iter()
        .zip(actual)
        .enumerate()
        .filter_map(|(index, (expected, actual))| {
            let expected = overlay_cells::arborui_cell(expected);
            let actual = overlay_cells::ratatui_cell(actual);
            (expected != actual).then_some((
                index % usize::from(arborui.frame().size().width),
                index / usize::from(arborui.frame().size().width),
                expected,
                actual,
            ))
        })
        .collect::<Vec<_>>();
    assert!(
        differences.is_empty(),
        "{} overlay styled-cell differences at {:?}; first cells (x, y, ArborUI, Ratatui): {:#?}",
        differences.len(),
        ratatui.semantic_state(),
        &differences[..differences.len().min(8)]
    );
}

fn arborui_overlay_state(app: &TestApp<OverlayLab>) -> OverlaySemanticState {
    let focus = match app.focused_key() {
        Some(key) if key == Key::from(OVERLAY_OPEN_KEY) => OverlayFocus::Open,
        Some(key) if key == Key::from(OVERLAY_BACKGROUND_KEY) => OverlayFocus::Background,
        Some(key) if key == Key::from(OVERLAY_CONFIRM_KEY) => OverlayFocus::Confirm,
        Some(key) if key == Key::from(OVERLAY_CANCEL_KEY) => OverlayFocus::Cancel,
        key => panic!("unexpected overlay focus: {key:?}"),
    };
    OverlaySemanticState {
        dialog_open: app.application().model().dialog_open(),
        confirmations: app.application().model().confirmations(),
        background_activations: app.application().model().background_activations(),
        focus,
    }
}

fn assert_unicode_frame(
    arborui: &TestApp<UnicodeLab>,
    ratatui: &RatatuiUnicodeLab,
    terminal: &mut Terminal<TestBackend>,
) {
    let frame =
        draw_test_unicode_frame(terminal, ratatui).expect("Ratatui Unicode frame must draw");
    assert_eq!(arborui_unicode_state(arborui), ratatui.semantic_state());
    assert_eq!(arborui.frame().characters(), frame);
}

fn arborui_unicode_state(app: &TestApp<UnicodeLab>) -> UnicodeSemanticState {
    UnicodeSemanticState {
        offset: app.application().model().offset(),
        replacement_is_wide: app.application().model().replacement_is_wide(),
        terminal_size: app.application().model().terminal_size(),
    }
}
