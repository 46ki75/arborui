# Ratatui Research Report

## Evidence Header

```text
Research date: 2026-07-16
Project version: ratatui 0.30.2
Project revision: e665c36cb14752a61cd777fbd06dbef8474f2add
Repository: https://github.com/ratatui/ratatui
Documentation version: docs.rs 0.30.2; website revision 84e68a03
Primary platform examined: Source inspection on Linux; no physical terminal reproduction
Report depth: Deep dive pilot
```

The latest stable release at the start of this research was
[`ratatui-v0.30.2`](https://github.com/ratatui/ratatui/releases/tag/ratatui-v0.30.2),
released on 2026-06-19. Source conclusions refer to that tag unless a finding
explicitly names a later issue or website revision.

## Executive Assessment

Ratatui is best understood as a mature, immediate-mode rendering, layout, and
widget library rather than a complete application framework. An application
retains control of its model, input, event loop, asynchronous work, redraw
policy, focus conventions, and component architecture. Each draw call describes
the complete desired viewport; Ratatui renders into a cell buffer, compares it
with the preceding buffer, and sends changed cells to a backend.

That boundary is Ratatui's central strength. It is easy to add to an existing
Rust program, does not select a runtime or state architecture, supports several
terminal backends, and offers a substantial widget and ecosystem surface. Its
public `Buffer` and `TestBackend` also provide a strong foundation for
deterministic logical-screen tests.

The same boundary explains most of the friction relevant to ArborUI. Ratatui
does not provide retained component identity, built-in focus or event routing,
serialized effects, deterministic application settlement, or a full-application
test driver. These are application responsibilities rather than missing parts
of Ratatui's stated rendering contract.

The most consequential technical difference is output recovery. Ratatui
documents backend errors as potentially leaving buffers, output, or cursor state
partially advanced and recommends treating such errors as fatal to the terminal
session. Its normal draw path swaps logical buffers before the backend's final
flush succeeds. ArborUI's prepared-frame transaction and explicit
physical-state invalidation therefore represent a real semantic difference, not
just an alternative API style. A custom stateful backend could potentially add
recovery beneath `Terminal`, but Ratatui's standard contract does not provide or
test that policy.

Ratatui is a strong choice when an application wants a flexible rendering
substrate and is willing to build or select its own application architecture.
It is not a drop-in substitute for ArborUI's proposed runtime, retained
interaction model, or recoverable terminal contract. Conversely, ArborUI should
not claim superiority merely for providing more machinery: Ratatui's smaller
contract may reduce integration and conceptual cost for applications that do
not need framework policy. That application-level cost was not measured here.

## Project Snapshot

Ratatui is a Rust TUI library descended from `tui-rs`. The project was forked in
February 2023 with the original maintainer's blessing after maintenance activity
had slowed while use and contributions continued. The
[`tui-rs` succession discussion](https://github.com/fdehau/tui-rs/issues/654)
describes governance continuity, rather than architectural rejection, as the
immediate reason for the fork. Ratatui has since continued active releases,
documentation, examples, and ecosystem development.

Version 0.30 reorganized the project into a modular workspace. The
[`ratatui` facade](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ARCHITECTURE.md)
targets applications, `ratatui-core` contains foundational buffer, text, layout,
style, terminal, and widget contracts, `ratatui-widgets` contains standard
widgets, and separate crates implement Crossterm, Termion, Termwiz, and Termina
backends. Widget libraries can depend on the smaller core contract rather than
the application facade.

The primary comparison category is rendering and widgets. Ratatui also provides
terminal initialization and viewport management, but intentionally leaves most
application-framework concerns outside its contract.

## Core Proposition

Ratatui offers attractive terminal rendering without taking control of the
application. Its official FAQ explicitly contrasts a library, where application
code keeps control flow, with a framework that supplies and owns more of the
program structure. A Ratatui application chooses when to draw, how to receive
events, where state lives, whether to use threads or an async runtime, and how
components communicate.

The rendering model is immediate and buffered. Widgets are configurable draw
commands, not retained UI objects. On every draw pass the application renders
everything that should be visible into a fresh logical buffer. Ratatui then
diffs that buffer against the previous frame and sends only changed cells to the
backend. This gives applications a simple state model while avoiding full
terminal output on every frame.

This model is especially effective for:

- Existing Rust programs that want to add a visual interface
- Applications with a custom reducer, event loop, runtime, or channel topology
- Dashboards and tools whose complete visible state is inexpensive to describe
- Custom widgets that can render directly into a rectangular cell buffer
- Projects that value backend choice and a large Rust widget ecosystem

It is less complete for applications that expect the UI library to own durable
component identity, interaction routing, task lifetimes, or application-level
testing.

## Architecture

### Application And State Model

Ratatui imposes no application model. The official
[event-handling guide](https://ratatui.rs/concepts/event-handling/) states that
Ratatui does not catch input itself; applications use the selected backend
library and choose centralized matching, message passing, or another structure.
The project documents Elm, component, and Flux patterns as options rather than
enforcing one.

`Widget::render` consumes a widget value and writes into a supplied `Buffer`.
Applications commonly implement `Widget` for a reference when they want to
reuse a value. `StatefulWidget` adds an associated state value passed by mutable
reference during rendering. That state remains owned and located by the
application. Ratatui does not retain a widget tree, reconcile keys, or assign
stable node identities.

This avoids retained-mode lifetime and ownership problems. It also means focus,
selection, scroll offsets, modal stacks, mouse targets, and component task
lifecycle require application conventions or third-party frameworks. The
experimental `WidgetRef` and `StatefulWidgetRef` traits support more dynamic
reuse, but remain behind the `unstable-widget-ref` feature in 0.30.2.

### Layout And Widgets

Layout operates on terminal rectangles and constraints. Rendering code normally
splits `Frame::area()` and passes the resulting rectangles to widgets. Later
renders overwrite earlier cell content, which provides a direct way to compose
popups and overlays when the application orders and clears them correctly.

The standard widget catalog is a major practical strength. It includes blocks,
paragraphs, lists, tables, tabs, calendars, charts, canvases, gauges, sparklines,
scrollbars, and related state types. Ratatui does not include a complete input or
form framework; editing models, focus behavior, validation, and event mapping
remain outside visual widget rendering.

Third-party widget authors can implement the small `Widget` or `StatefulWidget`
contract against `ratatui-core`. The modular split is designed to make that
dependency more stable and lighter than the facade. Core and widget crates also
support allocation-backed `no_std` configurations, while concrete terminal
integration remains separate.

### Rendering Pipeline

`Terminal<B>` owns a backend, a viewport, two buffers, cursor bookkeeping, and
a frame count. A normal draw pass performs these steps:

1. Check and apply an automatic resize where the viewport permits it.
2. Give the render callback a `Frame` backed by the current buffer.
3. Diff the current and previous buffers and call `Backend::draw` with changed cells.
4. Apply requested cursor visibility and position.
5. Reset and swap the logical buffers.
6. Call `Backend::flush`.

The sequence is documented in the
[`Terminal` contract](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/terminal.rs#L112-L157)
and implemented by
[`apply_buffer_with_cursor`](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/terminal/render.rs#L288-L319).

The render callback must describe the complete desired frame. After a successful
pass, the next current buffer has been reset; anything omitted from the next
render is treated as empty. Output is incremental, but widget construction,
painting, and the buffer comparison are not retained or dirty-subtree
operations. Layout results are memoized by the default LRU cache, but the cache
does not represent subtree invalidation.

`Buffer` is a rectangle and a vector of styled `Cell` values. A cell stores a
string symbol, foreground, background and underline colors, modifiers, and diff
options. Text rendering segments graphemes and measures terminal width. Wide
symbols occupy a leading cell and cause covered cells to be reset or skipped by
the relevant rendering and diff paths. Version 0.30.2 fixed a regression where
style could remain behind after replacing a wide symbol with narrow content,
showing both active maintenance and the subtlety of this representation.

### Backends And Terminal Lifecycle

The public `Backend` trait covers drawing changed cells, clearing, size and
cursor queries, cursor visibility, flushing, and optional line or scrolling
operations. Ratatui 0.30.2 ships facade features for Crossterm, Termion, Termwiz,
and Termina. Applications can implement another backend or use core buffers
without a terminal backend.

`Terminal` itself focuses on rendering and does not implicitly put the process
in raw mode or enter an alternate screen. The facade adds convenience helpers:

- `ratatui::run` and `ratatui::init` enable raw mode and enter the alternate screen.
- `init_with_options` enables raw mode without entering the alternate screen.
- Initialization helpers install a panic hook that attempts restoration before delegating.
- `restore` disables raw mode and leaves the alternate screen.

These helpers significantly improve the historical setup burden. They do not
own every terminal mode that application code may enable, such as mouse capture,
bracketed paste, focus reporting, or enhanced keyboard protocols. Suspension,
child-process handoff, Unix job control, and coordinated capability negotiation
also remain application or backend concerns.

### Viewports And Scrollback

Ratatui 0.30.2 has three explicit viewport modes:

| Viewport | Contract |
| --- | --- |
| `Fullscreen` | Own the complete backend size, begin at `(0, 0)`, and automatically resize during a draw |
| `Inline(height)` | Use full terminal width at the current cursor row, reserve rows, and preserve ordinary output above the live UI |
| `Fixed(Rect)` | Render into an application-supplied absolute region and require explicit resize management |

Inline mode and `insert_before` are important improvements over the original
`tui-rs` limitation to full-screen applications. With the `scrolling-regions`
feature, Ratatui can insert output above a live viewport without clearing and
redrawing it. The portable fallback may perform more repainting. Inline
construction also needs to discover and track cursor position, which creates
coordination requirements with application input readers.

Ratatui's explicit viewport distinction is a useful precedent for ArborUI. It
does not make alternate-screen, inline, fixed-region, and native-scrollback
behavior interchangeable; each mode has its own sizing and cursor rules.

## Core Strengths

### Small And Flexible Application Boundary

Ratatui can fit into synchronous loops, threads, Tokio applications, external
reducers, or custom channel systems because it does not select any of them. The
application calls `draw`; Ratatui does not invert control beyond the render
callback. This is valuable for tools where the TUI is one subsystem of a larger
program rather than the program's architectural center.

The cost of this flexibility is visible application code, but that code is not
necessarily accidental boilerplate. It can be the application's deliberate
scheduling and ownership policy. ArborUI should therefore compare its runtime
against actual user needs, not count Ratatui's lack of a runtime as an
unqualified defect.

### Mature Rendering And Viewport Capabilities

The complete-frame mental model is straightforward, while double-buffer diffing
reduces terminal output. Direct buffer access gives custom widgets a practical
escape hatch. Fullscreen, inline, and fixed viewports cover substantially more
integration styles than a conventional alternate-screen-only renderer.

Ratatui also exposes lower-level manual frame, buffer, flush, and swap methods
for specialized integrations. These escape hatches are clearly documented as
requiring the caller to manage synchronization.

### Widget And Ecosystem Breadth

The built-in visual catalog is much broader than ArborUI's current widget set,
and the small widget trait has enabled many external widget crates and examples.
The website provides tutorials, application patterns, templates, recipes, and a
showcase. These provide existing building blocks even though application
interaction remains the user's responsibility; their effect on total adoption
cost was not measured here.

The lesson is that architectural guarantees do not replace common widgets,
examples, and integration guidance. A framework with stronger semantics can
still lose on practical application cost if users must build every control.

### Modular Extension Surface

The 0.30 workspace split gives application authors a facade while allowing
widget authors to depend on a smaller core. Backend-specific types are isolated
in backend crates. This resembles ArborUI's intended facade and implementation
boundaries, while Ratatui demonstrates the ecosystem benefit of making the
third-party widget dependency intentionally small.

### Strong Logical Rendering Tests

Widgets render into the same public `Buffer` in production and tests. Direct
buffer tests therefore exercise real layout, clipping, text, styles, and widget
painting without a terminal. `Terminal<TestBackend>` extends that path through
frame drawing, diffing, viewport management, cursor handling, and logical
scrollback. This is a robust and comprehensible testing boundary.

### Maintenance And Governance Continuity

The transition from `tui-rs` demonstrates that governance is part of library
reliability. Ratatui preserved the familiar design, moved stewardship into an
active community, imported prior work, and continued evolving the architecture.
The fork is evidence that a project can need replacement for organizational
reasons even when users value its technical model.

## Limitations And Frustrations

### Application Architecture Must Be Assembled By Users

```text
Classification: Tradeoff
Requirement: A consistent application runtime with retained interaction identity
Library assumption: Applications should retain control and choose their own architecture
Observable friction: Focus, routing, timers, effects, redraw policy, and component lifecycle need application conventions
Root cause: Ratatui's public contract ends at rendering and terminal presentation
Workaround: Build an application layer or use a framework/template above Ratatui
Cost: Application-defined and not measured in this research
Current status: Intentional in 0.30.2
Evidence status: Verified
Confidence: High for the API boundary
```

Ratatui has no retained node tree, keyed reconciliation, focus manager,
capture-target-bubble routing, command scheduler, or application message queue.
This is consistent with its stated identity as a library. Simple applications
benefit from the freedom and low conceptual overhead. Larger teams may recreate
similar architectures independently. Reusable interactive components must agree
on conventions that Ratatui does not define, but this report did not compare a
representative set of applications closely enough to quantify duplicated work.

The original `tui-rs` maintainer identified complex UI abstractions, scrolling,
mouse support, and advanced layouts as difficult areas for the original
immediate-mode architecture. Current Ratatui has addressed many concrete
features, but it has not changed the fundamental ownership boundary. The modern
conclusion is narrower: Ratatui is capable of complex applications, but it does
not provide a shared retained interaction model for them.

### Backend Errors Are Not Recoverable Transactions

```text
Classification: Limitation relative to ArborUI's recovery requirement
Requirement: Commit logical frame state only after complete output acceptance
Library assumption: A backend failure normally ends the current terminal session
Observable friction: An error may leave output, buffers, or cursor state partially advanced
Root cause: Backend operations expose ordinary errors rather than applied/deferred/unknown outcomes
Workaround: Restore and terminate, replace Terminal, or implement a stateful backend that stages and can fully repaint its own shadow state
Cost: In-session recovery is not supplied or tested by Ratatui's standard terminal and backends
Current status: Documented behavior in 0.30.2
Evidence status: Verified
Confidence: High
```

The `Terminal` documentation explicitly warns that a failed draw may already
have resized buffers, written part of a diff, or left cursor state unapplied, and
recommends treating the error as fatal for that session. The implementation
calls `swap_buffers` before the backend's final `flush`. If that flush fails,
Ratatui's logical previous-frame baseline has advanced even though delivery may
be incomplete.

This does not make Ratatui incorrect under its documented contract. Most local
TUI applications exit after output failure. Ratatui's `Terminal` cannot defer
its own logical buffer swap until the final backend flush is confirmed. A custom
stateful backend could potentially retain a complete shadow screen, stage
output, and force its own full repaint after failure even when Ratatui later
provides an empty diff. That design was not found in the standard backends or
tested in this research. ArborUI should keep transaction ownership above its
backend if commit-after-acceptance must be guaranteed by the public contract.

### Inline Cursor Queries Must Coordinate With Input Ownership

```text
Classification: Bug
Requirement: Construct, resize, autoresize, or clear an inline viewport while an application event stream is active
Library assumption: Cursor position can be queried through the backend when needed
Observable friction: The application's input reader may consume the cursor-position response
Root cause: Terminal responses and application input share one byte stream without one coordinating owner
Workaround: Construct before starting input, or wrap the backend and provide tracked cursor state
Cost: Lifecycle restrictions or custom backend state that each affected application must implement
Current status: Open issue #2640 after 0.30.2
Evidence status: Supported
Confidence: Medium
```

[Issue #2640](https://github.com/ratatui/ratatui/issues/2640) reports that
inline viewport construction, resize or autoresize, and `Terminal::clear` can
issue cursor-position queries while another Crossterm input reader is active.
The source confirms that inline construction and resize obtain a cursor position
through `compute_inline_size`, while `clear` snapshots the backend cursor. The
race itself was not reproduced in this research.

The issue is valuable beyond Ratatui: terminal capability and cursor queries are
input events, not independent request-response calls. ArborUI should maintain a
single input owner that routes protocol responses separately from application
events.

### Full Rendering Work Scales With Every Requested Frame

```text
Classification: Performance tradeoff
Requirement: Efficient updates for very large trees or collections
Library assumption: Applications redraw the complete visible UI when they choose to draw
Observable friction: Widget painting and full-buffer comparison repeat for every requested frame
Root cause: Immediate complete-frame rendering without retained dirty subtrees
Workaround: Draw only after changes, coalesce events, cache application data, and virtualize collections
Cost: Optimization policy and retained caches move into application or widget code
Current status: Intentional in 0.30.2
Evidence status: Verified
Confidence: High
```

Ratatui avoids terminal work while an application chooses not to call `draw`,
and its diff reduces bytes emitted. It does not automatically avoid unchanged
paint work inside a requested frame. The default facade does enable an LRU
layout cache, so repeated identical `Layout` and `Rect` calculations generally
reuse solved geometry rather than rerunning the constraint solver. Large
collections, rapid streaming updates, graphics-heavy buffers, or unnecessarily
frequent tick loops can still make repeated painting and comparison visible.
No workload was measured here, so user-visible cost remains inferred.

This supports ArborUI's plan to keep a full-render reference path, coalesce
invalidations, remain idle when possible, and add incremental work only after
measurement. It does not by itself prove that ArborUI's more complex retained
architecture will outperform Ratatui.

### Unicode Correctness Still Crosses The Terminal Boundary

```text
Classification: Ecosystem tradeoff
Requirement: Predictable grapheme placement across terminals and fonts
Library assumption: Unicode segmentation and width libraries provide the logical cell model
Observable friction: Some terminals disagree about emoji, ambiguous width, or combining behavior
Root cause: Terminal width behavior is not fully standardized or reliably queryable
Workaround: Test target terminals, constrain supported environments, and apply targeted compatibility fixes
Cost: Compatibility matrices and occasional terminal-specific behavior remain necessary
Current status: Inherent; Ratatui actively fixes concrete regressions
Evidence status: Supported
Confidence: Medium
```

Ratatui's logical handling is substantially stronger than treating Rust `char`
values as terminal cells. It segments grapheme clusters, measures width, omits
wide text that cannot fit, and contains regression tests for CJK and emoji
cases. The remaining limitation is not unique to Ratatui. Buffer tests cannot
prove how a terminal emulator and font will advance the physical cursor.

Normal rendering uses `unicode-width`'s ordinary width calculation, with
targeted handling for halfwidth Katakana marks. Some public text measurements
also expose CJK width, but no renderer-wide configurable ambiguous-width policy
was found. Physical final-column and autowrap behavior was not established by
the logical test suite and remains unknown without terminal-level evidence.

ArborUI's explicit continuation cells and width policies offer stronger internal
invariants, but still require PTY or emulator compatibility evidence. The lesson
is to preserve a configurable width contract and avoid claiming universal
Unicode rendering from logical tests alone.

### Clear Can Leak A Wide Glyph Across A Popup's Left Edge

```text
Classification: Bug
Requirement: Clear a popup rectangle without leaking wide content from the underlying layer
Library assumption: Resetting cells inside the rectangle is sufficient
Observable friction: A wide glyph starting immediately left of the rectangle can overlap its first column
Root cause: Clear resets only cells inside its rectangle and does not sanitize the left neighbor
Workaround: Replace a wide left neighbor with a space, expand the clear area, or avoid placing the edge through the glyph
Cost: Application-specific boundary handling until an upstream fix ships
Current status: Open issue #2526 against 0.30; implementation unchanged in 0.30.2
Evidence status: Supported
Confidence: Medium
```

The pinned [`Clear` implementation](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-widgets/src/clear.rs#L38-L48)
resets only cells inside the intersection rectangle.
[Issue #2526](https://github.com/ratatui/ratatui/issues/2526) supplies a
logical-buffer report and a downstream Yazi example. The author of
[PR #2628](https://github.com/ratatui/ratatui/pull/2628) reproduced and proposed
a fix for the left-edge overlap but could not reproduce the issue's proposed
right-edge continuation case. This report therefore treats only the left-edge
behavior as supported. It was not independently reproduced during this
research.

## Testing Strategy

### Direct Buffer Tests

Ratatui recommends rendering widget units directly into `Buffer` rather than
using `TestBackend`. This invokes the production `Widget::render` or
`StatefulWidget::render` implementation. Expected buffers can be constructed
from lines and styled spans, so equality checks cover symbols, colors,
modifiers, and other cell metadata.

A representative pattern is:

```rust
let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
list.render(buffer.area, &mut buffer, &mut state);

assert_eq!(
    buffer,
    Buffer::with_lines([
        Line::from(vec![">>".italic().red(), "Item 1  ".bold().red()])
    ])
);
```

The repository's
[`List` tests](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-widgets/src/list.rs#L501-L627)
demonstrate this style. It is fast, deterministic, and semantically close to the
widget contract. It deliberately bypasses terminal diffing and backend output.

### Terminal And TestBackend Tests

`TestBackend` implements the production `Backend` trait with an in-memory screen,
cursor, and scrollback buffer. Used through `Terminal<TestBackend>`, it exercises
the real draw callback, buffer diff, cursor handling, buffer swap, automatic
resize, viewport logic, and logical line insertion. Public assertions cover the
main buffer, scrollback, cursor position, and cursor visibility.

This is an integration test of Ratatui's logical terminal pipeline, not a
terminal emulator. `TestBackend::Error` is `Infallible`, and it does not contain
an input event queue. Applications can implement another `Backend` for recording
or failure injection, but the standard test backend does not supply that
behavior.

The boundary is well chosen for Ratatui's scope. Application authors can render
complete screens without a TTY, but Ratatui cannot drive an event loop it does
not own.

### Snapshots

The official
[snapshot recipe](https://ratatui.rs/recipes/testing/snapshots/) combines
`Terminal<TestBackend>` with `insta` at an explicit terminal size. The displayed
snapshot is a character representation of the buffer and intentionally omits
color. Exact `Buffer` equality can assert styles, but the convenient display
snapshot cannot. Color-aware snapshot output remains under discussion in
[issue #1402](https://github.com/ratatui/ratatui/issues/1402).

No repository-owned `insta` snapshot suite was found at the recorded revision;
the core project primarily uses explicit buffer assertions. That choice keeps
expected semantics visible in Rust for many widget cases, while applications can
adopt snapshots for larger complete screens.

### Application Logic And Events

Because Ratatui does not own input or application state, the normal application
testing pattern is to extract event handlers, construct backend event values,
invoke those handlers directly, and assert model state. Rendering can then be
tested separately through a buffer or `TestBackend`.

This separation can be excellent when application architecture already exposes
pure update functions. It does not provide synthetic event injection into a
running application, deterministic clocks, timer advancement, command
completion, or a run-until-idle operation. Users must create those facilities in
their own runtime layer.

### Backend And Compatibility Tests

The repository includes backend-focused tests. For example, a Termion
integration test runs a `Terminal` through a writer and asserts emitted escape
bytes and reduced output on the second frame. Other backend crates test command,
style, or mock-terminal behavior. The pinned
[CI workflow](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/.github/workflows/ci.yml#L147-L313)
compiles and tests multiple operating systems, backends, feature combinations,
and `no_std` builds, and records coverage. Crossterm version combinations are
also exercised by the repository's backend test commands.

No PTY or terminal-emulator suite, general fuzz/property suite, or fault matrix
for every backend operation was found at the recorded revision. Therefore raw
mode restoration, alternate-screen behavior, real cursor-query races, terminal
width disagreements, signal behavior, and partial physical writes are not
covered by `TestBackend`.

### Benchmarks

Criterion benchmarks cover several widgets, text and line rendering, layout
constraints, rectangle iteration, and buffer construction. No merged benchmark
suite was found for complete application turns, terminal write completion,
queue latency, idle CPU, or physical backend recovery. The benchmarks support
local optimization but do not establish end-to-end application performance or
cross-project superiority.

### Testing Capability Summary

| Capability | Ratatui 0.30.2 |
| --- | --- |
| Render widgets without a terminal | Strong: direct production `Buffer` path |
| Render a complete logical screen | Strong: `Terminal<TestBackend>` |
| Assert characters and styles | Strong with `Buffer` equality |
| Character snapshots | Documented through external `insta` |
| Styled display snapshots | Not provided by `TestBackend::Display` |
| Inspect cursor and scrollback | Supported |
| Inject input into an application | Application-defined |
| Control clocks and timers | Application-defined |
| Wait for application settlement | Not provided |
| Inject backend failures | Requires a custom backend |
| Recover after uncertain output | Not guaranteed; session termination recommended |
| PTY or emulator validation | Not found in the core repository |
| Property or fuzz testing | Not found in the core repository |
| Cross-platform and backend CI | Strong compile and logical-test coverage |
| End-to-end application benchmarks | Not found |

## Common Scenario Assessment

| Scenario | Assessment |
| --- | --- |
| Form with focus and modal | Visual composition is straightforward; focus, validation, event routing, and modal policy are application-owned |
| Large keyed collection | List and table visuals exist; stable identity and visible-range construction are application-owned |
| Streaming external updates | Compatible with threads or async runtimes; scheduling, coalescing, and backpressure are application-owned |
| Unicode text input | Unicode rendering is capable; editing model, cursor movement, selection, and input widget behavior require other code |
| Overlay with mouse interaction | Later rendering supports visual overlays; a wide glyph immediately left of `Clear` is a current issue, while hit testing, targeting, and capture are application-owned |
| Resize during updates | Fullscreen and inline draws automatically recheck size; fixed viewports require explicit resize |
| Deferred or failed output | No deferred outcome; backend errors can make state uncertain and normally terminate the session |
| Suspend to a child process | Terminal setup can be restored and recreated, but handoff and resume orchestration are application-owned |
| Long idle periods | Efficient if the application blocks and does not draw; Ratatui does not choose the event-loop policy |
| Native scrollback conversation | Inline viewport and `insert_before` provide relevant primitives, with cursor-query and fallback-repaint constraints |

## Lessons For ArborUI

### Adopt Or Preserve

- Keep the application facade distinct from the smaller widget-author contract.
- Make direct semantic buffer rendering the preferred widget unit-test path.
- Keep complete logical-screen tests easy and independent of a physical terminal.
- Treat full-screen, inline, fixed-region, and native-scrollback modes as explicit contracts.
- Provide low-level rendering escape hatches without making them the ordinary application API.
- Invest in common widgets, examples, templates, and ecosystem documentation alongside architecture.
- Treat governance, maintainer continuity, and contribution flow as adoption-critical properties.

### Preserve ArborUI's Different Guarantees

- Retain prepared-frame commit only after complete backend acceptance.
- Mark physical state unknown after any possibly partial output and force a full repaint.
- Keep one owner for terminal input and route protocol responses before application events.
- Provide retained identity for focus, mouse capture, hit testing, and dynamic collections.
- Keep deterministic time, event injection, effect completion, and visual settlement in the public application harness.
- Test lifecycle behavior with PTYs because a logical backend cannot represent it.

### Avoid Overstating The Comparison

Ratatui's absent runtime features are largely intentional. ArborUI must show that
its additional framework policy reduces total application complexity without
making ordinary programs harder to integrate. In particular:

- Explicit invalidation may create errors and cognitive load that Ratatui avoids by redrawing.
- Retained identity and transactional staging add implementation complexity.
- A smaller widget ecosystem can dominate users' practical evaluation.
- Full layout and paint work currently weaken any unmeasured performance claim.
- Alternate-screen reliability does not prove useful inline or native-scrollback semantics.

### Follow-Up Experiments

1. Build the same moderate application in facade-only ArborUI and idiomatic Ratatui, then compare application-owned code, tests, and failure handling.
2. Benchmark complete turns for idle, one-cell updates, large lists, overlays, Unicode, resize storms, and streaming updates.
3. Add a Ratatui-style direct widget buffer test example to ArborUI documentation and compare assertion ergonomics.
4. Prototype explicit inline and native-scrollback contracts before extending ArborUI's current screen mode.
5. Retain PTY and fault-injection tests as a differentiator, but verify that they catch failures seen in real applications.

## Pilot Calibration

The research strategy was workable at deep-dive depth. The 3,000-6,000 word
target is appropriate for a direct competitor when architecture, testing, and
failure semantics all matter. Source inspection was necessary: high-level docs
would not reveal the buffer-swap-before-final-flush sequence or the precise
`TestBackend` boundary.

Three process refinements should carry into later reports:

- Pin release source and website documentation separately because current website guidance may postdate a stable release.
- For a negative repository finding, record the searched revision and facilities rather than claiming universal absence.
- Distinguish missing framework features from failures of the project's intended library contract before assigning a limitation.

No implementation prototype was required for the main conclusions. The
cursor-query race remains supported rather than verified because it was not
reproduced. Performance remains a structural analysis rather than a comparative
claim because no equivalent application benchmark was run.

## Evidence Appendix

All sources were accessed on 2026-07-16.

| Claim | Source | Version or revision | Source date | Accessed | Status | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| Stable baseline | [0.30.2 release](https://github.com/ratatui/ratatui/releases/tag/ratatui-v0.30.2) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Latest stable release on research date |
| Modular crate boundaries | [Architecture](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ARCHITECTURE.md) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Introduced in 0.30 |
| Immediate complete-frame model | [Crate documentation](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui/src/lib.rs#L131-L149) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Application redraws the desired UI |
| Terminal pipeline and error contract | [`Terminal` source](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/terminal.rs#L112-L157) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Documents partial progress on errors |
| Buffer swap precedes backend flush | [`apply_buffer_with_cursor`](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/terminal/render.rs#L288-L319) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Standard contract has no commit-after-acceptance guarantee |
| Changed-cell generation | [`Terminal::flush`](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/terminal/buffers.rs#L76-L114) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Compares logical buffers and calls backend draw |
| Widget contract | [`Widget` source](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/widgets/widget.rs#L7-L76) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Widgets are render commands |
| Application-owned widget state | [`StatefulWidget` source](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/widgets/stateful_widget.rs#L124-L134) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | State is passed to rendering |
| Application owns events | [Pinned event-handling guide](https://github.com/ratatui/ratatui-website/blob/84e68a03a243367b22c1868447258cac2394dc35/src/content/docs/concepts/event-handling.md#L5-L72) | website `84e68a03` | 2026-07-15 | 2026-07-16 | Supported | Documents multiple optional patterns |
| Library versus framework intent | [Pinned FAQ](https://github.com/ratatui/ratatui-website/blob/84e68a03a243367b22c1868447258cac2394dc35/src/content/docs/faq.md#L190-L237) | website `84e68a03` | 2026-07-15 | 2026-07-16 | Supported | Explicitly preserves application control flow |
| Viewport contracts | [`Viewport` source](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/terminal/viewport.rs#L5-L118) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Fullscreen, inline, and fixed |
| Inline insertion behavior | [`Terminal` inline docs](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/terminal.rs#L243-L280) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Scrolling-region feature changes path |
| Inline resize cursor query | [`Terminal::resize`](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/terminal/resize.rs#L23-L35) and [`compute_inline_size`](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/terminal/inline.rs#L390-L419) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Confirms cursor query path; not the external read race itself |
| Initialization and restoration | [Facade implementation](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui/src/init.rs#L397-L559) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Raw and alternate-screen setup and teardown |
| Panic restoration | [Panic-hook implementation](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui/src/init.rs#L562-L571) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Restore-first hook for facade helpers |
| Logical integration backend | [`TestBackend` source](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/backend/test.rs#L13-L18) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Intended for complete UI integration tests |
| TestBackend is infallible | [`Backend` implementation](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/backend/test.rs#L247-L260) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Failure injection needs another backend |
| Direct widget buffer tests | [`List` tests](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-widgets/src/list.rs#L501-L627) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Production widget rendering with style assertions |
| Terminal-level buffer tests | [`BarChart` integration test](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui/tests/widgets_barchart.rs#L8-L34) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Uses Terminal and TestBackend |
| Character snapshot recipe | [Pinned snapshot guide](https://github.com/ratatui/ratatui-website/blob/84e68a03a243367b22c1868447258cac2394dc35/src/content/docs/recipes/testing/snapshots.md#L21-L120) | website `84e68a03` | 2026-07-15 | 2026-07-16 | Supported | External `insta`, fixed size, no color display |
| Color snapshot limitation | [Issue #1402](https://github.com/ratatui/ratatui/issues/1402) | Open | 2024-10-05 | 2026-07-16 | Reported | Exact styled buffer assertions remain possible |
| Inline cursor-query race | [Issue #2640](https://github.com/ratatui/ratatui/issues/2640) | Open after 0.30.2 | 2026-07-08 | 2026-07-16 | Supported | Query sites verified; race not reproduced here |
| Clear left-edge wide-glyph bug | [Issue #2526](https://github.com/ratatui/ratatui/issues/2526), [PR #2628](https://github.com/ratatui/ratatui/pull/2628), and [`Clear` source](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-widgets/src/clear.rs#L38-L48) | Open; source `e665c36` | 2026-05-10 and 2026-07-02 | 2026-07-16 | Supported | Left edge reproduced upstream; right-edge report not confirmed; neither rerun here |
| Wide-cell cleanup regression fixed | [0.30.2 release notes](https://github.com/ratatui/ratatui/releases/tag/ratatui-v0.30.2) | 0.30.2 | 2026-06-19 | 2026-07-16 | Verified | Fix and regression coverage shipped |
| Default layout cache | [Facade features](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui/Cargo.toml#L23-L30) and [cached split](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-core/src/layout/layout.rs#L738-L756) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Reuses identical layout calculations |
| Historical strengths and limitations | [`tui-rs` issue #654](https://github.com/fdehau/tui-rs/issues/654) | Historical | 2022-08-14 | 2026-07-16 | Reported | Current Ratatui has addressed several listed limitations |
| Widget inventory | [`ratatui-widgets`](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui-widgets/src/lib.rs#L34-L72) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Standard visual widget catalog |
| Benchmark scope | [Benchmark registry](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/ratatui/benches/main.rs#L1-L30) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | Widget and rendering microbenchmarks |
| CI and compatibility scope | [Pinned CI workflow](https://github.com/ratatui/ratatui/blob/e665c36cb14752a61cd777fbd06dbef8474f2add/.github/workflows/ci.yml#L147-L313) | `e665c36` | 2026-06-19 | 2026-07-16 | Verified | OS, backend, `no_std`, and coverage jobs |
| PTY, fuzz, and property suites not found | [Pinned repository](https://github.com/ratatui/ratatui/tree/e665c36cb14752a61cd777fbd06dbef8474f2add) | `e665c36` | 2026-06-19 | 2026-07-16 | Inferred | Searched workspace, dependencies, tests, and workflows |
| Governance succession | [`tui-rs` discussion](https://github.com/fdehau/tui-rs/issues/654) and [pinned Ratatui FAQ](https://github.com/ratatui/ratatui-website/blob/84e68a03a243367b22c1868447258cac2394dc35/src/content/docs/faq.md#L165-L188) | Historical and website `84e68a03` | 2022-08-14 and 2026-07-15 | 2026-07-16 | Supported | Fork occurred with original author's blessing |
