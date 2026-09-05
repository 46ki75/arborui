#![allow(missing_docs)]

use std::{hint::black_box, time::Duration};

use arborui_comparison_collection_lab_ratatui::{
    ComparisonAction, OVERLAY_RESIZE_STORM, RatatuiCollectionLab, RatatuiLogLab, RatatuiOverlayLab,
    RatatuiTableLab, RatatuiUnicodeLab, STANDARD_RESIZE_STORM, UNICODE_RESIZE_STORM,
    UNICODE_RESIZE_STORM_OFFSET, draw_test_log_terminal, draw_test_overlay_terminal,
    draw_test_table_terminal, draw_test_terminal, draw_test_unicode_terminal,
};
use arborui_example_collection_lab::{
    CollectionLab, CollectionMode, LogAction, LogLab, Message, OverlayAction, OverlayLab,
    TableAction, TableLab, UnicodeAction, UnicodeLab,
};
use arborui_test::{KeyCode, Size, TestApp, TestAppOptions};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ratatui::{Terminal, backend::TestBackend};

const ITEM_COUNT: usize = 100_000;
const BASE_WIDTH: u16 = 48;
const BASE_HEIGHT: u16 = 12;
const RESIZED_HEIGHT: u16 = 16;
const OVERLAY_WIDTH: u16 = 40;
const OVERLAY_HEIGHT: u16 = 12;
const OVERLAY_RESIZED_WIDTH: u16 = 44;
const OVERLAY_RESIZED_HEIGHT: u16 = 14;
const UNICODE_WIDTH: u16 = 36;
const UNICODE_HEIGHT: u16 = 10;
const UNICODE_NARROW_WIDTH: u16 = 30;
const UNICODE_BOUNDARY_OFFSET: usize = 15;

#[derive(Clone, Copy)]
enum Scenario {
    PageDown,
    End,
    Resize,
    Selection,
    Reverse,
    UnchangedRedraw,
}

#[derive(Clone, Copy)]
enum TableScenario {
    PageDown,
    Selection,
    Resize,
    VisibleUpdate,
    OffscreenUpdate,
}

#[derive(Clone, Copy)]
enum LogScenario {
    PageUp,
    Resize,
    AppendFollowing,
    AppendPaused,
}

#[derive(Clone, Copy)]
enum OverlayScenario {
    Open,
    FocusNext,
    Cancel,
    Confirm,
    BackgroundActivation,
    ResizeOpen,
}

#[derive(Clone, Copy)]
enum UnicodeScenario {
    ShiftBoundary,
    ReplaceWide,
    ResizeNarrow,
}

impl UnicodeScenario {
    const ALL: [Self; 3] = [Self::ShiftBoundary, Self::ReplaceWide, Self::ResizeNarrow];

    const fn name(self) -> &'static str {
        match self {
            Self::ShiftBoundary => "shift-boundary",
            Self::ReplaceWide => "replace-wide",
            Self::ResizeNarrow => "resize-narrow",
        }
    }
}

impl OverlayScenario {
    const ALL: [Self; 6] = [
        Self::Open,
        Self::FocusNext,
        Self::Cancel,
        Self::Confirm,
        Self::BackgroundActivation,
        Self::ResizeOpen,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::FocusNext => "focus-next",
            Self::Cancel => "cancel",
            Self::Confirm => "confirm",
            Self::BackgroundActivation => "background-activation",
            Self::ResizeOpen => "resize-open",
        }
    }
}

impl LogScenario {
    const ALL: [Self; 4] = [
        Self::PageUp,
        Self::Resize,
        Self::AppendFollowing,
        Self::AppendPaused,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::PageUp => "page-up",
            Self::Resize => "resize",
            Self::AppendFollowing => "append-following",
            Self::AppendPaused => "append-paused",
        }
    }
}

impl TableScenario {
    const ALL: [Self; 5] = [
        Self::PageDown,
        Self::Selection,
        Self::Resize,
        Self::VisibleUpdate,
        Self::OffscreenUpdate,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::PageDown => "page-down",
            Self::Selection => "selection",
            Self::Resize => "resize",
            Self::VisibleUpdate => "visible-update",
            Self::OffscreenUpdate => "offscreen-update",
        }
    }
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

fn application_turns(criterion: &mut Criterion) {
    line_navigation(criterion);
    cold_initial_render(criterion);
    scenario_turns(criterion);
    table_line_navigation(criterion);
    table_cold_initial_render(criterion);
    table_scenario_turns(criterion);
    log_line_scrolling(criterion);
    log_cold_initial_render(criterion);
    log_scenario_turns(criterion);
    overlay_cold_initial_render(criterion);
    overlay_scenario_turns(criterion);
    unicode_cold_initial_render(criterion);
    unicode_scenario_turns(criterion);
    resize_storm_turns(criterion);
}

