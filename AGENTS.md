# Repository Instructions

## Sources Of Truth

- Treat `Cargo.toml`, the `justfile`, workflows, and implementation as authoritative. The design documents include target architecture and may describe APIs that are not implemented yet.
- The root workspace contains `crates/*` and `examples/*`. `fuzz/` is an intentionally separate Cargo workspace with its own lockfile and nightly toolchain commands.

## Architecture Boundaries

- Applications should use the `arborui` facade; downstream application tests should add only `arborui-test`. Keep the examples on this public boundary so they continue to detect
  accidental implementation-crate dependencies.
- Keep Crossterm types inside `arborui-backend-crossterm` and Taffy types private to `arborui-layout`. `arborui-ui` must remain terminal- and runtime-independent;
  `arborui-runtime` must not depend on `arborui-widgets`.
- Ephemeral `Element` values may borrow application data, but retained UI state must never retain those references or borrowing callbacks.
- A prepared render frame is committed only after the backend accepts the complete patch. A partial or uncertain write must invalidate physical screen state so the next successful write is a full repaint.
- Internal unit tests belong to their owning crate. `arborui-test` is the public full-application harness, not a repository-wide test bucket.

## Verification

- Install the pinned Node tools with `pnpm install --frozen-lockfile`; this also installs Lefthook. Rust is pinned to the 1.85.0 MSRV.
- Run `just ci` for the normal gate. It runs, in order, formatting/Markdown checks, Clippy with all targets and features, workspace tests with all features, then warning-free docs.
- Use `cargo test -p <package> <test-name> --all-features` for a focused unit test or `cargo test -p <package> --test <integration-test> --all-features` for one integration target.
- `just ci` skips the ignored native terminal lifecycle test. Run `just test-pty` separately; it requires a native PTY or ConPTY and forces one test thread.
- Use `just bench-smoke` to validate benchmark code without recording a Criterion baseline. Use `just package-check` after package metadata or package-content changes.
- Workspace lints forbid unsafe code and deny `unwrap`; public API additions also need documentation because CI promotes warnings to errors.

## Snapshots

- Follow `docs/testing-and-roadmap.md`: snapshot deterministic visual contracts at an explicit terminal size, retain a semantic assertion, and settle the application before capture.
  Character snapshots are the default.
- Run the focused test, then inspect changes with `just snapshot-review` using `cargo-insta` 1.48.0. Commit accepted `.snap` files, never `.snap.new`; CI sets `INSTA_UPDATE=no`.
- Full-width terminal rows intentionally contain trailing spaces. Do not trim them; `.gitattributes` exempts only `*.snap` from Git's trailing-space warning.

## Fuzzing And Releases

- The bounded fuzz command is `cargo +nightly-2026-07-01 fuzz run <target> fuzz/corpus/<target> -- -max_total_time=60`; valid targets are `text_edit_sequences` and
  `render_transactions`, and CI pins `cargo-fuzz` 0.13.2.
- Minimize a fixed fuzz failure with `cargo fuzz tmin`, retain it in the matching corpus, and add a named regression test to the owning crate.
- All publishable crates share one version and internal dependencies use exact matching versions. Release checks require a clean worktree and Cargo 1.90.0; follow
  `docs/releasing.md` rather than invoking `scripts/publish.sh --execute` directly.
