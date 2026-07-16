//! Isolated heap measurement process for one matched Collection Lab case.

use std::{error::Error, hint::black_box};

use arborui_comparison_collection_lab_ratatui::{
    ComparisonAction, RatatuiCollectionLab, draw_test_terminal,
};
use arborui_example_collection_lab::{CollectionLab, CollectionMode, Message};
use arborui_test::{Size, TestApp};
use ratatui::{Terminal, backend::TestBackend};

const WIDTH: u16 = 48;
const HEIGHT: u16 = 12;
const RESIZED_HEIGHT: u16 = 16;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[derive(Clone, Copy)]
enum Framework {
    Arborui,
    Ratatui,
}

#[derive(Clone, Copy)]
enum Scenario {
    Model,
    Cold,
    InitialRender,
    PageDown,
    Resize,
    Reverse,
    UnchangedRedraw,
}

struct Metrics {
    total_blocks: u64,
    total_bytes: u64,
    max_blocks: usize,
    max_bytes: usize,
    curr_blocks: usize,
    curr_bytes: usize,
    end_blocks: usize,
    end_bytes: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().collect::<Vec<_>>();
    let [_, framework, mode, scenario, item_count] = arguments.as_slice() else {
        return Err(
            "usage: memory_probe <arborui|ratatui> <fixed|variable> <scenario> <items>".into(),
        );
    };
    let framework = parse_framework(framework)?;
    let mode = parse_mode(mode)?;
    let scenario = parse_scenario(scenario)?;
    let item_count = item_count.parse::<usize>()?;

    let metrics = match framework {
        Framework::Arborui => measure_arborui(mode, scenario, item_count),
        Framework::Ratatui => measure_ratatui(mode, scenario, item_count),
    };
    println!(
        "total_blocks={} total_bytes={} max_blocks={} max_bytes={} curr_blocks={} curr_bytes={} end_blocks={} end_bytes={}",
        metrics.total_blocks,
        metrics.total_bytes,
        metrics.max_blocks,
        metrics.max_bytes,
        metrics.curr_blocks,
        metrics.curr_bytes,
        metrics.end_blocks,
        metrics.end_bytes
    );
    Ok(())
}

fn measure_arborui(mode: CollectionMode, scenario: Scenario, item_count: usize) -> Metrics {
    match scenario {
        Scenario::Model => {
            measure(|| CollectionLab::new(mode, item_count, viewport_height(HEIGHT)))
        }
        Scenario::Cold => measure(|| {
            let application = TestApp::new(
                CollectionLab::new(mode, item_count, viewport_height(HEIGHT)),
                base_size(),
            );
            assert_bounded(application.application().constructed_rows());
            application
        }),
        Scenario::InitialRender => {
            let model = CollectionLab::new(mode, item_count, viewport_height(HEIGHT));
            measure(move || {
                let application = TestApp::new(model, base_size());
                assert_bounded(application.application().constructed_rows());
                application
            })
        }
        Scenario::PageDown | Scenario::Resize | Scenario::Reverse | Scenario::UnchangedRedraw => {
            let mut application = TestApp::new(
                CollectionLab::new(mode, item_count, viewport_height(HEIGHT)),
                base_size(),
            );
            measure(move || {
                match scenario {
                    Scenario::PageDown => {
                        application.send(Message::PageDown);
                    }
                    Scenario::Resize => {
                        application.resize(Size::new(WIDTH, RESIZED_HEIGHT));
                    }
                    Scenario::Reverse => {
                        application.send(Message::Reverse);
                    }
                    Scenario::UnchangedRedraw => {
                        application.send(Message::Home);
                    }
                    Scenario::Model | Scenario::Cold | Scenario::InitialRender => {
                        unreachable!("setup scenarios are handled separately")
                    }
                }
                assert_bounded(application.application().constructed_rows());
                black_box(application)
            })
        }
    }
}

