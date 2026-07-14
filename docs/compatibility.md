# Compatibility

## Stability

`arborui` is pre-1.0 and its public API is experimental. All workspace crates use
one coordinated version.

- Patch releases within one `0.y` line contain compatible additions and fixes.
- Breaking API changes increment the minor version.
- Deprecations are preferred when a practical migration path exists.
- Release notes identify API, behavior, platform, and terminal compatibility
  changes.

## Minimum Rust Version

The minimum supported Rust version is 1.85.0. CI builds, lints, tests, and
generates documentation with that toolchain. An MSRV increase is a breaking
change before 1.0 and must be called out in release notes.

## Platform Matrix

| Environment | Validation | Status |
| --- | --- | --- |
| Linux PTY | Unit tests plus native PTY lifecycle and exact termios restoration | Tested |
| macOS PTY | Native PTY lifecycle in CI | Tested |
| Windows ConPTY | Native ConPTY process and cleanup-sequence lifecycle in CI | Tested |
| tmux | No automated compatibility run | Experimental |
| Specific terminal emulators | No automated visual-state run | Experimental |

The PTY matrix verifies normal RAII completion and ordered alternate-screen
cleanup. Unix additionally compares termios before and after the session.
ConPTY does not currently assert exact Windows console-mode equivalence. A PTY
is a transport and does not model screen contents, autowrap, scrolling, or
cursor rendering.

## Terminal Limitations

- Crossterm 0.29 is the only backend.
- Raw mode and event reading are process-global. Applications must use one
  active local event reader.
- Unix `SIGTSTP`, `SIGCONT`, `SIGHUP`, and `SIGTERM` lifecycle integration is
  not implemented.
- `panic=unwind` runs RAII cleanup. Abort, `SIGKILL`, power loss, and terminal
  host failure cannot be restored by application code.
- Cursor visibility and shape, title, and autowrap are restored to conservative
  usable defaults, not queried pre-session values.
- Capability detection uses environment hints for color. Enhanced keyboard,
  synchronized updates, hyperlinks, and explicit width behavior may require
  explicit capability configuration.
- Unicode display depends on the selected `WidthPolicy` and the terminal's own
  width implementation.

## Benchmark Policy

Deterministic tests gate patch size and no-op behavior. Criterion timing reports
are produced on scheduled CI and retained as artifacts. Compare base and head
on the same host; timing changes below 10 percent are informational, changes
between 10 and 20 percent require review, and larger reproducible changes are
treated as regression candidates.
