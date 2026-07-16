# Facade-Only Application Proof

Experiment dates: 2026-07-16 and 2026-07-17

## Question

Can a downstream ArborUI application implement and deterministically test a
controlled modal form with Unicode editing, keyed application state, focus
trapping, focus restoration, and pointer isolation without importing
implementation crates?

Can the same application add a second screen with externally produced updates,
cooperative cancellation, stale-result rejection, explicit settlement, and
recoverable errors through the public runtime and test facades?

Can external proxy ingress reject new work at a configured bound, expose
pressure, return ownership of rejected messages, and recover after draining
without moving producer backlog into an unbounded internal queue?

Can a facade-only application construct only a fixed or variable-height visible
range while preserving stable identity, collection focus, selection, overscan,
and measured scroll semantics across a million logical rows?

These are the first four bounded slices of the production-scale application
proof. They do not attempt to prove comparative ergonomics.

## Implementation

The existing [`Focus Queue`](../../../examples/focus-queue/) pilot now supports
editing a keyed task in a modal form. The form contains:

- A controlled grapheme-aware title input
- A controlled completion checkbox
- Explicit Save and Cancel actions
- Escape dismissal and scrim pointer isolation
- Forward and reverse focus traversal within the dialog
- Restoration to the originating keyed Edit control

The application continues to depend only on `arborui`. Its integration tests
use `arborui-test` plus the snapshot assertion library. The application does not
import an ArborUI implementation crate.

Two application-driven public widgets were added:

- `Checkbox`, which borrows a boolean value and emits the next value
- `Dialog`, which fills its containing overlay region with a focus scope,
  centers caller-supplied content, blocks lower pointer targets, and emits
  dismissal from Escape

Both widgets retain no application references or callbacks beyond the
frame-local `Element` value.

The second slice adds a persistent Queue and Activity navigation row. Activity
screen state remains owned by the application while either screen is visible.
Starting activity launches a demonstration producer on an operating-system
thread. It sends generation-tagged items and completion through `EventProxy`.
The application owns a cooperative cancellation signal, advances the generation
when cancelling or restarting, and ignores every item, completion, or failure
whose generation is no longer current.

The activity state machine distinguishes idle, running, cancelled, completed,
and failed states. Failures expose a Retry action. The application retains at
most 32 accepted log items with stable keys and renders the newest first. This
model bound remains distinct from runtime ingress capacity.

The third slice replaces unbounded proxy transport with a dedicated bounded
queue. `RuntimeOptions` selects a positive capacity and defaults to 1,024.
`EventProxy::send` rejects the new message with `Full` when occupied or `Closed`
after shutdown, retaining the unsent message in both cases. Shared metrics expose
depth, high-water mark, full rejections, capacity, and closure. The runner
consumes external messages directly instead of bulk-moving them into its
internal message deque, and alternates external and internal sources when both
are ready.

Focus Queue configures capacity eight. Its demonstration worker retries the same
recovered item or terminal result after `Full`, while cancellation remains able
to stop retries; `Closed` ends the producer. This is one explicit application
policy, not automatic throttling supplied by the runtime.

The fourth slice adds a separate facade-only
[`Collection Lab`](../../../examples/collection-lab/) rather than promoting an
unproven generic widget. Its fixed-height provider calculates a range in constant
time from row height, viewport height, scroll offset, and row-based overscan. Its
variable-height provider caches explicit non-zero item heights and prefix sums,
uses binary search for cell-based overscan, and preserves an item plus intra-item
anchor when a cached measurement changes.

The application constructs elements only for that range, keys rows with stable
model IDs, and translates the local range by the scroll offset within its first
item. Active and selected IDs remain application state. The viewport is one
stable focus target that implements arrows, Home, End, Page Up, Page Down, and
selection without making transient rows tab stops. Pointer selection maps a
visible row back to its stable ID. Reversing item order preserves active and
selected IDs while rebuilding measurement order.

The variable prototype deliberately measures explicit multiline content rather
than guessing wrapped text height. Both providers consume an application-owned
viewport height because `Application::view` cannot currently construct children
after layout resolves the viewport. Resize events update that height after the
initial explicit value.

## Deterministic Evidence

The public application harness verifies:

- Opening the dialog focuses its title input.
- Tab and Shift-Tab wrap inside the active focus scope.
- Cancel preserves the original task and restores focus to its keyed Edit
  control.
