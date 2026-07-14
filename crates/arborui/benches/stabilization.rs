#![allow(missing_docs)]
//! Stable public-path benchmarks for text measurement and frame preparation.

use std::hint::black_box;

use arborui::{CursorState, Point, Size, Style, WidthPolicy, measure, render::Renderer};
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn text_measurement(criterion: &mut Criterion) {
    let unicode =
        "ASCII a\u{301} \u{754c} \u{1f469}\u{200d}\u{1f4bb} \u{1f1e6}\u{1f1e7} \u{2764}\u{fe0f} "
            .repeat(256);
    let mut group = criterion.benchmark_group("text/measure");
    group.throughput(Throughput::Bytes(unicode.len() as u64));
    for policy in [WidthPolicy::Unicode, WidthPolicy::Cjk, WidthPolicy::WcWidth] {
        group.bench_with_input(
            BenchmarkId::new("unicode_10k", format!("{policy:?}")),
            &policy,
            |bencher, policy| {
                bencher.iter(|| measure(black_box(&unicode), black_box(*policy)));
            },
        );
    }
    group.finish();
}

fn render_preparation(criterion: &mut Criterion) {
    let size = Size::new(80, 24);
    let mut group = criterion.benchmark_group("render/prepare");
    group.throughput(Throughput::Elements(u64::from(size.area())));

    group.bench_function("80x24/one_cell_change", |bencher| {
        bencher.iter_batched(
            || initialized_renderer(size),
            |mut renderer| {
                let frame = renderer
                    .prepare(size, CursorState::HIDDEN, |canvas| {
                        canvas.draw_text(
                            Point::new(40, 12),
                            black_box("x"),
                            Style::default(),
                            None,
                        )?;
                        Ok(())
                    })
                    .expect("benchmark frame must render");
                black_box(frame.patch().runs.len());
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("80x24/full_repaint", |bencher| {
        bencher.iter(|| {
            let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
            let frame = renderer
                .prepare(size, CursorState::HIDDEN, |canvas| {
                    for y in 0..i32::from(size.height) {
                        canvas.draw_text(
                            Point::new(0, y),
                            black_box("Unicode \u{754c} \u{1f469}\u{200d}\u{1f4bb} status line"),
                            Style::default(),
                            None,
                        )?;
                    }
                    Ok(())
                })
                .expect("benchmark frame must render");
            black_box(frame.patch().runs.len());
        });
    });
    group.finish();
}

fn initialized_renderer(size: Size) -> Renderer {
    let mut renderer = Renderer::new(size, WidthPolicy::Unicode);
    let frame = renderer
        .prepare(size, CursorState::HIDDEN, |_| Ok(()))
        .expect("initial benchmark frame must render");
    renderer
        .commit(frame)
        .expect("initial benchmark frame must commit");
    renderer
}

criterion_group!(benches, text_measurement, render_preparation);
criterion_main!(benches);