fn resize_storm_turns(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/resize-storm");
    group.throughput(Throughput::Elements(STANDARD_RESIZE_STORM.len() as u64));

    for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
        let mode_name = mode_name(mode);
        group.bench_function(BenchmarkId::new("arborui", mode_name), |bencher| {
            let mut application = TestApp::with_options(
                CollectionLab::new(mode, ITEM_COUNT, viewport_height(BASE_HEIGHT)),
                base_size(),
                measurement_options(),
            );
            application.send(Message::SelectActive);
            bencher.iter(|| {
                for (width, height) in STANDARD_RESIZE_STORM {
                    black_box(application.resize(Size::new(width, height)));
                }
            });
        });
        group.bench_function(BenchmarkId::new("ratatui", mode_name), |bencher| {
            let (mut application, mut terminal) = ratatui_fixture(mode, ITEM_COUNT);
            ratatui_turn(
                &mut application,
                &mut terminal,
                ComparisonAction::SelectActive,
            );
            bencher.iter(|| {
                for (width, height) in STANDARD_RESIZE_STORM {
                    terminal.backend_mut().resize(width, height);
                    ratatui_turn(
                        &mut application,
                        &mut terminal,
                        ComparisonAction::Resize { width, height },
                    );
                }
            });
        });
    }

    group.bench_function(BenchmarkId::new("arborui", "table"), |bencher| {
        let mut application = TestApp::with_options(
            TableLab::new(ITEM_COUNT, BASE_WIDTH, BASE_HEIGHT),
            base_size(),
            measurement_options(),
        );
        application.send(TableAction::SelectActive);
        bencher.iter(|| {
            for (width, height) in STANDARD_RESIZE_STORM {
                black_box(application.resize(Size::new(width, height)));
            }
        });
    });
    group.bench_function(BenchmarkId::new("ratatui", "table"), |bencher| {
        let (mut application, mut terminal) = ratatui_table_fixture(ITEM_COUNT);
        ratatui_table_turn(&mut application, &mut terminal, TableAction::SelectActive);
        bencher.iter(|| {
            for (width, height) in STANDARD_RESIZE_STORM {
                terminal.backend_mut().resize(width, height);
                ratatui_table_turn(
                    &mut application,
                    &mut terminal,
                    TableAction::Resize { width, height },
                );
            }
        });
    });

    group.bench_function(BenchmarkId::new("arborui", "log-paused"), |bencher| {
        let mut application = TestApp::with_options(
            LogLab::new(
                ITEM_COUNT,
                ITEM_COUNT.saturating_mul(2),
                BASE_WIDTH,
                BASE_HEIGHT,
            ),
            base_size(),
            measurement_options(),
        );
        application.send(LogAction::PageUp);
        bencher.iter(|| {
            for (width, height) in STANDARD_RESIZE_STORM {
                black_box(application.resize(Size::new(width, height)));
            }
        });
    });
    group.bench_function(BenchmarkId::new("ratatui", "log-paused"), |bencher| {
        let (mut application, mut terminal) =
            ratatui_log_fixture_with_limit(ITEM_COUNT, ITEM_COUNT.saturating_mul(2));
        ratatui_log_turn(&mut application, &mut terminal, LogAction::PageUp);
        bencher.iter(|| {
            for (width, height) in STANDARD_RESIZE_STORM {
                terminal.backend_mut().resize(width, height);
                ratatui_log_turn(
                    &mut application,
                    &mut terminal,
                    LogAction::Resize { width, height },
                );
            }
        });
    });

    group.bench_function(BenchmarkId::new("arborui", "overlay-open"), |bencher| {
        let mut application = TestApp::with_options(
            OverlayLab::new(OVERLAY_WIDTH, OVERLAY_HEIGHT),
            overlay_size(),
            measurement_options(),
        );
        application.send(OverlayAction::Open);
        application.key(KeyCode::Tab);
        bencher.iter(|| {
            for (width, height) in OVERLAY_RESIZE_STORM {
                black_box(application.resize(Size::new(width, height)));
            }
        });
    });
    group.bench_function(BenchmarkId::new("ratatui", "overlay-open"), |bencher| {
        let (mut application, mut terminal) = ratatui_overlay_fixture();
        ratatui_overlay_turn(&mut application, &mut terminal, OverlayAction::Open);
        application.focus_next();
        draw_test_overlay_terminal(&mut terminal, &application).expect("overlay frame must draw");
        bencher.iter(|| {
            for (width, height) in OVERLAY_RESIZE_STORM {
                ratatui_overlay_resize(&mut application, &mut terminal, width, height);
            }
        });
    });

    group.bench_function(BenchmarkId::new("arborui", "unicode"), |bencher| {
        let mut application = TestApp::with_options(
            UnicodeLab::new(UNICODE_WIDTH, UNICODE_HEIGHT),
            unicode_size(),
            measurement_options(),
        );
        for _ in 0..UNICODE_RESIZE_STORM_OFFSET {
            application.send(UnicodeAction::ShiftRight);
        }
        bencher.iter(|| {
            for (width, height) in UNICODE_RESIZE_STORM {
                black_box(application.resize(Size::new(width, height)));
            }
        });
    });
    group.bench_function(BenchmarkId::new("ratatui", "unicode"), |bencher| {
        let (mut application, mut terminal) = ratatui_unicode_fixture();
        for _ in 0..UNICODE_RESIZE_STORM_OFFSET {
            ratatui_unicode_turn(&mut application, &mut terminal, UnicodeAction::ShiftRight);
        }
        bencher.iter(|| {
            for (width, height) in UNICODE_RESIZE_STORM {
                terminal.backend_mut().resize(width, height);
                ratatui_unicode_turn(
                    &mut application,
                    &mut terminal,
                    UnicodeAction::Resize { width, height },
                );
            }
        });
    });
    group.finish();
}

fn unicode_cold_initial_render(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/unicode-turn/cold-initial-render");
    group.throughput(Throughput::Elements(1));
    group.bench_function("arborui", |bencher| {
        bencher.iter(|| {
            black_box(TestApp::with_options(
                UnicodeLab::new(UNICODE_WIDTH, UNICODE_HEIGHT),
                unicode_size(),
                measurement_options(),
            ));
        });
    });
    group.bench_function("ratatui", |bencher| {
        bencher.iter(|| {
            let application = RatatuiUnicodeLab::new(UNICODE_WIDTH, UNICODE_HEIGHT);
            let mut terminal = Terminal::new(TestBackend::new(UNICODE_WIDTH, UNICODE_HEIGHT))
                .expect("test terminal must open");
            draw_test_unicode_terminal(&mut terminal, &application)
                .expect("initial Unicode frame must draw");
            black_box((application, terminal));
        });
    });
    group.finish();
}

fn unicode_scenario_turns(criterion: &mut Criterion) {
    for scenario in UnicodeScenario::ALL {
        let mut group =
            criterion.benchmark_group(format!("comparison/unicode-turn/{}", scenario.name()));
        group.throughput(Throughput::Elements(1));
        group.bench_function("arborui", |bencher| {
            let mut application = TestApp::with_options(
                UnicodeLab::new(UNICODE_WIDTH, UNICODE_HEIGHT),
                unicode_size(),
                measurement_options(),
            );
            prepare_arborui_unicode(&mut application, scenario);
            bencher.iter_custom(|iterations| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iterations {
                    let started = std::time::Instant::now();
                    arborui_unicode_turn(&mut application, scenario);
                    elapsed = elapsed.saturating_add(started.elapsed());
                    reset_arborui_unicode(&mut application, scenario);
                }
                black_box(application.application().model().terminal_size());
                elapsed
            });
        });
        group.bench_function("ratatui", |bencher| {
            let (mut application, mut terminal) = ratatui_unicode_fixture();
            prepare_ratatui_unicode(&mut application, &mut terminal, scenario);
            bencher.iter_custom(|iterations| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iterations {
                    let started = std::time::Instant::now();
                    ratatui_unicode_scenario_turn(&mut application, &mut terminal, scenario);
                    elapsed = elapsed.saturating_add(started.elapsed());
                    reset_ratatui_unicode(&mut application, &mut terminal, scenario);
                }
                black_box(application.semantic_state());
                elapsed
            });
        });
        group.finish();
    }
}

