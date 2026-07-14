# Architecture

## Status

This document is a design proposal for the initial `arborui` architecture. It
defines boundaries and invariants that should remain stable even when public
API details change.

## Goals

- Provide a Rust-native declarative API without requiring React-style hooks.
- Render Unicode text correctly at the extended-grapheme level.
- Keep rendering, layout, UI state, and terminal I/O independently testable.
- Support fullscreen, inline, remote, and headless output through backends.
- Make terminal restoration and child-process suspension reliable.
- Avoid selecting an async runtime for applications.
- Let third-party widgets depend on a small UI contract.
- Make correctness measurable before adding incremental rendering complexity.

## Initial Non-Goals

- A CSS selector or cascading style engine
- React-compatible hooks or reconciliation semantics
- Foreign-language bindings
- GPU, image, or sixel rendering
- Multiple layout engines
- Dirty-region painting in the first renderer
- A rich text editor in the first release
- A large standard widget catalog
- A mandatory async runtime

## Design Principles

### Separate Retained State

The library retains several different kinds of state, but they must not be
collapsed into one object graph:

| State | Owner |
| --- | --- |
| Application data | Application model |
| View description | Ephemeral `Element` tree |
| Component identity | UI tree |
| Focus and interaction metadata | UI tree and focus manager |
| Layout nodes and computed geometry | Layout engine |
| Current and next visual cells | Renderer |
| Terminal capabilities and active modes | Terminal session |
| Async work | Runtime command scheduler |

This separation prevents a renderer optimization from dictating the
application state model and prevents terminal resources from leaking into
widgets.

### Make State Transitions Explicit

Applications receive typed messages, update their model, and return commands.
Widgets do not mutate application state through hidden callbacks.

```rust
pub trait Application {
    type Message: Send + 'static;

    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message>;

    fn view(&self) -> Element<'_, Self::Message>;
}
```

The exact signatures may change, but these properties are required:

- `view` can borrow from the model.
- No borrowed value from `view` is retained after synchronous processing.
- Commands own all data needed after `update` returns.
- Application updates occur in one serialized location.

### Keep Backends Out Of Public UI Types

No Crossterm, Termwiz, Taffy, or Tokio type appears in the core UI API. Adapter
crates translate library-owned types at subsystem boundaries.

### Prefer Correct Full Work Before Incorrect Incremental Work

The initial renderer paints a complete next frame and compares it with the
committed frame. Dirty layout and paint regions are later optimizations. They
must preserve the output of full rendering and be justified by benchmarks.

## System Flow

```text
terminal event or completed command
                |
                v
       application update
                |
                v
      ephemeral view tree
                |
                v
 identity reconciliation and event metadata
                |
                v
             layout
                |
                v
        paint and composition
                |
                v
      grapheme-aware next buffer
                |
                v
        frame patch generation
                |
                v
         terminal backend
                |
                v
       commit or invalidate frame
```

An input event does not necessarily produce a frame. If `update` does not
invalidate the view, the runtime may process further events without layout or
painting.

## Dependency Direction

```text
text              -> core
layout            -> core
render            -> core, text
terminal          -> core, render
backend-crossterm -> terminal
ui                -> core, text, layout, render
widgets           -> core, text, layout, ui
runtime           -> core, ui, render, terminal
test              -> core, ui, render, terminal, runtime
arborui facade      -> selected public crates
```

Dependencies always point toward lower-level data and behavior. A lower layer
must not call upward into application or widget code.

## Ownership And Lifetimes

### Ephemeral View

`Element<'a, Message>` describes the current UI and may contain borrowed text,
styles, and collections. The runtime processes it synchronously for
reconciliation, event dispatch, layout, and painting.

The retained tree may store:

- Stable keys and node kinds
- Parent and child identity
- Last computed geometry
- Focusability and focus scope metadata
- Hover, capture, and selection metadata
- Invalidation state
- Widget-owned state explicitly designed for retention

The retained tree must not store:

- References into the application model
- References into a temporary `Element` tree
- Closures borrowing from `view`
- Layout backend node types in public structures

Event bindings are evaluated against the current ephemeral view. The runtime
rebuilds a view before dispatch when necessary, uses retained identity to find
the target, and moves any resulting message into the application queue. This
allows convenient message values without unsafe lifetime extension.

### Renderer Transaction

The renderer owns the committed visual frame. Preparing a patch does not
modify that frame. The frame is committed only after the backend reports that
the complete patch was accepted.

```text
prepare next frame
      |
      v
write complete patch? -- no --> invalidate committed terminal state
      |
     yes
      |
      v
commit next frame
```

If output may have been partially applied, the next successful write must be a
full repaint.

## Invalidation

The UI tracks the least expensive operation required after a change:

```rust
pub enum Invalidation {
    None,
    Paint,
    Layout,
    Recompose,
}
```

| Level | Required work |
| --- | --- |
| `None` | No visual work |
| `Paint` | Repaint with existing geometry |
| `Layout` | Recalculate affected geometry, then paint |
| `Recompose` | Rebuild and reconcile the affected view structure |

Invalidation can initially trigger broad work while retaining this API. Later
versions may restrict work to affected subtrees and regions.

Terminal-state invalidation is separate. Cursor shape, mouse mode, title, and
screen mode may change even when visual cells do not.

## Error Strategy

- Recoverable parsing and rendering failures return structured errors.
- Malformed terminal input must not panic.
- Allocation failures are propagated where the platform permits recovery.
- A failed terminal write marks physical screen state as unknown when partial
  output is possible.
- Terminal restoration is attempted after application errors and panic
  unwinding.
- Internal invariant violations may panic in debug builds but should be fuzzed
  and prevented at public boundaries.

## Extension Points

The initial stable extension points are:

- Custom widgets through `arborui-ui`
- Custom terminal backends through `arborui-terminal`
- Third-party widget crates without depending on the runtime
- Application-specific command producers through an event proxy
- Headless and remote transports through backend implementations

Layout engines, reconcilers, and cell representations are not initial public
extension points. Their invariants are too central to make replaceable before
the core design is proven.
