# arborui Design Documents

This directory describes the planned architecture for `arborui`, a Rust-native
terminal user interface library. The design is intentionally split across
small crates with explicit dependency direction.

The documents describe a target architecture, not an implemented or stable
API. Public names and signatures may change while the first applications are
built.

## Documents

| Document | Scope |
| --- | --- |
| [Architecture](architecture.md) | Goals, principles, data flow, ownership, and high-level decisions |
| [Crate Structure](crates.md) | Workspace packages, dependency graph, and package boundaries |
| [Rendering and Text](rendering-and-text.md) | Unicode, cells, surfaces, frame construction, and diffing |
| [UI and Runtime](ui-and-runtime.md) | Application model, retained identity, events, focus, commands, and scheduling |
| [Terminal](terminal.md) | Backends, capabilities, terminal state, output, and lifecycle |
| [Compatibility](compatibility.md) | Tested platforms, terminal limitations, SemVer, and MSRV policy |
| [Releasing](releasing.md) | Package verification, release checklist, and publishing gate |
| [Testing and Roadmap](testing-and-roadmap.md) | Verification strategy, benchmarks, milestones, and release criteria |

## Recommended Reading Order

1. Read [Architecture](architecture.md) for the system model.
2. Read [Crate Structure](crates.md) for ownership and dependency boundaries.
3. Read the subsystem documents relevant to the work being implemented.
4. Use [Testing and Roadmap](testing-and-roadmap.md) to determine milestone
   scope and exit criteria.

## Decision Summary

- Application state is explicit and user-owned.
- The primary application API follows a model-update-view design.
- Views are ephemeral and may safely borrow application data.
- Identity, layout metadata, focus, and interaction state are retained.
- Rendering uses grapheme-aware cell buffers and transactional frame commits.
- Layout is provided by Taffy behind library-owned types.
- UI code is terminal-independent and can run entirely headlessly.
- Terminal modes are desired state reconciled by an RAII session.
- Async integration is runtime-neutral.
- Procedural macros are optional and deferred until the manual API is stable.

## Influences

The design takes distinct ideas from several projects:

| Project | Main influence |
| --- | --- |
| [OpenTUI](https://github.com/anomalyco/opentui) | Grapheme-aware rendering, typed input, hit testing, terminal capabilities, and output transactions |
| [Ratatui](https://github.com/ratatui/ratatui) | Small rendering contracts, cell buffers, and headless testing |
| [Bubble Tea](https://github.com/charmbracelet/bubbletea) | Explicit messages, commands, and declarative terminal state |
| [Textual](https://github.com/Textualize/textual) | Repaint, layout, and recomposition invalidation levels |
| [iocraft](https://github.com/ccbrown/iocraft) | Declarative Rust UI ergonomics and runtime-neutral component futures |
| [Ink](https://github.com/vadimdemedes/ink) | Component semantics, terminal suspension, static output, and PTY testing |
| [Termwiz](https://github.com/wezterm/wezterm/tree/main/termwiz) | Typed terminal changes and capability-aware output |
| [Notcurses](https://github.com/dankamongmen/notcurses) | Independently composited planes and overlays |
