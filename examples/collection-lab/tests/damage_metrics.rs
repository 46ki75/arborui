#![allow(missing_docs)]
//! Opt-in logical repaint-area evidence for separated visible-row changes.

use std::time::Duration;

use arborui::{AppRunner, Capabilities, HeadlessRenderOutcome, RenderTimings, Renderer, Size};
use arborui_example_collection_lab::{CollectionLab, CollectionMode, Message};

const VIEWPORT: Size = Size::new(48, 12);
const DISTANT_ROW: usize = 7;
const SAMPLES: u32 = 100;

fn send(runner: &mut AppRunner<CollectionLab>, message: Message) {
    runner.enqueue(message);
    assert_eq!(runner.process_pending().updates, 1);
}

fn setup() -> AppRunner<CollectionLab> {
    let mut runner = AppRunner::new(
        CollectionLab::new(CollectionMode::Fixed, 100_000, 8),
        VIEWPORT,
        Renderer::new(VIEWPORT, Capabilities::default().width_policy),
    );
    assert_eq!(
        runner.render_headless().expect("initial frame must render"),
        HeadlessRenderOutcome::Committed
    );
    send(&mut runner, Message::SelectActive);
    assert_eq!(
        runner
            .render_headless()
            .expect("selection baseline must render"),
        HeadlessRenderOutcome::Committed
    );
    runner
}

fn measure() -> (RenderTimings, RenderTimings) {
    let mut runner = setup();
    for _ in 0..DISTANT_ROW {
        send(&mut runner, Message::Down);
    }

    let movement = runner
        .render_headless_timed()
        .expect("distant active-row movement must render");
    assert_eq!(movement.outcome, HeadlessRenderOutcome::Committed);
    assert_eq!(runner.application().active_key(), Some(DISTANT_ROW as u64));
    assert_eq!(runner.application().selected_key(), Some(0));

    send(&mut runner, Message::SelectActive);
    let selection = runner
        .render_headless_timed()
        .expect("distant selection movement must render");
    assert_eq!(selection.outcome, HeadlessRenderOutcome::Committed);
    assert_eq!(
        runner.application().selected_key(),
        Some(DISTANT_ROW as u64)
    );

    (
        movement.timings.expect("movement render must be timed"),
        selection.timings.expect("selection render must be timed"),
    )
}

#[test]
#[ignore = "optimized logical repaint-area report"]
fn reports_separated_row_damage() {
    let mut movement_paint = Duration::ZERO;
    let mut movement_total = Duration::ZERO;
    let mut selection_paint = Duration::ZERO;
    let mut selection_total = Duration::ZERO;
    for _ in 0..SAMPLES {
        let (movement, selection) = measure();
        for timings in [movement, selection] {
            assert_eq!(timings.repaint_regions, 2);
            assert_eq!(timings.repaint_cells, 96);
        }
        movement_paint = movement_paint.saturating_add(movement.paint);
        movement_total = movement_total.saturating_add(movement.total);
        selection_paint = selection_paint.saturating_add(selection.paint);
        selection_total = selection_total.saturating_add(selection.total);
    }

    println!("| Scenario | Repaint regions | Repaint cells | Paint | Render total |");
    println!("| --- | ---: | ---: | ---: | ---: |");
    for (name, paint, total) in [
        ("active-row move", movement_paint, movement_total),
        ("selection move", selection_paint, selection_total),
    ] {
        println!(
            "| {name} | 2 | 96 | {:.3} us | {:.3} us |",
            paint.as_secs_f64() * 1_000_000.0 / f64::from(SAMPLES),
            total.as_secs_f64() * 1_000_000.0 / f64::from(SAMPLES),
        );
    }
}
