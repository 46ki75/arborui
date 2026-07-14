# Testing And Roadmap

## Testing Principles

- Test semantic structures before encoded terminal bytes.
- Keep a simple reference implementation for optimized algorithms.
- Make Unicode and lifecycle failures reproducible without a physical terminal
  where possible.
- Use PTY tests for behavior that mocks cannot represent.
- Treat terminal restoration as correctness, not cleanup polish.
- Track end-to-end latency and bytes emitted, not only microbenchmark throughput.

## Unit Tests

Each crate owns tests for its contracts.

### `yatui-text`

- Grapheme segmentation
- Combining sequences
- ZWJ emoji
- Regional indicators
- Variation selectors
- CJK and ambiguous-width characters
- Line breaking
- Cursor movement
- Selection replacement
- Undo transaction grouping

### `yatui-render`

- Grapheme insertion and replacement
- Overwriting starts and continuations
- Clipping wide graphemes
- Drawing at the final column
- Style and hyperlink transitions
- Surface z-order
- Hit-map clipping
- Empty frame output
- Full repaint generation

### `yatui-layout`

- Integer rounding
- Percentage dimensions
- Min and max constraints
- Flex growth and shrinkage
- Text measurement integration
- Border and padding geometry
- Resize invalidation

### `yatui-ui`

- Keyed reconciliation
- Duplicate key diagnostics
- Node removal
- Capture, target, and bubble order
- Propagation cancellation
- Mouse capture
- Focus traversal
- Focus restoration after overlay removal
- Invalidation escalation

### `yatui-terminal`

- Desired-state transitions
- Idempotent restoration
- Input fragmentation
- Escape timeout behavior
- Bracketed paste
- Capability responses
- Output outcome handling
- Suspend and resume ordering

## Property And Fuzz Tests

Required properties include:

```text
replay(diff(current, next), current) == next
```

```text
decode(encode(valid terminal operation stream)) preserves semantics
```

```text
apply(edit sequence, simple String model) == apply(edit sequence, TextBuffer)
```

```text
undo(all edits) restores initial text and cursor state
```

Fuzz targets include:

- Arbitrary UTF-8 insertion, deletion, and movement
- Random buffer writes and overlapping graphemes
- Random surface composition
- Fragmented terminal byte streams
- Random retained tree insertion and removal
- Resize storms
- Output failures at every byte boundary

Fuzz regressions become permanent named tests.

## Headless Test API

`yatui-test` exposes an application-level harness:

```rust
let mut app = TestApp::new(MyApp::default(), Size::new(80, 24));

app.key(KeyCode::Enter);
app.mouse(MouseEvent::press(10, 4));
app.resize(Size::new(100, 30));

assert_snapshot!(app.frame());
assert_eq!(app.focused_key(), Some(Key::from("submit")));
```

The harness supports:

- Character snapshots
- Styled-cell snapshots
- Frame patch snapshots
- Normalized event injection
- Resize simulation
- Deterministic clocks
- Command completion
- Waiting until visual idle
- Focus and hit-map inspection
- Simulated backpressure and output failure

Snapshots are a supplement to structural assertions, not a replacement for
them.

## PTY Integration Tests

PTY tests cover behavior that requires an operating system terminal:

- Raw mode activation and restoration
- Alternate-screen entry and exit
- Cursor restoration
- Panic cleanup
- Child-process suspension
- Unix stop and continue
- Resize signals
- Partial writes and closed output
- Main-screen scrolling
- Last-column and autowrap behavior

Target environments:

| Environment | Purpose |
| --- | --- |
| Linux PTY | Primary CI behavior |
| macOS PTY | BSD terminal and signal differences |
| Windows ConPTY | Windows input and output behavior |
| tmux | Multiplexer capability and wrapping behavior |
| xterm-compatible emulator | Baseline VT behavior |
| Slow memory or pipe sink | Backpressure and ordered output |

Not every emulator needs to run on every change. A smaller gating matrix and a
larger scheduled compatibility matrix are acceptable.

## Benchmarks

Benchmark fixtures should represent applications, not only isolated loops.

### Rendering Scenarios