fn overlay_cold_initial_render(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/overlay-turn/cold-initial-render");
    group.throughput(Throughput::Elements(1));
    group.bench_function("arborui", |bencher| {
        bencher.iter(|| {
            black_box(TestApp::with_options(
                OverlayLab::new(OVERLAY_WIDTH, OVERLAY_HEIGHT),
                overlay_size(),
                measurement_options(),
            ));
        });
    });
    group.bench_function("ratatui", |bencher| {
        bencher.iter(|| {
            let application = RatatuiOverlayLab::new(OVERLAY_WIDTH, OVERLAY_HEIGHT);
            let mut terminal = Terminal::new(TestBackend::new(OVERLAY_WIDTH, OVERLAY_HEIGHT))
                .expect("test terminal must open");
            draw_test_overlay_terminal(&mut terminal, &application)
                .expect("initial overlay frame must draw");
            black_box((application, terminal));
        });
    });
    group.finish();
}

fn overlay_scenario_turns(criterion: &mut Criterion) {
    for scenario in OverlayScenario::ALL {
        let mut group =
            criterion.benchmark_group(format!("comparison/overlay-turn/{}", scenario.name()));
        group.throughput(Throughput::Elements(1));
        group.bench_function("arborui", |bencher| {
            let mut application = TestApp::with_options(
                OverlayLab::new(OVERLAY_WIDTH, OVERLAY_HEIGHT),
                overlay_size(),
                measurement_options(),
            );
            prepare_arborui_overlay(&mut application, scenario);
            bencher.iter_custom(|iterations| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iterations {
                    let started = std::time::Instant::now();
                    arborui_overlay_turn(&mut application, scenario);
                    elapsed = elapsed.saturating_add(started.elapsed());
                    reset_arborui_overlay(&mut application, scenario);
                }
                black_box(application.application().model());
                elapsed
            });
        });
        group.bench_function("ratatui", |bencher| {
            let (mut application, mut terminal) = ratatui_overlay_fixture();
            prepare_ratatui_overlay(&mut application, &mut terminal, scenario);
            bencher.iter_custom(|iterations| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iterations {
                    let started = std::time::Instant::now();
                    ratatui_overlay_scenario_turn(&mut application, &mut terminal, scenario);
                    elapsed = elapsed.saturating_add(started.elapsed());
                    reset_ratatui_overlay(&mut application, &mut terminal, scenario);
                }
                black_box(application.semantic_state());
                elapsed
            });
        });
        group.finish();
    }
}

