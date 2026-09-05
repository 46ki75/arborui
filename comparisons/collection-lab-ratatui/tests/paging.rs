//! Independent paging expectations, with adapter agreement as an extra oracle.

use arborui_comparison_collection_lab_ratatui::{
    ComparisonAction, RatatuiCollectionLab, RatatuiTableLab, SemanticState, TableSemanticState,
    draw_test_frame, draw_test_table_frame,
};
use arborui_example_collection_lab::{
    CollectionLab, CollectionMode, Message, TableAction, TableLab,
};
use arborui_test::{Size, TestApp};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn collection_paging_has_independent_expected_keys_and_matching_frames() {
    for (mode, count, viewport, reversed, down, up) in [
        (
            CollectionMode::Fixed,
            100,
            8,
            false,
            (8..=96).step_by(8).chain([99]).collect::<Vec<_>>(),
            (0..=11).rev().map(|page| 3 + page * 8).chain([0]).collect(),
        ),
        (
            CollectionMode::Variable,
            10,
            1,
            false,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
            vec![8, 7, 6, 5, 4, 3, 2, 1, 0],
        ),
        (
            CollectionMode::Variable,
            10,
            2,
            false,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
            vec![8, 7, 5, 4, 2, 1, 0],
        ),
        (
            CollectionMode::Variable,
            10,
            8,
            false,
            vec![4, 8, 9],
            vec![5, 1, 0],
        ),
        (CollectionMode::Fixed, 10, 8, true, vec![1, 0], vec![8, 9]),
        (CollectionMode::Fixed, 3, 8, false, vec![2], vec![0]),
        (CollectionMode::Variable, 3, 8, false, vec![2], vec![0]),
        (
            CollectionMode::Variable,
            3,
            1,
            false,
            vec![1, 2],
            vec![1, 0],
        ),
        (
            CollectionMode::Variable,
            10,
            8,
            true,
            vec![5, 2, 0],
            vec![4, 8, 9],
        ),
    ] {
        let height = viewport as u16 + 4;
        let mut arborui = TestApp::new(
            CollectionLab::new(mode, count, viewport),
            Size::new(48, height),
        );
        let mut ratatui = RatatuiCollectionLab::new(mode, count, 48, height);
        let mut terminal = Terminal::new(TestBackend::new(48, height)).expect("terminal opens");
        arborui.send(Message::SelectActive);
        ratatui.apply(ComparisonAction::SelectActive);
        if reversed {
            arborui.send(Message::Reverse);
            ratatui.apply(ComparisonAction::Reverse);
            assert_eq!(ratatui.semantic_state().active_key, Some(0));
            assert_eq!(arborui.application().active_key(), Some(0));
            arborui.send(Message::Home);
            ratatui.apply(ComparisonAction::Home);
        }
        for (action, message, keys) in [
            (ComparisonAction::PageDown, Message::PageDown, down),
            (ComparisonAction::PageUp, Message::PageUp, up),
        ] {
            for expected in keys.iter().chain(keys.last()) {
                ratatui.apply(action);
                assert_eq!(
                    ratatui.semantic_state().active_key,
                    Some(*expected),
                    "{mode:?}, {viewport} cells, reversed={reversed}, {action:?}"
                );
                arborui.send(message);
                let app = arborui.application();
                assert_eq!(app.active_key(), Some(*expected));
                assert_eq!(app.selected_key(), Some(0));
                let frame = draw_test_frame(&mut terminal, &mut ratatui).expect("frame draws");
                assert_eq!(
                    ratatui.semantic_state(),
                    SemanticState {
                        active_key: app.active_key(),
                        selected_key: app.selected_key(),
                        scroll_offset: app.scroll_offset(),
                        viewport_height: app.viewport_height(),
                        visible_range: app.visible_range(),
                        constructed_rows: app.constructed_rows(),
                    }
                );
                assert_eq!(arborui.frame().characters(), frame);
            }
        }
    }
}

#[test]
fn table_paging_has_independent_expected_keys_and_matching_frames() {
    let mut arborui = TestApp::new(TableLab::new(100, 64, 12), Size::new(64, 12));
    let mut ratatui = RatatuiTableLab::new(100, 64, 12);
    let mut terminal = Terminal::new(TestBackend::new(64, 12)).expect("terminal opens");
    arborui.send(TableAction::SelectActive);
    ratatui.apply(TableAction::SelectActive);
    let down = (7..=98).step_by(7).chain([99, 99]).collect::<Vec<_>>();
    let up = (0..=13)
        .rev()
        .map(|page| 1 + page * 7)
        .chain([0, 0])
        .collect();
    for (action, keys) in [(TableAction::PageDown, down), (TableAction::PageUp, up)] {
        for expected in keys {
            ratatui.apply(action);
            assert_eq!(ratatui.semantic_state().active_key, Some(expected));
            arborui.send(action);
            let app = arborui.application();
            let model = app.model();
            assert_eq!(model.active_key(), Some(expected));
            assert_eq!(model.selected_key(), Some(0));
            let frame = draw_test_table_frame(&mut terminal, &mut ratatui).expect("frame draws");
            assert_eq!(
                ratatui.semantic_state(),
                TableSemanticState {
                    active_key: model.active_key(),
                    selected_key: model.selected_key(),
                    scroll_offset: model.scroll_offset(),
                    viewport_height: model.viewport_height(),
                    visible_range: model.visible_range(),
                    constructed_rows: app.constructed_rows(),
                    generation: model.generation(),
                }
            );
            assert_eq!(arborui.frame().characters(), frame);
        }
    }
}

#[test]
fn collection_paging_handles_empty_single_underfilled_and_tall_final_rows() {
    for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
        for count in [0usize, 1, 3] {
            for height in [5, 6, 12, u16::MAX] {
                let mut app = RatatuiCollectionLab::new(mode, count, 48, height);
                for _ in 0..4 {
                    app.apply(ComparisonAction::PageDown);
                }
                assert_eq!(
                    app.semantic_state().active_key,
                    count.checked_sub(1).map(|key| key as u64)
                );
                let boundary = app.semantic_state();
                app.apply(ComparisonAction::PageDown);
                assert_eq!(app.semantic_state(), boundary);
                for _ in 0..4 {
                    app.apply(ComparisonAction::PageUp);
                }
                assert_eq!(app.semantic_state().active_key, (count > 0).then_some(0));
                assert_eq!(app.semantic_state().scroll_offset, 0);
                let boundary = app.semantic_state();
                app.apply(ComparisonAction::PageUp);
                assert_eq!(app.semantic_state(), boundary);
            }
        }
    }
}
