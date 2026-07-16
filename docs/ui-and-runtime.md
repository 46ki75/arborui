# UI And Runtime

## Application Model

The primary API follows model-update-view:

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

Application state has one owner. Terminal events, widget actions, timers, and
completed futures all become messages processed by `update`.

The facade prelude supports this application shape:

```rust
struct Counter {
    count: usize,
    label: String,
}

enum Message {
    Increment,
    Quit,
}

impl Application for Counter {
    type Message = Message;

    fn update(
        &mut self,
        message: Message,
        context: &mut UpdateContext<Message>,
    ) -> Command<Message> {
        match message {
            Message::Increment => {
                self.count += 1;
                self.label = format!("Count: {}", self.count);
                context.invalidate(Invalidation::Recompose);
            }
            Message::Quit => return Command::quit(),
        }

        Command::none()
    }

    fn view(&self) -> Element<'_, Message> {
        column([
            text(&self.label),
            button("Increment", || Message::Increment).build(),
        ])
    }
}
```

## Ephemeral Elements

`Element<'a, Message>` is a frame-local declaration. It may borrow text and
collections from the model. Elements contain enough information to reconcile
identity, measure, paint, and route a current event.

Borrowed values are never stored in the retained tree. Event bindings are
consumed during synchronous event dispatch. The runtime obtains a fresh view
for a later event.

This avoids the unsafe lifetime extension required by some retained Rust UI
frameworks while allowing applications to avoid unnecessary cloning.

## Retained Tree

```rust
pub struct RetainedNode {
    key: Key,
    kind: WidgetKind,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    layout: Rect,
    focus: FocusMetadata,
    interaction: InteractionState,
    invalidation: Invalidation,
}
```

The retained tree provides stable identity for:

- Reconciliation
- Focus
- Mouse capture
- Hover transitions
- Scroll and selection metadata
- Layout caching
- Paint invalidation
- Component-scoped tasks in a future extension

The tree does not own the application model or terminal session.

## Keys And Reconciliation

Static children may use structural position for identity. Dynamic collections
must use explicit keys.

```rust
list(items.iter().map(|item| {
    row_for(item).key(item.id)
}))
```

A node is reused only when its key and widget kind are compatible. Replacing a
node clears incompatible retained interaction state and repairs focus.

Duplicate sibling keys are errors in debug builds and should produce a clear
diagnostic in release builds.

## Widget Contract

The exact trait split will be prototyped, but widgets need four capabilities:

```rust
pub trait Widget<Message> {
    fn layout(&self, context: &mut LayoutContext<'_>) -> LayoutNode;

    fn paint(&self, context: &mut PaintContext<'_>);

    fn event(
        &self,
        event: &UiEvent,
        context: &mut EventContext<'_, Message>,
    );

    fn accessibility(&self, context: &mut AccessibilityContext<'_>);
}
```

Accessibility may initially be minimal, but reserving a semantic output path
prevents visual cells from becoming the only representation of the interface.

Widgets are controlled by default:

```rust
text_input(&model.query, Message::QueryChanged)
    .on_submit(|| Message::Submit)
    .build()
```

The actual mapping API must support messages containing event data without
requiring hidden mutation of the application model.

## Event Routing

Terminal events are normalized before entering the UI. Spatial and focused
events then use three-phase dispatch:

```text
root -> parent -> target    capture
                  target
target -> parent -> root    bubble
```

`EventContext` supports:

- Emit application message
- Mark handled
- Prevent default behavior
- Stop propagation
- Request focus
- Capture or release pointer
- Request invalidation

Global shortcuts are capture handlers on an application root, not a separate
broadcast channel.

The initial implementation invokes phase-specific handlers in this order:
capture from root through target, target, then bubble from target through root.
Stopping propagation skips all later handlers, including later handlers on the
same node. Marking an event handled and preventing its default behavior do not
stop propagation. Handler requests are applied after routing, and the last
focus or pointer-capture request wins.

### Event Categories

- Key press, repeat, and release
- Text input
- Bracketed paste
- Mouse press, release, move, drag, and scroll
- Focus gained and lost
- Terminal focus gained and lost
- Resize
- Tick or animation frame
- Custom application event

Terminal capability responses are normally consumed by the terminal session
before UI dispatch.

## Hit Testing

The renderer creates a hit map from the final composited surfaces. Each visible
cell identifies the topmost interactive retained node.

This guarantees that z-order, clipping, overlays, and hit testing agree. Mouse
capture overrides ordinary hit testing for drag and release events.

