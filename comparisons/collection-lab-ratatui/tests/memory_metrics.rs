//! Isolated allocation and retained-memory measurements for matched scenarios.

use std::{collections::HashMap, process::Command};

const ACTION_ITEM_COUNT: usize = 100_000;
const ITEM_COUNTS: [usize; 3] = [1_000, 100_000, 1_000_000];
const FRAMEWORKS: [&str; 2] = ["arborui", "ratatui"];
const MODES: [&str; 2] = ["fixed", "variable"];
const ACTIONS: [&str; 5] = ["cold", "page-down", "resize", "reverse", "unchanged-redraw"];

#[derive(Clone, Copy, Debug)]
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

#[test]
#[ignore = "runs the release-mode heap measurement matrix"]
fn reports_isolated_memory_metrics() {
    println!(
        "| Framework | Mode | Scenario | Items | Allocations | Allocated bytes | Peak blocks | Peak bytes | Retained blocks | Retained bytes |"
    );
    println!("| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");

    for framework in FRAMEWORKS {
        for mode in MODES {
            for scenario in ACTIONS {
                let metrics = run_probe(framework, mode, scenario, ACTION_ITEM_COUNT);
                assert_released(framework, mode, scenario, metrics);
                print_metrics(framework, mode, scenario, ACTION_ITEM_COUNT, metrics);
            }

            let mut initial_render = Vec::new();
            for scenario in ["model", "initial-render"] {
                for item_count in ITEM_COUNTS {
                    let metrics = run_probe(framework, mode, scenario, item_count);
                    assert_released(framework, mode, scenario, metrics);
                    print_metrics(framework, mode, scenario, item_count, metrics);
                    if scenario == "initial-render" {
                        initial_render.push(metrics);
                    }
                }
            }
            assert_viewport_bounded(framework, mode, &initial_render);
        }
    }
}

fn run_probe(framework: &str, mode: &str, scenario: &str, item_count: usize) -> Metrics {
    let output = Command::new(env!("CARGO_BIN_EXE_memory_probe"))
        .args([framework, mode, scenario, &item_count.to_string()])
        .output()
        .expect("memory probe must start");
    assert!(
        output.status.success(),
        "memory probe failed for {framework}/{mode}/{scenario}/{item_count}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("probe output must be UTF-8");
    parse_metrics(stdout.trim())
}

fn parse_metrics(output: &str) -> Metrics {
    let fields = output
        .split_ascii_whitespace()
        .map(|field| {
            let (name, value) = field
                .split_once('=')
                .expect("metric must use name=value syntax");
            let value = value.parse::<u64>().expect("metric must be an integer");
            (name, value)
        })
        .collect::<HashMap<_, _>>();
    Metrics {
        total_blocks: metric(&fields, "total_blocks"),
        total_bytes: metric(&fields, "total_bytes"),
        max_blocks: usize_metric(&fields, "max_blocks"),
        max_bytes: usize_metric(&fields, "max_bytes"),
        curr_blocks: usize_metric(&fields, "curr_blocks"),
        curr_bytes: usize_metric(&fields, "curr_bytes"),
        end_blocks: usize_metric(&fields, "end_blocks"),
        end_bytes: usize_metric(&fields, "end_bytes"),
    }
}

fn metric(fields: &HashMap<&str, u64>, name: &str) -> u64 {
    *fields.get(name).expect("required metric must be present")
}

fn usize_metric(fields: &HashMap<&str, u64>, name: &str) -> usize {
    usize::try_from(metric(fields, name)).expect("metric must fit usize")
}

fn assert_released(framework: &str, mode: &str, scenario: &str, metrics: Metrics) {
    assert_eq!(
        (metrics.end_blocks, metrics.end_bytes),
        (0, 0),
        "tracked memory leaked after {framework}/{mode}/{scenario}"
    );
}

fn assert_viewport_bounded(framework: &str, mode: &str, metrics: &[Metrics]) {
    let smallest = metrics.first().expect("smallest measurement must exist");
    let largest = metrics.last().expect("largest measurement must exist");
    assert!(
        largest.curr_bytes <= smallest.curr_bytes.saturating_mul(2).saturating_add(16_384),
        "initial render state grew with logical rows for {framework}/{mode}: {} to {} bytes",
        smallest.curr_bytes,
        largest.curr_bytes
    );
    assert!(
        largest.curr_blocks <= smallest.curr_blocks.saturating_mul(2).saturating_add(64),
        "initial render blocks grew with logical rows for {framework}/{mode}: {} to {}",
        smallest.curr_blocks,
        largest.curr_blocks
    );
}

fn print_metrics(framework: &str, mode: &str, scenario: &str, item_count: usize, metrics: Metrics) {
    println!(
        "| {framework} | {mode} | {scenario} | {item_count} | {} | {} | {} | {} | {} | {} |",
        metrics.total_blocks,
        metrics.total_bytes,
        metrics.max_blocks,
        metrics.max_bytes,
        metrics.curr_blocks,
        metrics.curr_bytes
    );
}
