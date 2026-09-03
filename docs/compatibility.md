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
is a transport and does not model screen contents, autowrap, scrolling, cursor
rendering, or terminal-rendered image pixels. PTY tests can validate Kitty
command bytes and cleanup ordering, but not the resulting pixels.

## Terminal Limitations

- Crossterm 0.29 is the only backend.
- The high-level runtime is supported for alternate-screen fullscreen use.
  Main-screen lifecycle transitions exist, but inline regions and native
  scrollback rendering do not yet have defined update or recovery semantics.
- Raw mode and event reading are process-global. Applications must use one
  active local event reader.
- Unix `SIGTSTP`, `SIGCONT`, `SIGHUP`, and `SIGTERM` lifecycle integration is
  not implemented.
- `panic=unwind` runs RAII cleanup. Abort, `SIGKILL`, power loss, and terminal
  host failure cannot be restored by application code.
- Cursor visibility and shape, title, and autowrap are restored to conservative
  usable defaults, not queried pre-session values.
- Capability detection uses environment hints for color. Enhanced keyboard,
  synchronized updates, and explicit width behavior may require explicit
  capability configuration. Capabilities are static for a session and are not
  renegotiated after resume.
- Kitty graphics use `KittyGraphicsMode::{Auto, Disabled, Enabled}`. `Auto` is
  environment-based, performs no active probing, and is disabled under SSH,
  mosh, tmux, GNU Screen, and Zellij.
- The Crossterm backend supports Kitty direct 32-bit RGBA transfer only on the
  alternate screen. It has no PNG passthrough, filesystem or shared-memory
  transport, animation, tmux placeholders, or main-screen images. Sources above
  10,000 pixels on either axis remain fallback-only for Kitty compatibility.
- The optional `image-decoding` feature supports content-detected BMP, GIF, ICO,
  JPEG, PNG, PNM, QOI, TGA, TIFF, and WebP input. Animated GIF and WebP input
  uses the first frame. SVG and other vector formats must be rasterized before
  constructing the backend-neutral RGBA image supplied to ArborUI.
- The Crossterm backend does not currently resolve or emit OSC 8 hyperlinks and
  reports hyperlink support as disabled even when configured otherwise.
- Unicode display depends on the selected `WidthPolicy` and the terminal's own
  width implementation.

## Benchmark Policy

Deterministic tests gate patch size and no-op behavior. Criterion timing reports
are produced on scheduled CI and retained as artifacts. Compare base and head
on the same host; timing changes below 10 percent are informational, changes
between 10 and 20 percent require review, and larger reproducible changes are
treated as regression candidates.