- A scrim click remains in the dialog without activating the covered task row.
- Editing `a👩‍💻界` can delete the ZWJ emoji as one grapheme and save `a界`.
- Saving the controlled checkbox updates the task and summary.
- Character snapshots cover the open dialog and saved Unicode state, with
  semantic assertions for model and focus state.
- Screen navigation preserves queue and timer state and keeps the keyed
  navigation control focused across recomposition.
- A controlled launcher receives the production `EventProxy` and cancellation
  signal without sleeping in tests.
- Proxy-delivered items settle deterministically through `TestApp::settle`.
- A barrier-coordinated worker thread observes cancellation and submits raced
  items and completion from the preceding generation, which are rejected.
- Failure is recoverable through a new generation; stale completion cannot
  settle the retry.
- Accepted history remains at 32 items, with semantic assertions for the
  retained range.
- Character snapshots cover idle and completed Activity states, including the
  maximum retained-history viewport.
- Capacity-two facade tests accept exactly two items, reject the third without
  changing the queue, inspect shared pressure metrics, drain accepted items,
  retry the recovered item, and settle completion.
- Runtime tests verify ownership recovery, clone-shared capacity, FIFO delivery,
  capacity release, high-water and rejection metrics, wake behavior, and closure
  after quit or runner destruction.
- Producer-policy tests verify retry of the recovered message after capacity
  drains and termination after cooperative cancellation or closed ingress.
- A fixed-height collection with one million logical rows constructs the same
  ten row elements and the same retained tree size as a 100-row collection at
  an eight-cell viewport with two-row overscan.
- Fixed range arithmetic is tested at a million rows; variable range tests cover
  cached height lookup and anchor preservation after a measurement changes.
- Application tests verify resize range recomputation, stable collection focus,
  selection surviving unmount and reorder by key, variable-height navigation,
  and deterministic multiline rendering.
- Criterion targets exercise fixed and variable visible-range lookup separately
  at 1,000, 100,000, and 1,000,000 logical rows.

One optimized Criterion run on 2026-07-17 measured fixed-height lookup at
approximately 6.6, 7.0, and 7.0 nanoseconds for those sizes. Variable-height
binary lookup measured approximately 17, 31, and 37 nanoseconds. These local
numbers establish algorithm shape for the prototype; they are not portable
end-to-end application claims.

A separately locked
[`Collection Lab Ratatui comparison`](../../../comparisons/collection-lab-ratatui/)
pins Ratatui 0.30.2 and uses the same providers, generated labels, stable keys,
measurements, overscan, dimensions, and action traces. It is excluded from the
product workspace so Ratatui does not enter the facade-only example graph. Its
six deterministic tests prove exact character-frame and semantic equivalence
for the canonical variable trace, stable identity through unmount and reverse,
ten constructed rows at one million fixed-height items, zero changed cells for
an explicit unchanged Ratatui redraw, and zero backend work when no redraw is
requested. Both test backends also report non-zero logical output for one-row
navigation.

The comparison requires Rust 1.88.0 because that is Ratatui 0.30.2's MSRV;
ArborUI remains pinned to Rust 1.85.0. One optimized run on 2026-07-17 under
Linux WSL2 on an Intel Core Ultra 7 255H measured these complete logical turns:

| Rows | Mode | ArborUI | Ratatui |
| ---: | --- | ---: | ---: |
| 1,000 | Fixed | 83.9 us | 9.50 us |
| 100,000 | Fixed | 82.1 us | 9.53 us |
| 1,000,000 | Fixed | 80.5 us | 12.4 us |
| 1,000 | Variable | 95.2 us | 11.8 us |
| 100,000 | Variable | 94.9 us | 11.2 us |
| 1,000,000 | Variable | 93.3 us | 11.4 us |

The million-row fixed Ratatui result had a wide 11.3-13.6 microsecond interval
and substantial outliers. Both sides remain approximately flat as logical item
count grows, confirming bounded virtualization. The roughly 6.5-8.8 times local
latency difference is not attributable to one isolated subsystem: ArborUI's
message-to-settled-frame path includes runtime settlement, retained
reconciliation, layout, hit geometry, and cloned test patches, while the matched
Ratatui application directly updates and redraws an immediate buffer. Production
ANSI bytes, allocation counts, and retained memory are not measured yet.

