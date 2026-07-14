//! Downstream test using only the public application and test facades.

use arborui_example_counter::Counter;
use arborui_test::{Key, KeyCode, Size, TestApp};

#[test]
fn counter_is_driven_through_public_facades() {
    let mut app = TestApp::new(Counter::default(), Size::new(16, 5));

    assert_eq!(
        app.frame().characters(),
        "Count: 0        \n                \nIncrement       \n                \nQuit            "
    );

    assert_eq!(app.focused_key(), Some(Key::from("increment")));
    app.key(KeyCode::Enter);

    assert_eq!(app.application().count(), 1);
    assert_eq!(
        app.frame().characters(),
        "Count: 1        \n                \nIncrement       \n                \nQuit            "
    );
}
