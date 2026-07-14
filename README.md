# arborui

`arborui` is an experimental Rust-native terminal user interface library. It is
being designed as a collection of focused crates for text processing,
rendering, layout, terminal integration, retained UI identity, application
runtime behavior, and widgets.

The project is in its initial implementation phase. The current code provides
shared core types, Unicode grapheme measurement, cell buffers, clipped drawing,
surface composition, transactional frame diffing, normalized terminal events,
RAII terminal sessions, a Crossterm backend, private-Taffy flex layout,
borrowed declarative elements, retained identity, keyed reconciliation, and a
headless UI-to-frame pipeline. Capture-target-bubble event routing,
transactional hit maps, pointer capture, hover tracking, focus scopes, keyboard
traversal, and focused cursor synchronization are also implemented. The runtime
adds serialized model updates, opaque commands, external event proxies,
runtime-neutral futures, idle-aware rendering, and transactional terminal
orchestration. Standard controlled widgets include flex composition, blocks,
buttons, stacks, lists, scrolling, and grapheme-aware text input. The public
`arborui-test` harness drives complete applications with deterministic time,
headless input, frame snapshots, and simulated output failures.

## Features

The `crossterm` feature is enabled by default and provides the Crossterm
terminal backend. Disable default features when integrating another backend:

```toml
[dependencies]
arborui = { version = "0.1.0", default-features = false }
```

Application code can import common model-update-view and widget APIs from the
prelude:

```rust
use arborui::prelude::*;
```

Downstream tests use the backend-independent harness as a separate development
dependency:

```toml
[dev-dependencies]
arborui-test = "0.1.0"
```

```rust
use arborui_test::{KeyCode, Size, TestApp};

let mut app = TestApp::new(MyApp::default(), Size::new(80, 24));
app.key(KeyCode::Enter);
assert!(app.frame().characters().contains("complete"));
```

See `examples/counter` for a complete facade-only application and downstream
test.

## Design

Start with the
[design document index](https://github.com/46ki75/arborui/blob/main/docs/README.md).
The design covers:

- [Architecture and ownership](https://github.com/46ki75/arborui/blob/main/docs/architecture.md)
- [Workspace crate boundaries](https://github.com/46ki75/arborui/blob/main/docs/crates.md)
- [Rendering and Unicode text](https://github.com/46ki75/arborui/blob/main/docs/rendering-and-text.md)
- [UI and runtime behavior](https://github.com/46ki75/arborui/blob/main/docs/ui-and-runtime.md)
- [Terminal lifecycle](https://github.com/46ki75/arborui/blob/main/docs/terminal.md)
- [Compatibility](https://github.com/46ki75/arborui/blob/main/docs/compatibility.md)
- [Testing and implementation roadmap](https://github.com/46ki75/arborui/blob/main/docs/testing-and-roadmap.md)

## Development

Install the repository tools and run the complete local check:

```console
pnpm install
just ci
```

The workspace MSRV is Rust 1.85.0 and is pinned in `rust-toolchain.toml`.

## License

Licensed under either the
[Apache License, Version 2.0](https://github.com/46ki75/arborui/blob/main/LICENSE-APACHE)
or the [MIT license](https://github.com/46ki75/arborui/blob/main/LICENSE-MIT), at
your option.