Pointer modality is separate from keyboard focus scopes. The deepest visible
`Element::pointer_modal` boundary receives pointer events targeted outside its
subtree and cancels lower-scope capture. If a captured node is removed, the
remaining drag and release events for each in-flight pointer button are consumed
rather than exposed to newly uncovered controls. Hover state and enter/leave
transitions still follow the current pointer position.

Hit maps are committed transactionally with rendered frames. UI pointer capture
is distinct from terminal mouse-reporting mode. Hover enter and leave are
target-only transitions recalculated from the committed hit map after a frame.
`UiTree::prepare` stages retained state in `PreparedUiFrame`; committing through
the tree advances UI and renderer state together. Stale, out-of-order, and
cross-renderer commits are rejected, and dispatch accepts only the renderer
state committed with the tree.

Hover changes are recalculated after a frame when geometry or z-order changes,
even when the mouse does not move.

## Focus

The focus manager owns one active focus target per focus scope.

Responsibilities include:

- Forward and reverse tab traversal
- Programmatic focus by key
- Mouse-based focus
- Focus scopes for dialogs and overlays
- Restoring previous focus when a scope closes
- Recovering when a focused node is removed
- Optional directional navigation
- Keeping a real terminal cursor synchronized with editable controls

Focus traversal uses retained tree order unless a widget supplies an explicit
order.

The deepest retained focus scope is active. Each scope keeps its focused node,
so removing an overlay restores the previous scope's focus. Traversal wraps at
scope boundaries; explicit order is sorted before structural order. A focused
element may provide a local terminal cursor intent, which is translated and
clipped to the viewport after layout. Collapsed or fully clipped descendants do
not participate in traversal or programmatic focus.

`Element::focus_style` applies an inheritable visual overlay while that element
owns retained focus. Standard buttons use reverse video across their complete
label by default; editable text inputs continue to expose a bar cursor.

## Commands

`Command<Message>` is an opaque public type with constructors rather than a
public enum whose representation becomes permanent.

Initial constructors:

```rust
Command::none()
Command::message(message)
Command::batch(commands)
Command::perform(future, map_output)
Command::after(duration, message)
Command::quit()
```

Commands make effects visible while preserving serialized application updates.

The runtime also provides a thread-safe proxy:

```rust
let proxy: EventProxy<Message> = runner.event_proxy();
if let Err(error) = proxy.send(Message::Loaded(data)) {
    match error.kind() {
        EventProxySendErrorKind::Full => retry_or_drop(error.into_inner()),
        EventProxySendErrorKind::Closed => stop_producer(error.into_inner()),
    }
}
```

This is the primary integration point for Tokio, async-std, smol, worker
threads, subprocess readers, and external callbacks.

External ingress uses a positive, bounded capacity. The default is 1,024
messages; `RuntimeOptions::with_event_ingress_capacity` configures another
capacity for `AppRunner::new_with_options`, `run_with_options`, and
`TestApp::with_runtime_options`. `EventProxy::send` does not wait for capacity:
it accepts the new message exactly once or returns an error retaining ownership
of that message. `Full` leaves the accepted FIFO unchanged and does not wake the
runner. `Closed` means orderly shutdown or runner destruction has ended ingress.
A successful send proves admission, not that the application update has run;
work admitted immediately before concurrent shutdown may be discarded.

`EventProxy::metrics` snapshots configured capacity, current depth, high-water
mark, full-rejection count, and closure. Clones share one queue and its metrics.
Accepted messages from one producer remain FIFO; interleaving between concurrent
producers is unspecified. A rejected message has no queue position, so retrying
it may place it after messages accepted in the meantime.

The initial implementation uses explicit invalidation through `UpdateContext`.
An update that mutates visible model state requests `Paint`, `Layout`, or
`Recompose`; updates that request no visual work do not rebuild a view. Immediate
messages in command batches preserve declaration order; future outputs are
delivered when they complete. Quitting cancels unfinished command futures when
the runner is dropped.

Explicit invalidation is a performance contract rather than a process-safety
boundary. If an application changes view structure without requesting
`Recompose`, the terminal runtime detects the mismatch at the next event. It
retains that event, requests recomposition, and dispatches the event exactly once
only after the matching UI tree, renderer, and hit map commit. Deferred writes
continue to defer dispatch, and uncertain writes force a full repaint before
dispatch. This recovery costs an extra view build and frame, so applications
should still request the correct invalidation during `update`.

Only `ViewDoesNotMatchCommittedTree` receives this orchestration-level recovery.
Duplicate sibling keys and use of the wrong committed renderer remain errors,
and direct lower-level dispatch retains its fallible contract for diagnosing
misuse. `arborui-test` follows the same ordering and returns a dispatch report
only after the retained event has actually been routed. Its fallible event API
surfaces a recovery write failure while retaining the event; callers resume by
retrying the same `UiEvent`. A different event is rejected with
`TestError::RecoveryEventMismatch` so input cannot be reordered or dropped.