fn measure_ratatui(mode: CollectionMode, scenario: Scenario, item_count: usize) -> Metrics {
    match scenario {
        Scenario::Model => measure(|| RatatuiCollectionLab::new(mode, item_count, WIDTH, HEIGHT)),
        Scenario::Cold => measure(|| {
            let mut application = RatatuiCollectionLab::new(mode, item_count, WIDTH, HEIGHT);
            let mut terminal =
                Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal must open");
            draw_test_terminal(&mut terminal, &mut application).expect("initial frame must draw");
            assert_bounded(application.semantic_state().constructed_rows);
            (application, terminal)
        }),
        Scenario::InitialRender => {
            let mut application = RatatuiCollectionLab::new(mode, item_count, WIDTH, HEIGHT);
            measure(move || {
                let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT))
                    .expect("test terminal must open");
                draw_test_terminal(&mut terminal, &mut application)
                    .expect("initial frame must draw");
                assert_bounded(application.semantic_state().constructed_rows);
                (application, terminal)
            })
        }
        Scenario::PageDown | Scenario::Resize | Scenario::Reverse | Scenario::UnchangedRedraw => {
            let (mut application, mut terminal) = ratatui_fixture(mode, item_count);
            measure(move || {
                match scenario {
                    Scenario::PageDown => application.apply(ComparisonAction::PageDown),
                    Scenario::Resize => {
                        terminal.backend_mut().resize(WIDTH, RESIZED_HEIGHT);
                        application.apply(ComparisonAction::Resize {
                            width: WIDTH,
                            height: RESIZED_HEIGHT,
                        });
                    }
                    Scenario::Reverse => application.apply(ComparisonAction::Reverse),
                    Scenario::UnchangedRedraw => application.apply(ComparisonAction::Home),
                    Scenario::Model | Scenario::Cold | Scenario::InitialRender => {
                        unreachable!("setup scenarios are handled separately")
                    }
                }
                draw_test_terminal(&mut terminal, &mut application).expect("frame must draw");
                assert_bounded(application.semantic_state().constructed_rows);
                black_box((application, terminal))
            })
        }
    }
}

fn measure<T>(operation: impl FnOnce() -> T) -> Metrics {
    let profiler = dhat::Profiler::builder()
        .testing()
        .trim_backtraces(Some(4))
        .build();
    let value = operation();
    let live = dhat::HeapStats::get();
    drop(value);
    let end = dhat::HeapStats::get();
    drop(profiler);
    Metrics {
        total_blocks: live.total_blocks,
        total_bytes: live.total_bytes,
        max_blocks: live.max_blocks,
        max_bytes: live.max_bytes,
        curr_blocks: live.curr_blocks,
        curr_bytes: live.curr_bytes,
        end_blocks: end.curr_blocks,
        end_bytes: end.curr_bytes,
    }
}

fn ratatui_fixture(
    mode: CollectionMode,
    item_count: usize,
) -> (RatatuiCollectionLab, Terminal<TestBackend>) {
    let mut application = RatatuiCollectionLab::new(mode, item_count, WIDTH, HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal must open");
    draw_test_terminal(&mut terminal, &mut application).expect("initial frame must draw");
    (application, terminal)
}

fn assert_bounded(constructed_rows: usize) {
    assert!(constructed_rows > 0);
    assert!(constructed_rows <= usize::from(RESIZED_HEIGHT));
}

fn parse_framework(value: &str) -> Result<Framework, Box<dyn Error>> {
    match value {
        "arborui" => Ok(Framework::Arborui),
        "ratatui" => Ok(Framework::Ratatui),
        _ => Err(format!("unknown framework: {value}").into()),
    }
}

fn parse_mode(value: &str) -> Result<CollectionMode, Box<dyn Error>> {
    match value {
        "fixed" => Ok(CollectionMode::Fixed),
        "variable" => Ok(CollectionMode::Variable),
        _ => Err(format!("unknown mode: {value}").into()),
    }
}

fn parse_scenario(value: &str) -> Result<Scenario, Box<dyn Error>> {
    match value {
        "model" => Ok(Scenario::Model),
        "cold" => Ok(Scenario::Cold),
        "initial-render" => Ok(Scenario::InitialRender),
        "page-down" => Ok(Scenario::PageDown),
        "resize" => Ok(Scenario::Resize),
        "reverse" => Ok(Scenario::Reverse),
        "unchanged-redraw" => Ok(Scenario::UnchangedRedraw),
        _ => Err(format!("unknown scenario: {value}").into()),
    }
}

const fn base_size() -> Size {
    Size::new(WIDTH, HEIGHT)
}

fn viewport_height(terminal_height: u16) -> usize {
    usize::from(terminal_height.saturating_sub(4).max(1))
}