fn log_line_scrolling(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/log-turn/line-scrolling");
    group.throughput(Throughput::Elements(1));
    for item_count in [1_000usize, 100_000, 1_000_000] {
        group.bench_with_input(
            BenchmarkId::new("arborui", item_count),
            &item_count,
            |bencher, count| {
                let mut application = TestApp::with_options(
                    LogLab::new(*count, *count, BASE_WIDTH, BASE_HEIGHT),
                    base_size(),
                    measurement_options(),
                );
                let mut up = true;
                bencher.iter(|| {
                    let action = if up { LogAction::Up } else { LogAction::Down };
                    up = !up;
                    black_box(application.send(action));
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("ratatui", item_count),
            &item_count,
            |bencher, count| {
                let (mut application, mut terminal) = ratatui_log_fixture(*count);
                let mut up = true;
                bencher.iter(|| {
                    let action = if up { LogAction::Up } else { LogAction::Down };
                    up = !up;
                    ratatui_log_turn(&mut application, &mut terminal, action);
                    black_box(application.semantic_state());
                });
            },
        );
    }
    group.finish();
}

fn log_cold_initial_render(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/log-turn/cold-initial-render");
    group.throughput(Throughput::Elements(1));
    group.bench_function("arborui", |bencher| {
        bencher.iter(|| {
            black_box(TestApp::with_options(
                LogLab::new(ITEM_COUNT, ITEM_COUNT, BASE_WIDTH, BASE_HEIGHT),
                base_size(),
                measurement_options(),
            ));
        });
    });
    group.bench_function("ratatui", |bencher| {
        bencher.iter(|| {
            let mut application =
                RatatuiLogLab::new(ITEM_COUNT, ITEM_COUNT, BASE_WIDTH, BASE_HEIGHT);
            let mut terminal = Terminal::new(TestBackend::new(BASE_WIDTH, BASE_HEIGHT))
                .expect("test terminal must open");
            draw_test_log_terminal(&mut terminal, &mut application)
                .expect("initial scrolling-log frame must draw");
            black_box((application, terminal));
        });
    });
    group.finish();
}

fn log_scenario_turns(criterion: &mut Criterion) {
    for scenario in LogScenario::ALL {
        let mut group =
            criterion.benchmark_group(format!("comparison/log-turn/{}", scenario.name()));
        group.throughput(Throughput::Elements(1));
        group.bench_function("arborui", |bencher| {
            let mut application = TestApp::with_options(
                LogLab::new(
                    ITEM_COUNT,
                    ITEM_COUNT.saturating_add(1_000_000),
                    BASE_WIDTH,
                    BASE_HEIGHT,
                ),
                base_size(),
                measurement_options(),
            );
            prepare_arborui_log(&mut application, scenario);
            let mut generation = 1u64;
            bencher.iter_custom(|iterations| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iterations {
                    let started = std::time::Instant::now();
                    arborui_log_turn(&mut application, scenario, generation);
                    elapsed = elapsed.saturating_add(started.elapsed());
                    reset_arborui_log(&mut application, scenario);
                    generation = generation.saturating_add(1);
                }
                black_box(application.application().model().generation());
                elapsed
            });
        });
        group.bench_function("ratatui", |bencher| {
            let (mut application, mut terminal) =
                ratatui_log_fixture_with_limit(ITEM_COUNT, ITEM_COUNT.saturating_add(1_000_000));
            prepare_ratatui_log(&mut application, &mut terminal, scenario);
            let mut generation = 1u64;
            bencher.iter_custom(|iterations| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iterations {
                    let started = std::time::Instant::now();
                    ratatui_log_scenario_turn(
                        &mut application,
                        &mut terminal,
                        scenario,
                        generation,
                    );
                    elapsed = elapsed.saturating_add(started.elapsed());
                    reset_ratatui_log(&mut application, &mut terminal, scenario);
                    generation = generation.saturating_add(1);
                }
                black_box(application.semantic_state());
                elapsed
            });
        });
        group.finish();
    }
}

fn table_line_navigation(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/table-turn/line-navigation");
    group.throughput(Throughput::Elements(1));
    for item_count in [1_000usize, 100_000, 1_000_000] {
        group.bench_with_input(
            BenchmarkId::new("arborui", item_count),
            &item_count,
            |bencher, count| {
                let mut application = TestApp::with_options(
                    TableLab::new(*count, BASE_WIDTH, BASE_HEIGHT),
                    base_size(),
                    measurement_options(),
                );
                let mut down = true;
                bencher.iter(|| {
                    let action = if down {
                        TableAction::Down
                    } else {
                        TableAction::Up
                    };
                    down = !down;
                    black_box(application.send(action));
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("ratatui", item_count),
            &item_count,
            |bencher, count| {
                let (mut application, mut terminal) = ratatui_table_fixture(*count);
                let mut down = true;
                bencher.iter(|| {
                    let action = if down {
                        TableAction::Down
                    } else {
                        TableAction::Up
                    };
                    down = !down;
                    ratatui_table_turn(&mut application, &mut terminal, action);
                    black_box(application.semantic_state());
                });
            },
        );
    }
    group.finish();
}

fn table_cold_initial_render(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/table-turn/cold-initial-render");
    group.throughput(Throughput::Elements(1));
    group.bench_function("arborui", |bencher| {
        bencher.iter(|| {
            black_box(TestApp::with_options(
                TableLab::new(ITEM_COUNT, BASE_WIDTH, BASE_HEIGHT),
                base_size(),
                measurement_options(),
            ));
        });
    });
    group.bench_function("ratatui", |bencher| {
        bencher.iter(|| {
            let mut application = RatatuiTableLab::new(ITEM_COUNT, BASE_WIDTH, BASE_HEIGHT);
            let mut terminal = Terminal::new(TestBackend::new(BASE_WIDTH, BASE_HEIGHT))
                .expect("test terminal must open");
            draw_test_table_terminal(&mut terminal, &mut application)
                .expect("initial table frame must draw");
            black_box((application, terminal));
        });
    });
    group.finish();
}

fn table_scenario_turns(criterion: &mut Criterion) {
    for scenario in TableScenario::ALL {
        let mut group =
            criterion.benchmark_group(format!("comparison/table-turn/{}", scenario.name()));
        group.throughput(Throughput::Elements(1));
        group.bench_function("arborui", |bencher| {
            let mut application = TestApp::with_options(
                TableLab::new(ITEM_COUNT, BASE_WIDTH, BASE_HEIGHT),
                base_size(),
                measurement_options(),
            );
            prepare_arborui_table(&mut application, scenario);
            let mut revision = 1u64;
            bencher.iter_custom(|iterations| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iterations {
                    let started = std::time::Instant::now();
                    arborui_table_turn(&mut application, scenario, revision);
                    elapsed = elapsed.saturating_add(started.elapsed());
                    reset_arborui_table(&mut application, scenario);
                    revision = revision.saturating_add(1);
                }
                black_box(application.application().model().generation());
                elapsed
            });
        });
        group.bench_function("ratatui", |bencher| {
            let (mut application, mut terminal) = ratatui_table_fixture(ITEM_COUNT);
            prepare_ratatui_table(&mut application, &mut terminal, scenario);
            let mut revision = 1u64;
            bencher.iter_custom(|iterations| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iterations {
                    let started = std::time::Instant::now();
                    ratatui_table_scenario_turn(
                        &mut application,
                        &mut terminal,
                        scenario,
                        revision,
                    );
                    elapsed = elapsed.saturating_add(started.elapsed());
                    reset_ratatui_table(&mut application, &mut terminal, scenario);
                    revision = revision.saturating_add(1);
                }
                black_box(application.semantic_state());
                elapsed
            });
        });
        group.finish();
    }
}

fn line_navigation(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/collection-turn/line-navigation");
    group.throughput(Throughput::Elements(1));
    for item_count in [1_000usize, 100_000, 1_000_000] {
        for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
            let mode_name = mode_name(mode);
            group.bench_with_input(
                BenchmarkId::new(format!("arborui/{mode_name}"), item_count),
                &item_count,
                |bencher, count| {
                    let mut application = TestApp::with_options(
                        CollectionLab::new(mode, *count, viewport_height(BASE_HEIGHT)),
                        base_size(),
                        measurement_options(),
                    );
                    let mut down = true;
                    bencher.iter_custom(|iterations| {
                        let started = std::time::Instant::now();
                        for _ in 0..iterations {
                            let message = if down { Message::Down } else { Message::Up };
                            down = !down;
                            black_box(application.send(message));
                        }
                        started.elapsed()
                    });
                },
            );
            group.bench_with_input(
                BenchmarkId::new(format!("ratatui/{mode_name}"), item_count),
                &item_count,
                |bencher, count| {
                    let (mut application, mut terminal) = ratatui_fixture(mode, *count);
                    let mut down = true;
                    bencher.iter_custom(|iterations| {
                        let mut elapsed = Duration::ZERO;
                        for _ in 0..iterations {
                            let action = if down {
                                ComparisonAction::Down
                            } else {
                                ComparisonAction::Up
                            };
                            down = !down;
                            let started = std::time::Instant::now();
                            ratatui_turn(&mut application, &mut terminal, action);
                            elapsed = elapsed.saturating_add(started.elapsed());
                        }
                        black_box(application.semantic_state());
                        elapsed
                    });
                },
            );
        }
    }
    group.finish();
}

fn cold_initial_render(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/collection-turn/cold-initial-render");
    group.throughput(Throughput::Elements(1));
    for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
        let mode_name = mode_name(mode);
        group.bench_function(BenchmarkId::new("arborui", mode_name), |bencher| {
            bencher.iter(|| {
                black_box(TestApp::with_options(
                    CollectionLab::new(mode, ITEM_COUNT, viewport_height(BASE_HEIGHT)),
                    base_size(),
                    measurement_options(),
                ));
            });
        });
        group.bench_function(BenchmarkId::new("ratatui", mode_name), |bencher| {
            bencher.iter(|| {
                let mut application =
                    RatatuiCollectionLab::new(mode, ITEM_COUNT, BASE_WIDTH, BASE_HEIGHT);
                let mut terminal = Terminal::new(TestBackend::new(BASE_WIDTH, BASE_HEIGHT))
                    .expect("test terminal must open");
                draw_test_terminal(&mut terminal, &mut application)
                    .expect("initial frame must draw");
                black_box((application, terminal));
            });
        });
    }
    group.finish();
}