## Async Runtime Independence

The core runtime may poll self-waking futures that do not require a particular
reactor. Applications using runtime-specific I/O should run that work in their
chosen executor and send results through `EventProxy`.

Future extensions may add scoped task IDs and cancellation. Task ownership must
be explicit; removing a retained component must not silently leave a task with
a stale UI handle.

Until scoped cancellation exists, an application integrating a long-running
producer owns its cancellation signal. Messages from replaceable work should
carry an application generation or request ID so an item or completion racing
with cancellation can be rejected during serialized update. Focus Queue's
producer recovers and retries `Full` messages while it remains current, and stops
on cancellation or `Closed`.

The ingress bound applies only to messages waiting in `EventProxy`. Direct
`AppRunner::enqueue`, UI handler output, immediate commands, timers, and future
completion use internal serialized queues. Bounded proxy ingress is therefore an
observable overload boundary, not a claim that every source of application work
or all application memory is bounded.

## Scheduler

The scheduler has independent queues for:

- Terminal events
- Application messages
- Completed commands
- Render requests
- Timers or animation frames

Scheduling rules:

- Application updates are serialized.
- Multiple invalidations in one turn coalesce into one frame.
- Idle applications do not continuously render.
- Interactive input has priority over low-priority animation work.
- Resize invalidates layout and terminal state.
- Backpressure may drop obsolete prepared frames but never reorder terminal bytes.
- Shutdown drains or cancels work according to documented command policy.

Concurrent application updates are not an initial goal. Concurrency belongs in
effects; model mutation remains deterministic.

The runtime polls self-waking futures without selecting an async reactor. Work
that requires Tokio, async-std, smol, or another reactor runs on that executor
and reports owned messages through `EventProxy`. Terminal writes commit UI,
hit-map, and renderer state only after `WriteOutcome::Applied`; deferred frames
are discarded, and unknown output state forces a complete repaint.

Runtime timers use a monotonic `Clock`. Normal runners install `SystemClock`;
headless harnesses supply a manual clock through `AppRunner::new_with_clock` so
`Command::after` can be tested without sleeping. `is_visually_idle` ignores
dormant futures and future timer deadlines while still reporting queued
messages, ready tasks, and visual invalidation.

Each scheduler turn has a finite work budget. Arrived application messages are
processed before another bounded group of future polls. Internal and external
message sources alternate when both remain ready so neither can monopolize the
update budget; each source retains its own FIFO order. Terminal input is checked
between saturated turns. Since terminal polling is a synchronous backend
contract, proxy messages arriving during a poll are observed no later than the
caller-configured poll interval.

## Standard Widgets

The first standard widget set is:

- Text
- Block
- Row
- Column
- Stack
- Spacer
- Button
- Checkbox
- Text input
- Scroll view
- List
- Dialog

Tables, trees, forms, markdown, charts, and editors should begin as separate
ecosystem crates until their lower-level requirements are understood.

The initial widgets are controlled. `TextInput` borrows an application-owned
`TextBuffer` and emits an updated owned buffer; `Checkbox` borrows a boolean and
emits the next value; `ScrollView` borrows an offset and emits signed deltas.
Button activation uses a repeatable message factory, so application messages do
not need to implement `Clone`. `Dialog` fills its containing layout region with
a focus scope and pointer barrier around caller-supplied content while leaving
open or closed state in the application. A viewport modal therefore uses a
viewport-sized stack host with the dialog as its final layer. Block painting and
stack/scroll composition use the backend-neutral `Element` paint and layout
contracts rather than terminal-backend types.

## Public Headless Harness

`arborui-test` owns an in-memory terminal and drives the same `AppRunner`, retained
tree, renderer, and transactional write path as a real application. `TestApp`
supports key, mouse, paste, resize, direct UI event, and external-proxy input;
manual time advancement; and settling until no immediate visual work remains.
Runtime options can set a small ingress capacity for deterministic saturation
tests. `TestApp::send` is direct model-message injection and intentionally
bypasses proxy admission; tests of external pressure use `event_proxy`.

The committed `TestFrame` exposes resolved graphemes, styles, hyperlinks, and
cursor state. Its `Display` representation is a character snapshot, while its
`Debug` representation retains styled-cell and continuation details. Submitted
`FramePatch` values, focused keys, retained nodes, and hit-map results remain
available for structural assertions.

Output behavior can be scripted as deferred, state-unknown, or failed. Deferred
and failed writes leave the committed test frame unchanged; unknown and failed
writes require the next successful attempt to be a full repaint.
