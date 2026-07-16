//! ArborUI phase attribution for the matched Collection Lab scenarios.

use std::time::{Duration, Instant};

use arborui::{
    AppRunner, Capabilities, HeadlessRenderOutcome, RenderTimings, Renderer, Size, UiEvent,
};
use arborui_example_collection_lab::{CollectionLab, CollectionMode, Message};

const ITEM_COUNT: usize = 100_000;
const WIDTH: u16 = 48;
const HEIGHT: u16 = 12;
const RESIZED_HEIGHT: u16 = 16;
const SAMPLES: u32 = 100;
const INITIAL_SAMPLES: u32 = 20;

#[derive(Clone, Copy)]
enum Scenario {
    PageDown,
    End,
    Resize,
    Selection,
    Reverse,
    UnchangedRedraw,
}

impl Scenario {
    const ALL: [Self; 6] = [
        Self::PageDown,
        Self::End,
        Self::Resize,
        Self::Selection,
        Self::Reverse,
        Self::UnchangedRedraw,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::PageDown => "page-down",
            Self::End => "end",
            Self::Resize => "resize",
            Self::Selection => "selection",
            Self::Reverse => "reverse",
            Self::UnchangedRedraw => "unchanged-redraw",
        }
    }
}

#[derive(Default)]
struct Totals {
    update: Duration,
    render: RenderTimings,
}

#[test]
#[ignore = "runs the optimized phase measurement matrix"]
fn reports_arborui_render_phase_metrics() {
    println!(
        "| Mode | Scenario | Update ns | View ns | Stage/reconcile ns | Layout ns | Paint ns | Diff ns | Commit ns | Post-commit ns | Render total ns |"
    );
    println!("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");

    for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
        let initial = measure_initial_render(mode);
        print_totals(mode, "initial-render", initial, INITIAL_SAMPLES);
        for scenario in Scenario::ALL {
            let totals = measure_scenario(mode, scenario);
            print_totals(mode, scenario.name(), totals, SAMPLES);
        }
    }
}

fn measure_initial_render(mode: CollectionMode) -> Totals {
    let mut totals = Totals::default();
    for _ in 0..INITIAL_SAMPLES {
        let model = CollectionLab::new(mode, ITEM_COUNT, viewport_height(HEIGHT));
        let mut runner = new_runner(model);
        let rendered = runner
            .render_headless_timed()
            .expect("initial frame must render");
        assert_eq!(rendered.outcome, HeadlessRenderOutcome::Committed);
        add_timings(
            &mut totals.render,
            rendered.timings.expect("render must include timings"),
        );
    }
    totals
}

fn measure_scenario(mode: CollectionMode, scenario: Scenario) -> Totals {
    let mut runner = new_runner(CollectionLab::new(
        mode,
        ITEM_COUNT,
        viewport_height(HEIGHT),
    ));
    assert_eq!(
        runner.render_headless().expect("initial frame must render"),
        HeadlessRenderOutcome::Committed
    );
    prepare_selection(&mut runner, scenario);

    let mut totals = Totals::default();
    for _ in 0..SAMPLES {
        let update_started = Instant::now();
        apply_scenario(&mut runner, scenario);
        totals.update = totals.update.saturating_add(update_started.elapsed());
        let rendered = runner
            .render_headless_timed()
            .expect("scenario frame must render");
        assert_eq!(rendered.outcome, HeadlessRenderOutcome::Committed);
        add_timings(
            &mut totals.render,
            rendered.timings.expect("render must include timings"),
        );
        reset_scenario(&mut runner, scenario);
    }
    totals
}

fn apply_scenario(runner: &mut AppRunner<CollectionLab>, scenario: Scenario) {
    match scenario {
        Scenario::PageDown => send(runner, Message::PageDown),
        Scenario::End => send(runner, Message::End),
        Scenario::Resize => resize(runner, RESIZED_HEIGHT),
        Scenario::Selection => send(runner, Message::SelectActive),
        Scenario::Reverse => send(runner, Message::Reverse),
        Scenario::UnchangedRedraw => send(runner, Message::Home),
    }
}

fn reset_scenario(runner: &mut AppRunner<CollectionLab>, scenario: Scenario) {
    match scenario {
        Scenario::PageDown | Scenario::End => {
            send(runner, Message::Home);
            render_reset(runner);
        }
        Scenario::Resize => {
            resize(runner, HEIGHT);
            render_reset(runner);
        }
        Scenario::Selection => {
            for message in [Message::Down, Message::SelectActive, Message::Home] {
                send(runner, message);
                render_reset(runner);
            }
        }
        Scenario::Reverse => {
            send(runner, Message::Reverse);
            render_reset(runner);
        }
        Scenario::UnchangedRedraw => {}
    }
}

fn prepare_selection(runner: &mut AppRunner<CollectionLab>, scenario: Scenario) {
    if matches!(scenario, Scenario::Selection) {
        reset_scenario(runner, scenario);
    }
}

fn send(runner: &mut AppRunner<CollectionLab>, message: Message) {
    runner.enqueue(message);
    runner.process_pending();
}

fn resize(runner: &mut AppRunner<CollectionLab>, height: u16) {
    runner
        .dispatch_ui_event(UiEvent::Resize(Size::new(WIDTH, height)))
        .expect("resize event must dispatch");
    runner.process_pending();
}

fn render_reset(runner: &mut AppRunner<CollectionLab>) {
    assert_eq!(
        runner.render_headless().expect("reset frame must render"),
        HeadlessRenderOutcome::Committed
    );
}

fn new_runner(application: CollectionLab) -> AppRunner<CollectionLab> {
    let size = Size::new(WIDTH, HEIGHT);
    AppRunner::new(
        application,
        size,
        Renderer::new(size, Capabilities::default().width_policy),
    )
}

fn add_timings(total: &mut RenderTimings, sample: RenderTimings) {
    total.total = total.total.saturating_add(sample.total);
    total.view_construction = total
        .view_construction
        .saturating_add(sample.view_construction);
    total.staging_reconciliation = total
        .staging_reconciliation
        .saturating_add(sample.staging_reconciliation);
    total.layout = total.layout.saturating_add(sample.layout);
    total.paint = total.paint.saturating_add(sample.paint);
    total.diff = total.diff.saturating_add(sample.diff);
    total.commit = Some(
        total
            .commit
            .unwrap_or_default()
            .saturating_add(sample.commit.unwrap_or_default()),
    );
    total.post_commit = Some(
        total
            .post_commit
            .unwrap_or_default()
            .saturating_add(sample.post_commit.unwrap_or_default()),
    );
}

fn print_totals(mode: CollectionMode, scenario: &str, totals: Totals, samples: u32) {
    println!(
        "| {} | {scenario} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
        mode_name(mode),
        average(totals.update, samples),
        average(totals.render.view_construction, samples),
        average(totals.render.staging_reconciliation, samples),
        average(totals.render.layout, samples),
        average(totals.render.paint, samples),
        average(totals.render.diff, samples),
        average(totals.render.commit.unwrap_or_default(), samples),
        average(totals.render.post_commit.unwrap_or_default(), samples),
        average(totals.render.total, samples),
    );
}

fn average(duration: Duration, samples: u32) -> u128 {
    duration.as_nanos() / u128::from(samples)
}

const fn mode_name(mode: CollectionMode) -> &'static str {
    match mode {
        CollectionMode::Fixed => "fixed",
        CollectionMode::Variable => "variable",
    }
}

fn viewport_height(terminal_height: u16) -> usize {
    usize::from(terminal_height.saturating_sub(4).max(1))
}