fn scenario_turns(criterion: &mut Criterion) {
    for scenario in Scenario::ALL {
        let mut group =
            criterion.benchmark_group(format!("comparison/collection-turn/{}", scenario.name()));
        group.throughput(Throughput::Elements(1));
        for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
            let mode_name = mode_name(mode);
            group.bench_function(BenchmarkId::new("arborui", mode_name), |bencher| {
                let mut application = TestApp::with_options(
                    CollectionLab::new(mode, ITEM_COUNT, viewport_height(BASE_HEIGHT)),
                    base_size(),
                    measurement_options(),
                );
                prepare_arborui_selection(&mut application, scenario);
                bencher.iter_custom(|iterations| {
                    let mut elapsed = Duration::ZERO;
                    for _ in 0..iterations {
                        let started = std::time::Instant::now();
                        arborui_turn(&mut application, scenario);
                        elapsed = elapsed.saturating_add(started.elapsed());
                        reset_arborui(&mut application, scenario);
                    }
                    black_box(arborui_semantic_marker(&application));
                    elapsed
                });
            });
            group.bench_function(BenchmarkId::new("ratatui", mode_name), |bencher| {
                let (mut application, mut terminal) = ratatui_fixture(mode, ITEM_COUNT);
                prepare_ratatui_selection(&mut application, &mut terminal, scenario);
                bencher.iter_custom(|iterations| {
                    let mut elapsed = Duration::ZERO;
                    for _ in 0..iterations {
                        let started = std::time::Instant::now();
                        ratatui_scenario_turn(&mut application, &mut terminal, scenario);
                        elapsed = elapsed.saturating_add(started.elapsed());
                        reset_ratatui(&mut application, &mut terminal, scenario);
                    }
                    black_box(application.semantic_state());
                    elapsed
                });
            });
        }
        group.finish();
    }
}

fn arborui_turn(application: &mut TestApp<CollectionLab>, scenario: Scenario) {
    match scenario {
        Scenario::PageDown => {
            black_box(application.send(Message::PageDown));
        }
        Scenario::End => {
            black_box(application.send(Message::End));
        }
        Scenario::Resize => {
            black_box(application.resize(Size::new(BASE_WIDTH, RESIZED_HEIGHT)));
        }
        Scenario::Selection => {
            black_box(application.send(Message::SelectActive));
        }
        Scenario::Reverse => {
            black_box(application.send(Message::Reverse));
        }
        Scenario::UnchangedRedraw => {
            black_box(application.send(Message::Home));
        }
    }
}

fn reset_arborui(application: &mut TestApp<CollectionLab>, scenario: Scenario) {
    match scenario {
        Scenario::PageDown | Scenario::End => {
            application.send(Message::Home);
        }
        Scenario::Resize => {
            application.resize(base_size());
        }
        Scenario::Selection => {
            application.send(Message::Down);
            application.send(Message::SelectActive);
            application.send(Message::Home);
        }
        Scenario::Reverse => {
            application.send(Message::Reverse);
        }
        Scenario::UnchangedRedraw => {}
    }
}

fn prepare_arborui_selection(application: &mut TestApp<CollectionLab>, scenario: Scenario) {
    if matches!(scenario, Scenario::Selection) {
        application.send(Message::Down);
        application.send(Message::SelectActive);
        application.send(Message::Home);
    }
}

fn ratatui_scenario_turn(
    application: &mut RatatuiCollectionLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: Scenario,
) {
    match scenario {
        Scenario::PageDown => {
            ratatui_turn(application, terminal, ComparisonAction::PageDown);
        }
        Scenario::End => ratatui_turn(application, terminal, ComparisonAction::End),
        Scenario::Resize => ratatui_resize(application, terminal, RESIZED_HEIGHT),
        Scenario::Selection => {
            ratatui_turn(application, terminal, ComparisonAction::SelectActive);
        }
        Scenario::Reverse => ratatui_turn(application, terminal, ComparisonAction::Reverse),
        Scenario::UnchangedRedraw => {
            ratatui_turn(application, terminal, ComparisonAction::Home);
        }
    }
}

fn reset_ratatui(
    application: &mut RatatuiCollectionLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: Scenario,
) {
    match scenario {
        Scenario::PageDown | Scenario::End => {
            ratatui_turn(application, terminal, ComparisonAction::Home);
        }
        Scenario::Resize => ratatui_resize(application, terminal, BASE_HEIGHT),
        Scenario::Selection => {
            ratatui_turn(application, terminal, ComparisonAction::Down);
            ratatui_turn(application, terminal, ComparisonAction::SelectActive);
            ratatui_turn(application, terminal, ComparisonAction::Home);
        }
        Scenario::Reverse => {
            ratatui_turn(application, terminal, ComparisonAction::Reverse);
        }
        Scenario::UnchangedRedraw => {}
    }
}

fn prepare_ratatui_selection(
    application: &mut RatatuiCollectionLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: Scenario,
) {
    if matches!(scenario, Scenario::Selection) {
        ratatui_turn(application, terminal, ComparisonAction::Down);
        ratatui_turn(application, terminal, ComparisonAction::SelectActive);
        ratatui_turn(application, terminal, ComparisonAction::Home);
    }
}

fn ratatui_fixture(
    mode: CollectionMode,
    item_count: usize,
) -> (RatatuiCollectionLab, Terminal<TestBackend>) {
    let mut application = RatatuiCollectionLab::new(mode, item_count, BASE_WIDTH, BASE_HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(BASE_WIDTH, BASE_HEIGHT)).expect("test terminal must open");
    draw_test_terminal(&mut terminal, &mut application).expect("initial frame must draw");
    (application, terminal)
}