- One changed cell in an 80 by 24 frame
- One changed cell in a 240 by 80 frame
- Full repaint
- Scrolling log region
- Large table
- Overlapping popup
- Unicode-heavy text
- Rapid resize

### Metrics

- Input-to-prepared-frame latency
- Input-to-write-complete latency
- Layout time
- Paint time
- Diff time
- Serialization time
- Bytes emitted
- Allocations per frame
- Peak memory
- Idle CPU usage
- Slow-sink behavior

Microbenchmarks for grapheme segmentation, text measurement, cell comparison,
and ANSI serialization remain useful, but they do not substitute for these
end-to-end scenarios.

## Implementation Milestones

### Milestone 1: Workspace And Core Types

Status: implemented.

Deliver:

- Workspace manifests
- `yatui-core`
- Initial geometry, color, style, and cursor types
- Shared lint, formatting, MSRV, and documentation configuration

Exit criterion: lower-level crates can share stable value types without a
facade or backend dependency.

### Milestone 2: Text And Rendering Core

Status: implemented.

Deliver:

- `yatui-text`
- `yatui-render`
- Width policy
- Grapheme store
- Cell and buffer
- Canvas and surfaces
- Frame diff and prepared-frame transaction
- Headless render tests

Exit criterion: arbitrary frames can be painted, diffed, replayed, and verified
without a real terminal.

### Milestone 3: Terminal Contract And Backend

Status: initial implementation complete. PTY compatibility and Unix job-control
validation remain part of stabilization.

Deliver:

- `yatui-terminal`
- `yatui-backend-crossterm`
- Backend trait
- Normalized events
- Desired terminal state
- RAII session
- Alternate-screen mode
- Cursor management
- Suspend and resume

Exit criterion: a basic fullscreen renderer restores the terminal after normal
exit, application error, panic, resize, and suspension.

### Milestone 4: Layout And UI Tree

Deliver:

- `yatui-layout`
- `yatui-ui`
- Private Taffy adapter
- Ephemeral elements
- Retained identity
- Keys and reconciliation
- Paint, layout, and recomposition invalidation

Exit criterion: borrowed declarative views can be reconciled and rendered
without retaining unsafe references.

### Milestone 5: Interaction

Deliver:

- Event routing
- Hit testing
- Mouse capture
- Focus manager
- Keyboard navigation
- Hover and focus transitions after layout changes

Exit criterion: nested interactive widgets compose without global event
broadcasts or manual focus flags.

### Milestone 6: Runtime And Widgets

Deliver:

- `yatui-runtime`
- `yatui-widgets`
- `Application` and `Command`
- Event proxy
- Scheduler
- Standard widget set
- Controlled text input

Exit criterion: applications remain idle without rendering, integrate with
external async work, and can be fully tested headlessly.

### Milestone 7: Facade And Public Test Harness

Deliver:

- `yatui`
- `yatui-test`
- Prelude and feature structure
- Application examples
- Snapshot and event-injection APIs

Exit criterion: a downstream application can depend only on `yatui` and test
with `yatui-test` without importing backend implementation details.

### Milestone 8: Stabilization

Deliver:

- PTY CI matrix
- Fuzzing corpus
- Benchmark baselines
- API documentation
- Semver and MSRV policy
- Publishing automation
- Compatibility notes

Exit criterion: there are no known terminal-restoration failures, benchmark
regressions are tracked, and crate versions can be published in dependency
order.

### Milestone 9: Optional Ergonomics

Potential deliverables:

- `yatui-macros`
- Declarative view macro
- Derive helpers
- Scoped tasks
- Split-footer screen mode
- Dirty-region painting
- Additional terminal backends

These features are accepted only after the manual API and correctness baseline
are proven in real applications.

## Open Design Questions

The following decisions should be resolved through prototypes and architecture
decision records:

- Whether `Widget` is one trait or separate measure, paint, and event traits
- How event bindings map dynamic event data into typed messages
- Whether frame patches borrow cells or own encoded runs
- Whether the terminal contract splits input, output, and lifecycle traits
- Whether grapheme storage is global to a renderer or scoped to buffers
- How controlled and uncontrolled scroll state coexist
- Which crates support `no_std` or `alloc` without `std`

An open question must not be accidentally stabilized through a convenience
re-export or macro expansion before it is decided.
