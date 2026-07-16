#![allow(missing_docs)]

use std::{hint::black_box, time::Duration};

use arborui_comparison_collection_lab_ratatui::{
    ComparisonAction, RatatuiCollectionLab, draw_test_terminal,
};
use arborui_example_collection_lab::{CollectionLab, CollectionMode, Message};
use arborui_test::{Size, TestApp};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ratatui::{Terminal, backend::TestBackend};

fn application_turns(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("comparison/collection-turn");
    group.throughput(Throughput::Elements(1));
    for item_count in [1_000usize, 100_000, 1_000_000] {
        for mode in [CollectionMode::Fixed, CollectionMode::Variable] {
            let mode_name = match mode {
                CollectionMode::Fixed => "fixed",
                CollectionMode::Variable => "variable",
            };
            group.bench_with_input(
                BenchmarkId::new(format!("arborui/{mode_name}"), item_count),
                &item_count,
                |bencher, count| {
                    let mut application =
                        TestApp::new(CollectionLab::new(mode, *count, 8), Size::new(48, 12));
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
                    let mut application = RatatuiCollectionLab::new(mode, *count, 48, 12);
                    let mut terminal =
                        Terminal::new(TestBackend::new(48, 12)).expect("test terminal must open");
                    draw_test_terminal(&mut terminal, &mut application)
                        .expect("initial frame must draw");
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
                            application.apply(action);
                            draw_test_terminal(&mut terminal, &mut application)
                                .expect("measured frame must draw");
                            black_box(application.semantic_state());
                            elapsed = elapsed.saturating_add(started.elapsed());
                        }
                        elapsed
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, application_turns);
criterion_main!(benches);
