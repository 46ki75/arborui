set shell := ["bash", "-cu"]

default: ci

fmt:
    cargo fmt --all
    pnpm run lint:fix

fmt-check:
    cargo fmt --all -- --check
    pnpm run lint

lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    cargo test --workspace --all-features

snapshot-review:
    cargo insta review

run-focus-queue:
    cargo run -p arborui-example-focus-queue

test-pty:
    cargo test -p arborui-backend-crossterm --test pty_lifecycle -- --ignored --test-threads=1

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

deny:
    cargo deny --locked check
    cargo deny --locked --manifest-path fuzz/Cargo.toml check
    cargo deny --locked --manifest-path comparisons/collection-lab-ratatui/Cargo.toml check

architecture-check:
    node scripts/check-architecture-dependencies.mjs

ci: fmt-check architecture-check lint test doc

test-cov:
    cargo llvm-cov --no-report --workspace --all-features

coverage: test-cov
    cargo llvm-cov report --show-missing-lines --color=always

coverage-html: test-cov
    cargo llvm-cov report --html --open

coverage-ci: test-cov
    cargo llvm-cov report --lcov --output-path lcov.info

bench-smoke:
    cargo test -p arborui --bench stabilization --all-features
    cargo test -p arborui-example-collection-lab --bench visible_ranges --all-features

bench:
    cargo bench -p arborui --bench stabilization --all-features -- --noplot
    cargo bench -p arborui-example-collection-lab --bench visible_ranges --all-features -- --noplot

focus-queue-ingress-metrics:
    cargo test --release -p arborui-example-focus-queue --test live_ingress_metrics --all-features -- --ignored --nocapture

focus-queue-slow-sink-metrics:
    cargo test --release -p arborui-example-focus-queue --test slow_sink_metrics --all-features -- --ignored --nocapture

collection-lab-damage-metrics:
    cargo test --release -p arborui-example-collection-lab --test damage_metrics --all-features -- --ignored --nocapture

comparison-check:
    CARGO_TARGET_DIR="{{justfile_directory()}}/target/comparisons/collection-lab-ratatui" cargo +1.88.0 fmt --manifest-path comparisons/collection-lab-ratatui/Cargo.toml -- --check
    CARGO_TARGET_DIR="{{justfile_directory()}}/target/comparisons/collection-lab-ratatui" cargo +1.88.0 clippy --manifest-path comparisons/collection-lab-ratatui/Cargo.toml --all-targets --locked -- -D warnings
    CARGO_TARGET_DIR="{{justfile_directory()}}/target/comparisons/collection-lab-ratatui" cargo +1.88.0 test --manifest-path comparisons/collection-lab-ratatui/Cargo.toml --locked

comparison-bench-smoke:
    CARGO_TARGET_DIR="{{justfile_directory()}}/target/comparisons/collection-lab-ratatui" cargo +1.88.0 test --manifest-path comparisons/collection-lab-ratatui/Cargo.toml --bench application_turns --locked

comparison-bench:
    CARGO_TARGET_DIR="{{justfile_directory()}}/target/comparisons/collection-lab-ratatui" cargo +1.88.0 bench --manifest-path comparisons/collection-lab-ratatui/Cargo.toml --bench application_turns --locked -- --noplot

comparison-output-metrics:
    CARGO_TARGET_DIR="{{justfile_directory()}}/target/comparisons/collection-lab-ratatui" cargo +1.88.0 test --manifest-path comparisons/collection-lab-ratatui/Cargo.toml --test output_metrics --locked -- --nocapture

comparison-memory-metrics:
    CARGO_TARGET_DIR="{{justfile_directory()}}/target/comparisons/collection-lab-ratatui" cargo +1.88.0 test --release --manifest-path comparisons/collection-lab-ratatui/Cargo.toml --test memory_metrics --locked -- --ignored --nocapture

comparison-phase-metrics:
    CARGO_TARGET_DIR="{{justfile_directory()}}/target/comparisons/collection-lab-ratatui" cargo +1.88.0 test --release --manifest-path comparisons/collection-lab-ratatui/Cargo.toml --test phase_metrics --locked -- --ignored --nocapture

package-check:
    bash scripts/check-package-contents.sh

publish-dry-run:
    bash scripts/publish.sh --dry-run
