# Crate Structure

## Workspace

```text
yatui/
  Cargo.toml
  crates/
    yatui/
    yatui-core/
    yatui-text/
    yatui-render/
    yatui-layout/
    yatui-terminal/
    yatui-backend-crossterm/
    yatui-ui/
    yatui-runtime/
    yatui-widgets/
    yatui-test/
    yatui-macros/
```

Crate boundaries represent ownership and dependency boundaries, not just file
organization. New crates should be added only when they are independently
useful, isolate an optional dependency, or enforce an important architectural
boundary.

## Dependency Graph

`A -> B` means that crate A directly depends on crate B.

```text
yatui-text              -> yatui-core
yatui-render            -> yatui-core, yatui-text
yatui-layout            -> yatui-core
yatui-terminal          -> yatui-core, yatui-render
yatui-backend-crossterm -> yatui-terminal
yatui-ui                -> yatui-core, yatui-text, yatui-render, yatui-layout
yatui-widgets           -> yatui-core, yatui-text, yatui-layout, yatui-ui
yatui-runtime           -> yatui-core, yatui-ui, yatui-render, yatui-terminal
yatui-test              -> yatui-core, yatui-ui, yatui-render,
                           yatui-terminal, yatui-runtime
yatui-macros            -> proc-macro implementation dependencies only
yatui                    -> selected public crates
```

Cycles are not permitted. Re-exporting a type does not justify reversing the
dependency direction.

## Crate Responsibilities

### `yatui-core`

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

### `yatui-text`

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

### `yatui-render`

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

### `yatui-layout`

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

### `yatui-terminal`

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

### `yatui-backend-crossterm`

Implements `TerminalBackend` with Crossterm. Crossterm types remain inside this
crate. It translates events, serializes frame patches, and manages platform
terminal modes.

Additional backends should be separate crates, for example:

```text
yatui-backend-termina
yatui-backend-termwiz
yatui-backend-ssh
```

### `yatui-ui`

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

### `yatui-runtime`

Owns the `Application` trait, commands, scheduler, event loop, event proxy,
terminal orchestration, and shutdown behavior.

Expected modules:

```text
app
command
event_loop
proxy
scheduler
task
```

The runtime depends on `yatui-ui`; `yatui-ui` never depends on the runtime.
The runtime does not depend on the standard widget crate.

### `yatui-widgets`

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
represented by explicit state types from `yatui-text` or the application.

Third-party widget crates should normally depend on `yatui-ui` and whichever
lower-level crates they directly use, not on the `yatui` facade.

### `yatui-test`

Provides downstream application and widget test utilities:

```text
backend
clock
events
snapshot
test_app
```

Internal unit tests remain in their owning crates. `yatui-test` is a public
headless harness, not a central location for all repository tests.

### `yatui-macros`

Contains optional procedural macros after the manual API is stable. Macro
expansions must use public APIs and must not rely on private runtime internals.
No core crate depends on this crate.

### `yatui`

The facade crate used by most applications. It contains minimal implementation
code and re-exports selected APIs.

Example shape:

```rust
pub use yatui_runtime::{AppRunner, Application, Command};
pub use yatui_ui::{Element, Key};
pub use yatui_widgets as widgets;

#[cfg(feature = "crossterm")]
pub use yatui_backend_crossterm::CrosstermBackend;
```

## Features

The facade may provide these features:

```toml
[features]
default = ["crossterm"]
crossterm = ["dep:yatui-backend-crossterm"]
macros = ["dep:yatui-macros"]
test-utils = ["dep:yatui-test"]
serde = ["yatui-core/serde", "yatui-text/serde"]
```

The `widgets`, `macros`, `test-utils`, and `serde` features are introduced only
when their corresponding crates or implementations exist.

Lower-level crates should have empty or minimal default features. Backends are
separate crates rather than a growing collection of features in
`yatui-terminal`.

## Versioning And Publishing

During the pre-1.0 period, all workspace crates use one coordinated version.
Internal dependencies use both a path and an exact package version:

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"
authors = ["Ikuma Yamashita <me@ikuma.cloud>"]
repository = "https://github.com/46ki75/yatui"
categories = ["command-line-interface"]
keywords = ["terminal", "tui", "cli", "ui"]

[workspace.dependencies]
yatui-core = { path = "crates/yatui-core", version = "=0.1.0" }
yatui-text = { path = "crates/yatui-text", version = "=0.1.0" }
```

Publish in topological dependency order:

1. `yatui-core`
2. `yatui-text` and `yatui-layout`
3. `yatui-render`
4. `yatui-terminal`
5. Terminal backends
6. `yatui-ui`
7. `yatui-runtime` and `yatui-widgets`
8. `yatui-test` and `yatui-macros`
9. `yatui`

## Boundary Review Checklist

Before adding a dependency between workspace crates, verify:

- The source crate directly uses the dependency's public API.
- The dependency points toward a lower-level concern.
- The dependency does not introduce terminal I/O into headless UI code.
- The dependency does not expose third-party types through a stable API.
- A feature flag would not be a clearer solution for truly optional behavior.
- The change does not create a second route for application state mutation.
