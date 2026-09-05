//! Paging contracts exercised through public keyboard and frame controls.

use arborui_example_collection_lab::{CollectionLab, CollectionMode, TableLab};
use arborui_test::{
    Key, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind, Point, Size, TestApp,
};

#[test]
fn collection_keyboard_pages_to_both_ends_and_reveals_each_target() {
    for (mode, count, down, up) in [
        (
            CollectionMode::Fixed,
            100,
            (8..=96).step_by(8).chain([99, 99]).collect::<Vec<_>>(),
            (0..=11)
                .rev()
                .map(|page| 3 + page * 8)
                .chain([0, 0])
                .collect(),
        ),
        (
            CollectionMode::Variable,
            10,
            vec![4, 8, 9, 9],
            vec![5, 1, 0, 0],
        ),
    ] {
        let mut app = TestApp::new(CollectionLab::new(mode, count, 8), Size::new(48, 12));
        app.key(KeyCode::Enter);
        for (key, expected_keys) in [(KeyCode::PageDown, down), (KeyCode::PageUp, up)] {
            for expected in expected_keys {
                app.key_with(key, KeyModifiers::NONE, KeyEventKind::Repeat);
                assert_eq!(app.application().active_key(), Some(expected));
                assert_eq!(app.application().selected_key(), Some(0));
                assert_eq!(app.focused_key(), Some(Key::from("collection")));
                assert!(
                    app.frame()
                        .characters()
                        .contains(&format!("Item {expected:06}"))
                );
            }
        }
    }
}

#[test]
fn table_keyboard_pages_to_both_ends_and_reveals_each_target() {
    let mut app = TestApp::new(TableLab::new(100, 64, 12), Size::new(64, 12));
    app.key(KeyCode::Enter);
    let down = (7..=98).step_by(7).chain([99, 99]).collect::<Vec<_>>();
    let up = (0..=13)
        .rev()
        .map(|page| 1 + page * 7)
        .chain([0, 0])
        .collect();
    for (key, expected_keys) in [(KeyCode::PageDown, down), (KeyCode::PageUp, up)] {
        for expected in expected_keys {
            app.key(key);
            assert_eq!(app.application().model().active_key(), Some(expected));
            assert_eq!(app.application().model().selected_key(), Some(0));
            assert_eq!(app.focused_key(), Some(Key::from("table")));
            assert!(
                app.frame()
                    .characters()
                    .contains(&format!("Service {expected:06}"))
            );
        }
    }
}

#[test]
fn collection_paging_ignores_wheel_detachment_and_uses_reordered_keys() {
    let mut app = TestApp::new(
        CollectionLab::new(CollectionMode::Fixed, 100, 8),
        Size::new(48, 12),
    );
    app.key(KeyCode::Enter);
    for _ in 0..12 {
        app.mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            position: Point::new(3, 4),
            modifiers: KeyModifiers::NONE,
        });
    }
    assert!(app.application().scroll_offset() > 8);
    assert_eq!(app.application().active_key(), Some(0));
    app.key(KeyCode::PageDown);
    assert_eq!(app.application().active_key(), Some(8));
    assert!(app.frame().characters().contains("Item 000008"));

    app.key(KeyCode::End);
    for _ in 0..100 {
        app.mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            position: Point::new(3, 4),
            modifiers: KeyModifiers::NONE,
        });
    }
    assert_eq!(app.application().scroll_offset(), 0);
    app.key(KeyCode::PageUp);
    assert_eq!(app.application().active_key(), Some(91));
    assert!(app.frame().characters().contains("Item 000091"));
    app.key(KeyCode::Character('r'));
    assert_eq!(app.application().active_key(), Some(91));
    app.key(KeyCode::PageDown);
    assert_eq!(app.application().active_key(), Some(83));
    assert_eq!(app.application().selected_key(), Some(0));
    assert!(app.frame().characters().contains("Item 000083"));
}