fn ratatui_turn(
    application: &mut RatatuiCollectionLab,
    terminal: &mut Terminal<TestBackend>,
    action: ComparisonAction,
) {
    application.apply(action);
    draw_test_terminal(terminal, application).expect("frame must draw");
}

fn ratatui_resize(
    application: &mut RatatuiCollectionLab,
    terminal: &mut Terminal<TestBackend>,
    height: u16,
) {
    terminal.backend_mut().resize(BASE_WIDTH, height);
    ratatui_turn(
        application,
        terminal,
        ComparisonAction::Resize {
            width: BASE_WIDTH,
            height,
        },
    );
}

fn arborui_semantic_marker(application: &TestApp<CollectionLab>) -> (Option<u64>, Option<u64>) {
    (
        application.application().active_key(),
        application.application().selected_key(),
    )
}

fn arborui_table_turn(application: &mut TestApp<TableLab>, scenario: TableScenario, revision: u64) {
    match scenario {
        TableScenario::PageDown => {
            black_box(application.send(TableAction::PageDown));
        }
        TableScenario::Selection => {
            black_box(application.send(TableAction::SelectActive));
        }
        TableScenario::Resize => {
            black_box(application.resize(Size::new(BASE_WIDTH, RESIZED_HEIGHT)));
        }
        TableScenario::VisibleUpdate => {
            black_box(application.send(TableAction::BackgroundUpdate { key: 0, revision }));
        }
        TableScenario::OffscreenUpdate => {
            black_box(application.send(TableAction::BackgroundUpdate {
                key: u64::try_from(ITEM_COUNT - 1).unwrap_or(u64::MAX),
                revision,
            }));
        }
    }
}

fn reset_arborui_table(application: &mut TestApp<TableLab>, scenario: TableScenario) {
    match scenario {
        TableScenario::PageDown => {
            application.send(TableAction::Home);
        }
        TableScenario::Selection => {
            application.send(TableAction::Down);
            application.send(TableAction::SelectActive);
            application.send(TableAction::Home);
        }
        TableScenario::Resize => {
            application.resize(base_size());
        }
        TableScenario::VisibleUpdate | TableScenario::OffscreenUpdate => {}
    }
}

fn prepare_arborui_table(application: &mut TestApp<TableLab>, scenario: TableScenario) {
    if matches!(scenario, TableScenario::Selection) {
        reset_arborui_table(application, scenario);
    }
}

fn ratatui_table_scenario_turn(
    application: &mut RatatuiTableLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: TableScenario,
    revision: u64,
) {
    let action = match scenario {
        TableScenario::PageDown => TableAction::PageDown,
        TableScenario::Selection => TableAction::SelectActive,
        TableScenario::Resize => {
            terminal.backend_mut().resize(BASE_WIDTH, RESIZED_HEIGHT);
            TableAction::Resize {
                width: BASE_WIDTH,
                height: RESIZED_HEIGHT,
            }
        }
        TableScenario::VisibleUpdate => TableAction::BackgroundUpdate { key: 0, revision },
        TableScenario::OffscreenUpdate => TableAction::BackgroundUpdate {
            key: u64::try_from(ITEM_COUNT - 1).unwrap_or(u64::MAX),
            revision,
        },
    };
    ratatui_table_turn(application, terminal, action);
}

fn reset_ratatui_table(
    application: &mut RatatuiTableLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: TableScenario,
) {
    match scenario {
        TableScenario::PageDown => {
            ratatui_table_turn(application, terminal, TableAction::Home);
        }
        TableScenario::Selection => {
            for action in [
                TableAction::Down,
                TableAction::SelectActive,
                TableAction::Home,
            ] {
                ratatui_table_turn(application, terminal, action);
            }
        }
        TableScenario::Resize => {
            terminal.backend_mut().resize(BASE_WIDTH, BASE_HEIGHT);
            ratatui_table_turn(
                application,
                terminal,
                TableAction::Resize {
                    width: BASE_WIDTH,
                    height: BASE_HEIGHT,
                },
            );
        }
        TableScenario::VisibleUpdate | TableScenario::OffscreenUpdate => {}
    }
}

fn prepare_ratatui_table(
    application: &mut RatatuiTableLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: TableScenario,
) {
    if matches!(scenario, TableScenario::Selection) {
        reset_ratatui_table(application, terminal, scenario);
    }
}

fn ratatui_table_fixture(item_count: usize) -> (RatatuiTableLab, Terminal<TestBackend>) {
    let mut application = RatatuiTableLab::new(item_count, BASE_WIDTH, BASE_HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(BASE_WIDTH, BASE_HEIGHT)).expect("test terminal must open");
    draw_test_table_terminal(&mut terminal, &mut application)
        .expect("initial table frame must draw");
    (application, terminal)
}

fn ratatui_table_turn(
    application: &mut RatatuiTableLab,
    terminal: &mut Terminal<TestBackend>,
    action: TableAction,
) {
    application.apply(action);
    draw_test_table_terminal(terminal, application).expect("table frame must draw");
}

fn prepare_arborui_log(application: &mut TestApp<LogLab>, scenario: LogScenario) {
    if matches!(scenario, LogScenario::AppendPaused) {
        application.send(LogAction::PageUp);
    }
}

fn arborui_log_turn(application: &mut TestApp<LogLab>, scenario: LogScenario, generation: u64) {
    match scenario {
        LogScenario::PageUp => {
            black_box(application.send(LogAction::PageUp));
        }
        LogScenario::Resize => {
            black_box(application.resize(Size::new(BASE_WIDTH, RESIZED_HEIGHT)));
        }
        LogScenario::AppendFollowing | LogScenario::AppendPaused => {
            black_box(application.send(LogAction::Append {
                count: 1,
                generation,
            }));
        }
    }
}

fn reset_arborui_log(application: &mut TestApp<LogLab>, scenario: LogScenario) {
    match scenario {
        LogScenario::PageUp => {
            application.send(LogAction::End);
        }
        LogScenario::Resize => {
            application.resize(base_size());
        }
        LogScenario::AppendFollowing | LogScenario::AppendPaused => {}
    }
}

fn prepare_ratatui_log(
    application: &mut RatatuiLogLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: LogScenario,
) {
    if matches!(scenario, LogScenario::AppendPaused) {
        ratatui_log_turn(application, terminal, LogAction::PageUp);
    }
}

