# Crate Structure

## Workspace

```text
arborui/
  Cargo.toml
  crates/
    arborui/
    arborui-core/
    arborui-text/
    arborui-render/
    arborui-layout/
    arborui-terminal/
    arborui-backend-crossterm/
    arborui-ui/
    arborui-runtime/
    arborui-widgets/
    arborui-test/
```

Crate boundaries represent ownership and dependency boundaries, not just file
organization. New crates should be added only when they are independently
useful, isolate an optional dependency, or enforce an important architectural
boundary.

## Dependency Graph

`A -> B` means that crate A directly depends on crate B.

```text
arborui-render            -> arborui-core, arborui-text
arborui-layout            -> arborui-core
arborui-terminal          -> arborui-core, arborui-render, arborui-text
arborui-backend-crossterm -> arborui-core, arborui-render, arborui-terminal
arborui-ui                -> arborui-core, arborui-text, arborui-render, arborui-layout
arborui-widgets           -> arborui-core, arborui-text, arborui-layout, arborui-ui
arborui-runtime           -> arborui-core, arborui-ui, arborui-render, arborui-terminal
arborui-test              -> arborui-core, arborui-text, arborui-ui, arborui-render,
                           arborui-terminal, arborui-runtime
arborui                    -> selected public crates
```

Cycles are not permitted. Re-exporting a type does not justify reversing the
dependency direction.

## Crate Responsibilities

### `arborui-core`

Contains small, stable value types shared by other crates.

Expected modules:

```text
color
cursor
geometry
style
```

Expected public types include `Point`, `Size`, `Rect`, `Insets`, `Color`,
`Style`, and `CursorState`.

This crate does not contain cells, widgets, layout nodes, terminal I/O, or an
application runtime. It should have very few dependencies. `no_std` support is
desirable if it remains nearly free, but it is not a first-release gate.

### `arborui-text`

Owns Unicode segmentation, display width, wrapping, measurement, styled text,
and editing data structures.

Expected modules:

```text
edit
grapheme
line_break
measure
rope
selection
styled
width
```

It does not know about cells, escape sequences, layout trees, or widgets.

### `arborui-render`

Owns visual cells, grapheme storage, buffers, surfaces, clipping, composition,
hit maps, frame patches, and prepared-frame transactions.

Expected modules:

```text
buffer
canvas
cell
compositor
diff
frame
grapheme_store
patch
surface
```

It accepts geometry and style but does not know about application messages,
focus traversal, widgets, or terminal backend libraries.

### `arborui-layout`

Owns library-facing layout types and the private Taffy adapter.

Expected modules:

```text
dimension
engine
measure
style
tree
```

Taffy node IDs, styles, and errors must not appear in the public API.

### `arborui-terminal`

Defines terminal events, capabilities, desired terminal state, backend
contracts, output outcomes, and session lifecycle.

Expected modules:

```text
backend
capabilities
event
operations
session
state
transport
```

It may re-export the render patch type used by `TerminalBackend`, but it does
not own UI events, widgets, or the application loop.

### `arborui-backend-crossterm`

Implements `TerminalBackend` with Crossterm. Crossterm types remain inside this
crate. It translates events, serializes frame patches, and manages platform
terminal modes.

Additional backends should be separate crates, for example:

```text
arborui-backend-termina
arborui-backend-termwiz
arborui-backend-ssh
```

### `arborui-ui`

Owns ephemeral elements, retained identity, reconciliation, widget contracts,
event routing, focus, hit testing, and invalidation.

Expected modules:

```text
element
event
focus
hit_test
invalidation
key
node
reconcile
tree
widget
```

This crate must work without a real terminal or application event loop.

### `arborui-runtime`

Owns the `Application` trait, commands, scheduler, event loop, event proxy,
terminal orchestration, and shutdown behavior.

Expected modules:

```text
app
clock
command
event_loop
proxy
scheduler
task
```

The runtime depends on `arborui-ui`; `arborui-ui` never depends on the runtime.
The runtime does not depend on the standard widget crate.

### `arborui-widgets`

Contains the standard widget catalog. The first set is deliberately small:

```text
block
button
column
input
list
row
scroll
spacer
stack
text
```

Widgets are controlled by default. Complex state such as editable text is
represented by explicit state types from `arborui-text` or the application.

Third-party widget crates should normally depend on `arborui-ui` and whichever
lower-level crates they directly use, not on the `arborui` facade.

### `arborui-test`

Provides downstream application and widget test utilities:

```text
app
backend
clock
frame
```

Internal unit tests remain in their owning crates. `arborui-test` is a public
headless harness, not a central location for all repository tests.

### `arborui`

The facade crate used by most applications. It contains minimal implementation
code and re-exports selected APIs.

Example shape:

```rust
pub use arborui_runtime::{AppRunner, Application, Command};
pub use arborui_ui::{Element, Key};
pub use arborui_widgets as widgets;

#[cfg(feature = "crossterm")]
pub use arborui_backend_crossterm::CrosstermBackend;
```

## Features

The facade initially provides one backend-selection feature:

```toml
[features]
default = ["crossterm"]
crossterm = ["dep:arborui-backend-crossterm"]
```

`arborui-test` remains a separate development dependency so tests exercise an
explicit public boundary without adding test utilities to application builds.
Potential `macros` and `serde` features are introduced only when their
implementations exist.

Lower-level crates should have empty or minimal default features. Backends are
separate crates rather than a growing collection of features in
`arborui-terminal`.

## Versioning And Publishing

During the pre-1.0 period, all workspace crates use one coordinated version.
Internal dependencies use both a path and an exact package version:

```toml
[workspace]
resolver = "3"
members = ["crates/*", "examples/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"
authors = ["Ikuma Yamashita <me@ikuma.cloud>"]
repository = "https://github.com/46ki75/arborui"
categories = ["command-line-interface"]
keywords = ["terminal", "tui", "cli", "ui"]

[workspace.dependencies]
arborui-core = { path = "crates/arborui-core", version = "=0.1.0" }
arborui-text = { path = "crates/arborui-text", version = "=0.1.0" }
```

Patch releases within a `0.y` line contain compatible additions and fixes.
Breaking API changes or an MSRV increase require the next `0.(y+1).0` release.
All releases use one `vX.Y.Z` tag and include compatibility notes. The MSRV is
Rust 1.85.0 and is tested directly; it may increase only in a breaking release.

Publish in topological dependency order:

1. `arborui-core` and `arborui-text`
2. `arborui-layout` and `arborui-render`
3. `arborui-terminal` and `arborui-ui`
4. `arborui-backend-crossterm`, `arborui-runtime`, and `arborui-widgets`
5. `arborui-test`
6. `arborui`

## Boundary Review Checklist

Before adding a dependency between workspace crates, verify:

- The source crate directly uses the dependency's public API.
- The dependency points toward a lower-level concern.
- The dependency does not introduce terminal I/O into headless UI code.
- The dependency does not expose third-party types through a stable API.
- A feature flag would not be a clearer solution for truly optional behavior.
- The change does not create a second route for application state mutation.