The widget unit tests independently verify checkbox activation and that a dialog
owns focus, handles Escape, and replaces lower pointer targets.

Run the focused evidence with:

```console
cargo test -p arborui-runtime --all-features
cargo test -p arborui-test --all-features
cargo test -p arborui-widgets --all-features
INSTA_UPDATE=no cargo test -p arborui-example-focus-queue --all-features
INSTA_UPDATE=no cargo test -p arborui-example-collection-lab --all-features
cargo bench -p arborui-example-collection-lab --bench visible_ranges -- --noplot
```

## Finding

The public boundary is sufficient for this modal form without application-level
focus flags or manual event broadcasting. The retained focus scope governs
keyboard traversal. Composited hit testing, explicit pointer-modal routing, and
captured-sequence suppression jointly govern pointer isolation.

The experiment also found a concrete composition constraint: the overlay host
must remain structurally stable while a dialog opens. Conditionally replacing
the application root with a stack removes the previously focused retained node,
so there is no identity to restore. Focus Queue now always renders the same stack
host and conditionally adds the keyed dialog layer. This preserves the
application subtree and restores the exact originating control.

The external-work slice required no runtime API change. `EventProxy` is
sufficient for an external producer to submit owned messages while application
updates remain serialized. Cancellation is necessarily cooperative at this
layer, and generation checks are still required because a producer may race the
cancel request with an already prepared item or terminal result.

`TestApp::settle` can deterministically drain messages that a controlled
producer has already submitted. It cannot infer whether an arbitrary external
producer will send more work later. Explicit application settlement state is
therefore part of the tested contract rather than an implication of visual idle.

The second screen also exposed a layout constraint. A nested screen container
with percentage height retained its content-derived minimum and could clip the
persistent navigation under a long log. Giving the screen a zero flex basis and
allowing it to grow within the root keeps the footer visible while the inner
scroll view clips the retained log.

The bounded-ingress slice found that replacing the channel alone would be
insufficient. Bulk transfer into the runner's internal message deque would free
producer slots before updates consumed those messages, allowing a sustained
producer to relocate an unbounded backlog behind the advertised capacity.
Keeping ingress as a separate queue until each message is selected preserves the
configured waiting bound. Fair alternation prevents an always-ready internal or
external source from monopolizing serialized updates.

Reject-new is the first policy because it preserves accepted FIFO entries and
requires no framework guess about message equivalence. It is observable overload
signalling, not lossless delivery, producer fairness, or automatic rate control.
Coalescing and replace-latest remain future opt-in policies for message classes
where intermediate values are demonstrably obsolete.

The collection slice confirms that clipping is not virtualization: bounded work
requires the application to avoid constructing off-window elements in the first
place. Stable model keys preserve logical active and selected state, but a row
that leaves the window intentionally loses retained `NodeId` identity. Making
the collection viewport the focus target prevents logical keyboard focus from
disappearing with such rows.

Fixed-height range discovery needs no framework change. Cached variable heights
also support logarithmic range lookup, although this prototype's simple prefix
array makes one changed measurement update a suffix in linear time. That is an
explicit prototype tradeoff rather than a claimed final data structure.

The main public-API gap is layout-resolved child construction. The experiment
must know its initial viewport height before `view`, then observe later terminal
resizes at the root. ArborUI should not promote a generic virtual collection
until another application demonstrates whether it needs a safe synchronous
deferred-child seam, a viewport-reporting contract, or only controlled
application sizing. Wrapped-row measurement remains a related open contract.

## Limits And Next Evidence

This slice does not complete the production-scale proof. It leaves these
requirements open:

- Select and table controls driven by application requirements
- Form validation and broader loading or error recovery
- A matched Ratatui-plus-application implementation
- Application-level measurements for code size, latency, allocations, emitted
  bytes, idle work, and retained memory
- Integration with a real service, subprocess, or async executor rather than the
  demonstration thread producer

The next application evidence should separate production ANSI bytes,
allocations, retained memory, and phase costs before selecting optimization
work. The matched logical comparison should add initial render, Page Down, End,
resize, selection, reverse, and explicit unchanged-redraw timing rather than
generalizing from alternating one-row navigation. Select and table requirements
can then extend the pilot without treating this local collection experiment as a
stabilized widget API.