fn ratatui_log_scenario_turn(
    application: &mut RatatuiLogLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: LogScenario,
    generation: u64,
) {
    match scenario {
        LogScenario::PageUp => {
            ratatui_log_turn(application, terminal, LogAction::PageUp);
        }
        LogScenario::Resize => {
            terminal.backend_mut().resize(BASE_WIDTH, RESIZED_HEIGHT);
            ratatui_log_turn(
                application,
                terminal,
                LogAction::Resize {
                    width: BASE_WIDTH,
                    height: RESIZED_HEIGHT,
                },
            );
        }
        LogScenario::AppendFollowing | LogScenario::AppendPaused => ratatui_log_turn(
            application,
            terminal,
            LogAction::Append {
                count: 1,
                generation,
            },
        ),
    }
}

fn reset_ratatui_log(
    application: &mut RatatuiLogLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: LogScenario,
) {
    match scenario {
        LogScenario::PageUp => ratatui_log_turn(application, terminal, LogAction::End),
        LogScenario::Resize => {
            terminal.backend_mut().resize(BASE_WIDTH, BASE_HEIGHT);
            ratatui_log_turn(
                application,
                terminal,
                LogAction::Resize {
                    width: BASE_WIDTH,
                    height: BASE_HEIGHT,
                },
            );
        }
        LogScenario::AppendFollowing | LogScenario::AppendPaused => {}
    }
}

fn ratatui_log_fixture(item_count: usize) -> (RatatuiLogLab, Terminal<TestBackend>) {
    ratatui_log_fixture_with_limit(item_count, item_count)
}

fn ratatui_log_fixture_with_limit(
    item_count: usize,
    history_limit: usize,
) -> (RatatuiLogLab, Terminal<TestBackend>) {
    let mut application = RatatuiLogLab::new(item_count, history_limit, BASE_WIDTH, BASE_HEIGHT);
    let mut terminal =
        Terminal::new(TestBackend::new(BASE_WIDTH, BASE_HEIGHT)).expect("test terminal must open");
    draw_test_log_terminal(&mut terminal, &mut application)
        .expect("initial scrolling-log frame must draw");
    (application, terminal)
}

fn ratatui_log_turn(
    application: &mut RatatuiLogLab,
    terminal: &mut Terminal<TestBackend>,
    action: LogAction,
) {
    application.apply(action);
    draw_test_log_terminal(terminal, application).expect("scrolling-log frame must draw");
}

fn prepare_arborui_overlay(application: &mut TestApp<OverlayLab>, scenario: OverlayScenario) {
    if matches!(
        scenario,
        OverlayScenario::FocusNext
            | OverlayScenario::Cancel
            | OverlayScenario::Confirm
            | OverlayScenario::ResizeOpen
    ) {
        application.send(OverlayAction::Open);
    }
}

fn arborui_overlay_turn(application: &mut TestApp<OverlayLab>, scenario: OverlayScenario) {
    match scenario {
        OverlayScenario::Open => {
            black_box(application.send(OverlayAction::Open));
        }
        OverlayScenario::FocusNext => {
            black_box(application.key(KeyCode::Tab));
        }
        OverlayScenario::Cancel => {
            black_box(application.send(OverlayAction::Cancel));
        }
        OverlayScenario::Confirm => {
            black_box(application.send(OverlayAction::Confirm));
        }
        OverlayScenario::BackgroundActivation => {
            black_box(application.send(OverlayAction::ActivateBackground));
        }
        OverlayScenario::ResizeOpen => {
            black_box(application.resize(Size::new(OVERLAY_RESIZED_WIDTH, OVERLAY_RESIZED_HEIGHT)));
        }
    }
}

fn reset_arborui_overlay(application: &mut TestApp<OverlayLab>, scenario: OverlayScenario) {
    match scenario {
        OverlayScenario::Open => {
            application.send(OverlayAction::Cancel);
        }
        OverlayScenario::FocusNext => {
            application.key(KeyCode::Tab);
        }
        OverlayScenario::Cancel | OverlayScenario::Confirm => {
            application.send(OverlayAction::Open);
        }
        OverlayScenario::BackgroundActivation => {}
        OverlayScenario::ResizeOpen => {
            application.resize(overlay_size());
        }
    }
}

fn prepare_ratatui_overlay(
    application: &mut RatatuiOverlayLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: OverlayScenario,
) {
    if matches!(
        scenario,
        OverlayScenario::FocusNext
            | OverlayScenario::Cancel
            | OverlayScenario::Confirm
            | OverlayScenario::ResizeOpen
    ) {
        ratatui_overlay_turn(application, terminal, OverlayAction::Open);
    }
}

fn ratatui_overlay_scenario_turn(
    application: &mut RatatuiOverlayLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: OverlayScenario,
) {
    match scenario {
        OverlayScenario::Open => ratatui_overlay_turn(application, terminal, OverlayAction::Open),
        OverlayScenario::FocusNext => {
            application.focus_next();
            draw_test_overlay_terminal(terminal, application).expect("overlay frame must draw");
        }
        OverlayScenario::Cancel => {
            ratatui_overlay_turn(application, terminal, OverlayAction::Cancel);
        }
        OverlayScenario::Confirm => {
            ratatui_overlay_turn(application, terminal, OverlayAction::Confirm);
        }
        OverlayScenario::BackgroundActivation => {
            ratatui_overlay_turn(application, terminal, OverlayAction::ActivateBackground);
        }
        OverlayScenario::ResizeOpen => ratatui_overlay_resize(
            application,
            terminal,
            OVERLAY_RESIZED_WIDTH,
            OVERLAY_RESIZED_HEIGHT,
        ),
    }
}

fn reset_ratatui_overlay(
    application: &mut RatatuiOverlayLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: OverlayScenario,
) {
    match scenario {
        OverlayScenario::Open => {
            ratatui_overlay_turn(application, terminal, OverlayAction::Cancel);
        }
        OverlayScenario::FocusNext => {
            application.focus_next();
            draw_test_overlay_terminal(terminal, application).expect("overlay frame must draw");
        }
        OverlayScenario::Cancel | OverlayScenario::Confirm => {
            ratatui_overlay_turn(application, terminal, OverlayAction::Open);
        }
        OverlayScenario::BackgroundActivation => {}
        OverlayScenario::ResizeOpen => {
            ratatui_overlay_resize(application, terminal, OVERLAY_WIDTH, OVERLAY_HEIGHT)
        }
    }
}

