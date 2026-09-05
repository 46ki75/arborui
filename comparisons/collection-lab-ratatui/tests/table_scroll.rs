//! Controlled table scrolling must preserve semantic and rendered viewport parity.

use arborui::{Color as ArborColor, Modifier as ArborModifier};
use arborui_comparison_collection_lab_ratatui::{
    RatatuiTableLab, TableSemanticState, draw_test_table_frame,
};
use arborui_example_collection_lab::{TableAction, TableLab};
use arborui_test::{KeyCode, KeyModifiers, MouseEvent, MouseEventKind, Point, Size, TestApp};
use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Modifier},
};

#[test]
fn controlled_scroll_keeps_active_upper_overscan_row_offscreen() {
    let mut arborui = TestApp::new(TableLab::new(100, 48, 12), Size::new(48, 12));
    let mut ratatui = RatatuiTableLab::new(100, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
    assert_table_frame(&arborui, &mut ratatui, &mut terminal, 0..7);

    let action = TableAction::Scrolled(Point::new(0, 1));
    arborui.send(action);
    ratatui.apply(action);

    assert_eq!(arborui.application().model().scroll_offset(), 1);
    assert_eq!(ratatui.model().scroll_offset(), 1);
    assert_eq!(arborui.application().model().active_key(), Some(0));
    assert_eq!(arborui.application().model().selected_key(), None);
    assert_table_frame(&arborui, &mut ratatui, &mut terminal, 1..8);
}

#[test]
fn scrolling_past_both_overscan_margins_restores_selection_highlight() {
    // Each trace crosses both overscan rows, leaves the constructed range, and returns.
    for (active, offsets) in [
        (0, [1, 2, 3, 20, 3, 2, 1, 0]),
        (50, [43, 42, 41, 0, 41, 42, 43, 44]),
    ] {
        let mut arborui = TestApp::new(TableLab::new(100, 48, 12), Size::new(48, 12));
        let mut ratatui = RatatuiTableLab::new(100, 48, 12);
        let mut terminal =
            Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
        arborui.send(TableAction::Select(active));
        ratatui.apply(TableAction::Select(active));
        let initial_offset = active.saturating_sub(6) as usize;
        assert_table_frame(
            &arborui,
            &mut ratatui,
            &mut terminal,
            initial_offset..initial_offset + 7,
        );

        for offset in offsets {
            let delta = offset as i32 - arborui.application().model().scroll_offset() as i32;
            let action = TableAction::Scrolled(Point::new(0, delta));
            arborui.send(action);
            ratatui.apply(action);

            assert_eq!(arborui.application().model().scroll_offset(), offset);
            assert_eq!(arborui.application().model().active_key(), Some(active));
            assert_eq!(arborui.application().model().selected_key(), Some(active));
            assert_table_frame(&arborui, &mut ratatui, &mut terminal, offset..offset + 7);
        }
    }
}

#[test]
fn public_mouse_wheel_keeps_selection_offscreen_until_scrolled_back() {
    let mut arborui = TestApp::new(TableLab::new(100, 48, 12), Size::new(48, 12));
    let mut ratatui = RatatuiTableLab::new(100, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
    arborui.key(KeyCode::Enter);
    ratatui.apply(TableAction::SelectActive);
    assert_table_frame(&arborui, &mut ratatui, &mut terminal, 0..7);

    for (delta, offset) in [(1, 1), (1, 2), (1, 3), (-1, 2), (-1, 1), (-1, 0)] {
        arborui.mouse(MouseEvent {
            kind: if delta > 0 {
                MouseEventKind::ScrollDown
            } else {
                MouseEventKind::ScrollUp
            },
            position: Point::new(2, 4),
            modifiers: KeyModifiers::NONE,
        });
        ratatui.apply(TableAction::Scrolled(Point::new(0, delta)));

        assert_eq!(arborui.application().model().scroll_offset(), offset);
        assert_eq!(arborui.application().model().active_key(), Some(0));
        assert_eq!(arborui.application().model().selected_key(), Some(0));
        assert_table_frame(&arborui, &mut ratatui, &mut terminal, offset..offset + 7);
    }
}

#[test]
fn empty_and_short_tables_match_including_a_one_row_viewport() {
    for height in [6, 12] {
        for count in [0, 1, 3] {
            let mut arborui = TestApp::new(TableLab::new(count, 48, height), Size::new(48, height));
            let mut ratatui = RatatuiTableLab::new(count, 48, height);
            let mut terminal =
                Terminal::new(TestBackend::new(48, height)).expect("test terminal must open");
            let visible_count = count.min(usize::from(height - 5));
            assert_table_frame(&arborui, &mut ratatui, &mut terminal, 0..visible_count);
            for (action, offset) in [
                (TableAction::SelectActive, 0),
                (
                    TableAction::Scrolled(Point::new(0, i32::MAX)),
                    count - visible_count,
                ),
                (TableAction::Scrolled(Point::new(0, -i32::MAX)), 0),
            ] {
                arborui.send(action);
                ratatui.apply(action);
                assert_eq!(arborui.application().model().scroll_offset(), offset);
                assert_table_frame(
                    &arborui,
                    &mut ratatui,
                    &mut terminal,
                    offset..offset + visible_count,
                );
            }
        }
    }
}

#[test]
fn highlight_uses_the_clipped_body_not_the_model_viewport() {
    let mut ratatui = RatatuiTableLab::new(100, 48, 12);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
    ratatui.apply(TableAction::Select(6));
    ratatui.apply(TableAction::Scrolled(Point::new(0, 1)));
    draw_test_table_frame(&mut terminal, &mut ratatui).expect("initial table must draw");
    let state = ratatui.semantic_state();

    // A frame may be clipped before the model receives a resize. Heights <= 4 have no body.
    for (width, height) in [
        (0, 12),
        (1, 12),
        (2, 12),
        (48, 0u16),
        (48, 1),
        (48, 2),
        (48, 3),
        (48, 4),
        (48, 5),
        (48, 6),
        (48, 12),
    ] {
        terminal.backend_mut().resize(width, height);
        let frame = draw_test_table_frame(&mut terminal, &mut ratatui)
            .expect("clipped table frame must draw");
        let body_height = if width > 2 {
            usize::from(height.saturating_sub(4)).min(7)
        } else {
            0
        };
        assert_eq!(ratatui.semantic_state(), state);
        assert_eq!(
            rendered_rows(&frame),
            (1..1 + body_height).collect::<Vec<_>>()
        );
        let highlighted_rows: Vec<_> = (0..height)
            .filter(|y| width > 2 && terminal.backend().buffer()[(1, *y)].fg == Color::LightYellow)
            .collect();
        assert_eq!(
            highlighted_rows,
            if body_height >= 6 { vec![8] } else { vec![] },
        );
    }
}

fn rendered_rows(characters: &str) -> Vec<usize> {
    characters
        .lines()
        .skip(3)
        .filter_map(|line| {
            line.chars()
                .skip(1)
                .take(6)
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect()
}

fn assert_table_frame(
    arborui: &TestApp<TableLab>,
    ratatui: &mut RatatuiTableLab,
    terminal: &mut Terminal<TestBackend>,
    expected_rows: std::ops::Range<usize>,
) {
    let frame = draw_test_table_frame(terminal, ratatui).expect("table frame must draw");
    let model = arborui.application().model();
    assert_eq!(
        TableSemanticState {
            active_key: model.active_key(),
            selected_key: model.selected_key(),
            scroll_offset: model.scroll_offset(),
            viewport_height: model.viewport_height(),
            visible_range: model.visible_range(),
            constructed_rows: arborui.application().constructed_rows(),
            generation: model.generation(),
        },
        ratatui.semantic_state(),
    );
    let expected_rows: Vec<_> = expected_rows.collect();
    let arbor_frame = arborui.frame().characters();
    for (name, characters) in [("ArborUI", &arbor_frame), ("Ratatui", &frame)] {
        assert_eq!(
            rendered_rows(characters),
            expected_rows,
            "{name} rendered row IDs"
        );
    }
    assert_eq!(arbor_frame, frame);

    // Check ID cells independently of the renderers' differing gap/focus styling.
    for (index, key) in expected_rows.iter().enumerate() {
        let active = model.active_key() == Some(*key as u64);
        let selected = model.selected_key() == Some(*key as u64);
        for x in 1..7 {
            let arbor_cell = arborui
                .frame()
                .cell(Point::new(x, index as i32 + 3))
                .expect("body cell must exist");
            let ratatui_cell = &terminal.backend().buffer()[(x as u16, index as u16 + 3)];
            assert_eq!(
                arbor_cell.style.foreground,
                if active {
                    Some(ArborColor::BrightYellow)
                } else if selected {
                    Some(ArborColor::BrightWhite)
                } else {
                    None
                },
            );
            assert_eq!(
                ratatui_cell.fg,
                if active {
                    Color::LightYellow
                } else if selected {
                    Color::White
                } else {
                    Color::Reset
                },
            );
            assert_eq!(
                arbor_cell.style.background,
                selected.then_some(ArborColor::Blue)
            );
            assert_eq!(
                ratatui_cell.bg,
                if selected { Color::Blue } else { Color::Reset }
            );
            assert_eq!(
                arbor_cell.style.modifiers.contains(ArborModifier::BOLD),
                active
            );
            assert_eq!(ratatui_cell.modifier.contains(Modifier::BOLD), active);
        }
    }
}
