# yatui

`yatui` is an experimental Rust-native terminal user interface library. It is
being designed as a collection of focused crates for text processing,
rendering, layout, terminal integration, retained UI identity, application
runtime behavior, and widgets.

The project is in its initial implementation phase. The current code provides
shared core types, Unicode grapheme measurement, cell buffers, clipped drawing,
surface composition, transactional frame diffing, normalized terminal events,
RAII terminal sessions, a Crossterm backend, private-Taffy flex layout,
borrowed declarative elements, retained identity, keyed reconciliation, and a
headless UI-to-frame pipeline. The remaining subsystems are developed
incrementally.

## Features

The `crossterm` feature is enabled by default and provides the Crossterm
terminal backend. Disable default features when integrating another backend:

```toml
[dependencies]
yatui = { version = "0.1.0", default-features = false }
```

## Design

Start with the [design document index](docs/README.md). The design covers:

- [Architecture and ownership](docs/architecture.md)
- [Workspace crate boundaries](docs/crates.md)
- [Rendering and Unicode text](docs/rendering-and-text.md)
- [UI and runtime behavior](docs/ui-and-runtime.md)
- [Terminal lifecycle](docs/terminal.md)
- [Testing and implementation roadmap](docs/testing-and-roadmap.md)

## Development

Install the repository tools and run the complete local check:

```console
pnpm install
just ci
```

The workspace MSRV is Rust 1.85.0 and is pinned in `rust-toolchain.toml`.

## License

Licensed under either the [Apache License, Version 2.0](LICENSE-APACHE) or the
[MIT license](LICENSE-MIT), at your option.