fn ratatui_overlay_fixture() -> (RatatuiOverlayLab, Terminal<TestBackend>) {
    let application = RatatuiOverlayLab::new(OVERLAY_WIDTH, OVERLAY_HEIGHT);
    let mut terminal = Terminal::new(TestBackend::new(OVERLAY_WIDTH, OVERLAY_HEIGHT))
        .expect("test terminal must open");
    draw_test_overlay_terminal(&mut terminal, &application)
        .expect("initial overlay frame must draw");
    (application, terminal)
}

fn ratatui_overlay_turn(
    application: &mut RatatuiOverlayLab,
    terminal: &mut Terminal<TestBackend>,
    action: OverlayAction,
) {
    application.apply(action);
    draw_test_overlay_terminal(terminal, application).expect("overlay frame must draw");
}

fn ratatui_overlay_resize(
    application: &mut RatatuiOverlayLab,
    terminal: &mut Terminal<TestBackend>,
    width: u16,
    height: u16,
) {
    terminal.backend_mut().resize(width, height);
    ratatui_overlay_turn(
        application,
        terminal,
        OverlayAction::Resize { width, height },
    );
}

fn prepare_arborui_unicode(application: &mut TestApp<UnicodeLab>, scenario: UnicodeScenario) {
    if matches!(scenario, UnicodeScenario::ShiftBoundary) {
        for _ in 0..UNICODE_BOUNDARY_OFFSET {
            application.send(UnicodeAction::ShiftRight);
        }
    }
}

fn arborui_unicode_turn(application: &mut TestApp<UnicodeLab>, scenario: UnicodeScenario) {
    match scenario {
        UnicodeScenario::ShiftBoundary => {
            black_box(application.send(UnicodeAction::ShiftRight));
        }
        UnicodeScenario::ReplaceWide => {
            black_box(application.send(UnicodeAction::ReplaceWide));
        }
        UnicodeScenario::ResizeNarrow => {
            black_box(application.resize(Size::new(UNICODE_NARROW_WIDTH, UNICODE_HEIGHT)));
        }
    }
}

fn reset_arborui_unicode(application: &mut TestApp<UnicodeLab>, scenario: UnicodeScenario) {
    match scenario {
        UnicodeScenario::ShiftBoundary => {
            application.send(UnicodeAction::ShiftLeft);
        }
        UnicodeScenario::ReplaceWide => {
            application.send(UnicodeAction::ReplaceWide);
        }
        UnicodeScenario::ResizeNarrow => {
            application.resize(unicode_size());
        }
    }
}

fn prepare_ratatui_unicode(
    application: &mut RatatuiUnicodeLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: UnicodeScenario,
) {
    if matches!(scenario, UnicodeScenario::ShiftBoundary) {
        for _ in 0..UNICODE_BOUNDARY_OFFSET {
            ratatui_unicode_turn(application, terminal, UnicodeAction::ShiftRight);
        }
    }
}

fn ratatui_unicode_scenario_turn(
    application: &mut RatatuiUnicodeLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: UnicodeScenario,
) {
    match scenario {
        UnicodeScenario::ShiftBoundary => {
            ratatui_unicode_turn(application, terminal, UnicodeAction::ShiftRight);
        }
        UnicodeScenario::ReplaceWide => {
            ratatui_unicode_turn(application, terminal, UnicodeAction::ReplaceWide);
        }
        UnicodeScenario::ResizeNarrow => {
            ratatui_unicode_resize(application, terminal, UNICODE_NARROW_WIDTH);
        }
    }
}

fn reset_ratatui_unicode(
    application: &mut RatatuiUnicodeLab,
    terminal: &mut Terminal<TestBackend>,
    scenario: UnicodeScenario,
) {
    match scenario {
        UnicodeScenario::ShiftBoundary => {
            ratatui_unicode_turn(application, terminal, UnicodeAction::ShiftLeft);
        }
        UnicodeScenario::ReplaceWide => {
            ratatui_unicode_turn(application, terminal, UnicodeAction::ReplaceWide);
        }
        UnicodeScenario::ResizeNarrow => {
            ratatui_unicode_resize(application, terminal, UNICODE_WIDTH);
        }
    }
}

fn ratatui_unicode_fixture() -> (RatatuiUnicodeLab, Terminal<TestBackend>) {
    let application = RatatuiUnicodeLab::new(UNICODE_WIDTH, UNICODE_HEIGHT);
    let mut terminal = Terminal::new(TestBackend::new(UNICODE_WIDTH, UNICODE_HEIGHT))
        .expect("test terminal must open");
    draw_test_unicode_terminal(&mut terminal, &application)
        .expect("initial Unicode frame must draw");
    (application, terminal)
}

fn ratatui_unicode_turn(
    application: &mut RatatuiUnicodeLab,
    terminal: &mut Terminal<TestBackend>,
    action: UnicodeAction,
) {
    application.apply(action);
    draw_test_unicode_terminal(terminal, application).expect("Unicode frame must draw");
}

fn ratatui_unicode_resize(
    application: &mut RatatuiUnicodeLab,
    terminal: &mut Terminal<TestBackend>,
    width: u16,
) {
    terminal.backend_mut().resize(width, UNICODE_HEIGHT);
    ratatui_unicode_turn(
        application,
        terminal,
        UnicodeAction::Resize {
            width,
            height: UNICODE_HEIGHT,
        },
    );
}

fn viewport_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(4).max(1) as usize
}

fn measurement_options() -> TestAppOptions {
    TestAppOptions {
        record_patches: false,
        ..TestAppOptions::default()
    }
}

const fn base_size() -> Size {
    Size::new(BASE_WIDTH, BASE_HEIGHT)
}

const fn overlay_size() -> Size {
    Size::new(OVERLAY_WIDTH, OVERLAY_HEIGHT)
}

const fn unicode_size() -> Size {
    Size::new(UNICODE_WIDTH, UNICODE_HEIGHT)
}

const fn mode_name(mode: CollectionMode) -> &'static str {
    match mode {
        CollectionMode::Fixed => "fixed",
        CollectionMode::Variable => "variable",
    }
}

criterion_group!(benches, application_turns);
criterion_main!(benches);
