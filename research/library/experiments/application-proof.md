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

Can matched ArborUI and Ratatui applications produce the same modal overlay
semantics and characters while exposing complete-turn, output, memory, and
ArborUI phase costs?

These are the first five bounded slices of the production-scale application
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
depth, high-water mark, accepted and dequeued totals, full rejections, cumulative
and maximum admission-to-dequeue latency, capacity, and closure. The runner
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

The fifth slice adds a matched overlay workload. Both sides keep the application
stack structurally stable, place an opaque scrim over the background, and center
a 26x7 confirmation dialog. Focus opens inside the dialog, forward traversal
wraps, and cancellation or confirmation restores the originating background
control. Covered pointer targets cannot activate. ArborUI receives real key
events through the runtime; because Ratatui is immediate-mode, its adapter
implements the same focus policy explicitly in application state.

The sixth slice adds matched Unicode-heavy rows with combining, CJK, ZWJ emoji,
flag, variation-selector, and ambiguous-width content. Controlled cell offsets
cut through a wide grapheme at the left viewport edge, while a separate update
replaces a width-two glyph with width-one ASCII. Both adapters share the model
and require complete-grapheme clipping.

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
- A capacity-one facade test fills ingress with the final item, recovers the
  rejected completion, and proves completion cannot overtake that item.
- Runtime tests verify ownership recovery, clone-shared capacity, FIFO delivery,
  capacity release, high-water and rejection metrics, exact manual-clock queue
  latency, wake behavior, and closure after quit or runner destruction.
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
- Matched overlay tests prove focus trapping, wrapping, restoration, and pointer
  isolation with exact character and semantic parity at 40x12 normally and
  44x14 after resizing while open.
- Matched Unicode tests prove exact character and semantic parity through a
  boundary-cutting shift, wide-to-narrow replacement, and narrow and wide
  resizes. Public facade tests inspect the leading and continuation cells after
  clipping.

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
allocation counts and retained memory are measured separately below.

The matched benchmark now also isolates cold initial render, Page Down, End,
resize, selection, reverse, and unchanged redraw at 100,000 items. One optimized
run on the same machine produced these point estimates:

| Scenario | ArborUI fixed | Ratatui fixed | ArborUI variable | Ratatui variable |
| --- | ---: | ---: | ---: | ---: |
| Cold initial render | 20.6 ms | 28.7 ms | 19.5 ms | 24.0 ms |
| Page Down | 118 us | 14.0 us | 143 us | 12.1 us |
| End | 97.7 us | 10.5 us | 103 us | 13.0 us |
| Resize 48x12 to 48x16 | 142 us | 20.0 us | 143 us | 22.9 us |
| Selection | 80.1 us | 9.49 us | 91.4 us | 11.8 us |
| Reverse | 817 us | 755 us | 839 us | 780 us |
| Unchanged redraw | 78.8 us | 8.60 us | 87.9 us | 11.7 us |

Cold initial render includes model generation, harness creation, and first draw.
The other cases use persistent fixtures with untimed deterministic resets. Cold
initial render and fixed Page Down had wide intervals and substantial outliers.
Reverse mainly measures shared O(n) application policy because both sides reverse
100,000 items and rebuild their providers.

The production-output probe passes real ArborUI patches and Ratatui buffer diffs
through their Crossterm backends under fixed 48x12 ANSI16 conditions. The cells
below are `bytes/writer calls/flushes`; writer calls are serializer callbacks,
not operating-system syscalls.

| Scenario | ArborUI fixed | Ratatui fixed | ArborUI variable | Ratatui variable |
| --- | ---: | ---: | ---: | ---: |
| Initial render | 5265/3722/1 | 861/542/1 | 5259/3722/1 | 1047/689/1 |
| Page Down | 875/623/1 | 189/159/1 | 1243/905/1 | 249/216/1 |
| End | 1055/767/1 | 207/183/1 | 1095/797/1 | 247/213/1 |
| Resize | 7161/4986/1 | 1101/695/2 | 7155/4986/1 | 1407/926/2 |
| Selection | 785/588/1 | 157/132/1 | 1145/864/1 | 209/183/1 |
| Reverse | 875/623/1 | 189/159/1 | 899/641/1 | 213/177/1 |
| Unchanged redraw | 0/0/0 | 19/12/1 | 0/0/0 | 19/12/1 |

Ratatui resize includes its production clear and therefore two flushes. ArborUI
suppresses empty prepared patches before backend output; Ratatui's empty diff
still emits 19 bytes of reset commands and flushes. Transport-level buffering,
allocations, retained memory, and phase costs use separate probes so their
instrumentation does not perturb the latency or serializer results above.

The allocation probe runs one release-mode DHAT process per case. It separates
the O(n) generated model from first-frame framework state. Model retained bytes
grow from 148,987 at 1,000 items to 148,999,987 at one million items. In
contrast, first-render retained bytes remain exactly 97,484 for ArborUI fixed,
92,988 for ArborUI variable, and 82,944 for both Ratatui modes at every tested
item count. All tracked live allocations return to zero after dropping each
measured result.

At 100,000 fixed-height items, Page Down allocates and retains 122,177/44,884
bytes in ArborUI and 0/0 in Ratatui. After retained-layout reuse, unchanged
redraw allocates and retains 56,648/39,892 bytes in ArborUI and 0/0 in Ratatui.
Damaged-row selection allocates and retains 73,621/44,692 bytes versus 0/0.
Resize is 302,653/123,428 versus 165,888/165,888 bytes, while reverse is
2,520,281/2,444,860 versus 2,400,008/2,400,008 bytes. Variable-height selection
is 73,054/42,532, while unchanged redraw allocates and retains 47,880/35,492
bytes. These are operation-local allocations; fixture allocations made before
profiling are intentionally excluded.

Opt-in ArborUI instrumentation now separates application view construction,
staged reconciliation, layout, paint, diff, commit, post-commit refresh, and
combined terminal validation/serialization/write. Existing untimed methods do
not read the clock, and the transactional write-before-commit ordering is
unchanged. The first retained-layout measurement removed layout time from fixed
and variable selection and unchanged redraw. The subsequent optimization reuses
the exact committed logical frame when reconciliation reports no change,
reducing fixed and variable unchanged-redraw render totals to 14.7 and 16.8
microseconds in that run. Owned buffer, hit-map, and grapheme state are still
cloned so preparation remains transactional. A renderer generation mismatch,
including physical-state invalidation, returns to complete painting.
Layout-required Page Down, resize, and reverse turns continue through complete
layout. Ratatui does not expose equivalent internal boundaries, so only its
complete-turn timings are compared.

The damaged-row optimization clones the same owned committed state for
paint-only work, clears one full-width vertical band covering paint-invalid
nodes, and replays only painters intersecting that band in normal order. The
full-width boundary preserves wide-grapheme atomicity, while ordered replay
preserves overlap and hit-map semantics. Fixed selection paint fell from 33.4 to
13.1 microseconds and the complete render total from 50.0 to 33.1. Variable
selection paint fell from 43.1 to 18.7 microseconds and the render total from
62.4 to 38.3. Complete Criterion turns measured 33.3 and 38.4 microseconds,
about 40% below the preceding documented fixed and variable selection results.
Selection serialization remains unchanged. The full gate exposed that focus
transitions must mark both previous and current focus nodes as damaged even
after transition reporting consumes the event metadata; the retained
invalidation now carries that dependency.

The finer-grained damage slice adds deterministic repaint-region and repaint-cell
counters to the opt-in timings. Its facade-only Collection Lab probe starts with
row zero selected, batches active movement to visible row seven, then moves the
selection to that row. The original contiguous band reported one region and 384
cells for each turn; one initial optimized local sample spent 26.866 and 26.711
microseconds painting and 50.793 and 47.694 microseconds on the complete render.
Those two separated invalid rows select a narrower vertical strategy without
selecting horizontal clipping: merging only overlapping or adjacent row spans
preserves full-width wide-grapheme safety and leaves two regions totaling 96
cells, a deterministic 75% area reduction.

An optimized 100-sample run on 2026-07-17 reported:

| Scenario | Repaint regions | Repaint cells | Paint average | Render average |
| --- | ---: | ---: | ---: | ---: |
| Active-row move | 2 | 96 | 11.030 us | 30.743 us |
| Selection move | 2 | 96 | 10.652 us | 28.774 us |

The before timing samples and optimized averages are observational rather than a
formal paired benchmark; the deterministic area change is the selection signal.
The existing matched fixed/variable selection phase probe remained healthy at
11.515 and 16.507 microseconds of paint in the same optimized validation run.
Focused callback counts prove that clean intervening rows are skipped, and the
incremental/full-reference oracle compares complete buffers, patches, hit maps,
retained geometry, and committed renderer state across separated style changes
including wide content.

The next cross-workload slice adds a facade-only virtualized service table. A
shared application model owns fixed-height range construction, responsive column
widths, stable active and selected keys, Unicode region data, and deterministic
producer updates. ArborUI composes only visible and overscanned rows from public
facade primitives; the matched side renders the same window through Ratatui
0.30.2's stateful `Table`. Exact semantic and character-frame comparisons cover
page navigation, selection, visible and offscreen updates, and narrow and wide
resizes. Construction remains bounded through one million logical rows.

At 100,000 rows, complete Criterion turns measure 189 versus 204 microseconds for
Page Down, 55.4 versus 215 microseconds for selection, 279 versus 223 microseconds
for resize, 181 versus 190 microseconds for a visible producer update, and 32.5
versus 195 microseconds for an offscreen producer update, with ArborUI listed
first. ArborUI line navigation remains approximately 55 microseconds from 1,000
through one million rows; Ratatui remains approximately 198 to 206 microseconds.
Cold construction and initial render measure 11.8 versus 12.3 milliseconds.

The offscreen update is the strongest incremental result: it changes retained
application data but not the visible projection, takes 34.8 microseconds in the
ArborUI phase probe, performs no layout, spends 1.4 microseconds painting, and
emits no production output. The visible update changes text, takes 166.0
microseconds in the render phases, and emits 102 bytes through 68 writer calls.
This distinction validates committed-frame reuse across a second workload while
showing that visible text changes still pass through complete table-row layout.
The deterministic update intentionally excludes thread scheduling and ingress
latency, which Focus Queue measures separately.

The completed overlay workload measures cold initial, open, focus-next, cancel,
confirm, background activation, and resize-open turns. One optimized local run
on 2026-07-17 produced these Criterion point estimates and ranges:

| Scenario | ArborUI | Ratatui |
| --- | ---: | ---: |
| Cold initial | 82.9 us (82.70-83.17) | 17.2 us (17.16-17.31) |
| Open | 117 us (116.0-118.2) | 19.30 us (19.24-19.35) |
| Focus next | 33.7 us (33.24-34.13) | 13.1 us (12.77-13.39) |
| Cancel | 83.2 us (82.07-84.39) | 10.62 us (10.55-10.68) |
| Confirm | 80.05 us (79.85-80.26) | 11.49 us (11.45-11.52) |
| Background activation | 10.27 us (10.19-10.34) | 6.09 us (6.06-6.13) |
| Resize while open | 123.2 us (122.8-123.6) | 26.87 us (26.76-26.98) |

The production serializer probe reports `bytes/writer calls/flushes` as
4512/3171/1 versus 701/428/1 initially, 4622/3242/1 versus 1321/845/1 on open,
226/152/1 versus 19/12/1 on focus-next, and 4512/3171/1 versus 955/668/1 for
both cancel and confirm. Background activation is 0/0/0 versus 19/12/1, and
resize-open is 5792/4058/1 versus 1533/1018/2. Writer calls are serializer
callbacks, not operating-system syscalls.

The overlay model allocates zero bytes on both sides. Initial retained memory is
79,092 bytes for ArborUI and 69,120 for Ratatui. ArborUI action-scoped retained
allocations are 58,828 bytes for open, 36,260 for focus-next, 58,860 for cancel
and confirm, 30,828 for background activation, and 101,348 for resize-open;
Ratatui retains zero for the non-resize actions and 138,240 bytes for its resized
double buffer. Action rows are incremental allocations retained at sample
capture after building the baseline, not total process memory.

ArborUI phase attribution in `update/view/stage/layout/paint/diff/commit/post`
order is 0/1502/2331/25289/30664/11936/4420/861 ns initially,
455/2770/4445/48096/42630/13272/4445/1579 ns on open, and
3681/2199/3926/0/15873/2790/2906/1337 ns on focus-next. Cancel is
2899/1519/3209/23765/28278/13330/5357/920 ns, confirm is
3056/1376/3247/24812/28919/13246/5500/885 ns, background activation is
309/1049/1897/0/987/1878/3395/638 ns, and resize-open is
2652/2495/5383/43649/48179/13670/4724/1436 ns. Their measured totals are
77,772, 118,736, 31,384, 77,110, 78,697, 11,118, and 120,647 ns respectively.

The completed Unicode workload measures cold initial, a shift from offset 15 to
16 that cuts the first CJK grapheme, wide-to-narrow replacement, and resize from
36x10 to 30x10. Criterion measured ArborUI/Ratatui at 106.2/24.48 microseconds
initially, 74.79/13.62 for the shift, 68.78/18.80 for replacement, and
76.65/15.52 for resize. Ratatui replacement varied widely from 12.78 to 27.36
microseconds, so its point estimate is not treated as precise.

Production output in `bytes/writer calls/flushes` is 3307/2340/1 versus
1016/664/1 initially, 737/580/1 versus 196/166/1 for the boundary shift,
125/88/1 versus 44/33/1 for replacement, and 2797/1980/1 versus 947/624/2 for
resize. The shared model retains 396 bytes on both sides. Initial-render retained
framework memory is 62,268 bytes for ArborUI and 51,840 for Ratatui.

ArborUI Unicode phase totals are 106,794 ns initially, 81,016 for the boundary
shift, 82,964 for replacement, and 90,305 for resize. Paint is the largest named
phase in every case, but all three action turns also perform layout.

The completed resize-storm slice applies eight complete alternating narrow/wide
and short/tall resize turns before returning to each workload's base size. Exact
semantic and character parity is checked after every intermediate frame for
fixed and variable collections, responsive table, paused log, open overlay, and
Unicode clipping. Optimized ArborUI/Ratatui storm totals are 877/132 microseconds
for fixed collection, 902/151 for variable collection, 1.717/1.722 milliseconds
for table, 1.328 milliseconds/180 microseconds for paused log, 914/185
microseconds for open overlay, and 718/134 microseconds for Unicode.

Every ArborUI resize is an accepted full repaint. Aggregate production bytes
range from 30,556 to 46,777 for ArborUI and 7,202 to 10,908 for Ratatui across
the six cases. ArborUI uses eight flushes; Ratatui's measured production clear
plus full draw uses sixteen. ArborUI's complete-storm phase totals range from
698,620 ns for Unicode to 1,503,069 ns for table. Layout dominates the
table, paint dominates collection and Unicode, and layout and paint are close
for the paused log and overlay. This does not identify one workload-independent
local optimization.

The live-ingress slice timestamps accepted proxy entries with a private monotonic
ingress clock and records latency when the runner removes each entry immediately
before application update. A manual-clock runtime test proves exact cumulative
and maximum values without sleeping. Focus Queue separately runs a real producer
thread through the public facade with 1,024 items plus completion, a forced first
`Full`, recovered-message retry, complete headless rendering, and semantic
settlement. One optimized run on 2026-07-17 reported:

| Capacity | Accepted | Full rejections | Average queue latency | Max queue latency | Complete burst | Drain turns | Frames |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1025 | 1060 | 24.946 us | 591.507 us | 130.671 ms | 1063 | 1021 |
| 8 | 1025 | 128 | 19.112 us | 315.829 us | 15.333 ms | 139 | 127 |
| 64 | 1025 | 12 | 66.494 us | 360.219 us | 2.799 ms | 19 | 14 |

The probe deliberately uses a 50-microsecond retry delay rather than Focus
Queue's human-scale 25-millisecond demonstration delay. Scheduling, rejection,
turn, and frame counts are observational rather than regression thresholds. All
capacities reach their exact bound, retain only 32 application items, dequeue
all 1,025 accepted messages, and preserve completion ordering. Larger capacity
reduces retry delay and total burst time in this run; per-message queue latency
is scheduler-sensitive and not monotonic by capacity. Repeated invalidations
still coalesce substantially, most clearly at capacity 64, where 1,025 updates
commit 14 frames. This evidence selects no new local renderer optimization;
slow terminal polling and input-to-write-complete latency remain separate open
measurements.

The slow-sink slice keeps the public application and runtime boundary, uses the
production Crossterm patch serializer over an instrumented writer, and applies a
fixed delay once per successful nonempty flush. Each of 64 generation-tagged
Focus Queue items is admitted and rendered separately so one input has one
unambiguous output completion. Input-to-write latency begins immediately before
the successful `EventProxy::send` call and ends when the writer's flush
completes; it intentionally excludes transactional commit and post-commit
refresh. One optimized run on 2026-07-17 reported:

| Sink delay | Input-to-write average | p50 | p95 | Maximum | Queue average | Update average | Render average | Instrumented serialize/write average | ANSI bytes | Writer calls | Flushes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 ms | 0.176 ms | 0.171 ms | 0.260 ms | 0.364 ms | 0.298 us | 0.408 us | 0.193 ms | 0.004 ms | 14302 | 9700 | 64 |
| 1 ms | 1.256 ms | 1.252 ms | 1.318 ms | 1.332 ms | 0.628 us | 0.859 us | 1.287 ms | 1.064 ms | 14302 | 9700 | 64 |
| 5 ms | 5.298 ms | 5.288 ms | 5.369 ms | 5.423 ms | 1.147 us | 1.508 us | 5.338 ms | 5.088 ms | 14302 | 9700 | 64 |

The full report also records view construction, reconciliation, layout, paint,
and diff. All 192 measured updates produce one accepted patch and flush, no full
repaint, and identical ANSI volume at each delay. The imposed sink delay transfers
almost one-for-one into write-complete and complete-render latency while queue
and update time remain in the microsecond range. The instrumented writer locks
shared counters on each serializer callback, so its baseline is not an
unmodified production-writer cost; those callbacks are not operating-system
calls. The scheduled benchmark workflow enforces ceilings of 15,000 ANSI bytes
and 10,000 serializer callbacks for the 64-update trace, plus latency overhead
above the imposed sink delay of 5 milliseconds average, 10 milliseconds at
p95, and 50 milliseconds for any one sample. These deliberately broad timing
limits detect sustained, repeated-tail, or extreme one-sample regressions on the
scheduled GitHub-hosted runner image without treating scheduler-sensitive
measurements as portable performance guarantees. Normal CI enforces the output
ceilings without applying wall-clock limits. The deterministic companion test
blocks one flush, fills the exact two-message ingress capacity from another
thread, observes `Full` with ownership preserved, then applies the recovered
message in FIFO order. Existing runtime transaction tests separately prove that
only a complete `Applied` write commits and that uncertain output forces a full
repaint. The slow-sink evidence therefore selects no new renderer optimization;
the synchronous sink itself is the dominant boundary under imposed delay.

`UiTree::prepare_full` preserves a separately callable complete-layout reference.
The incremental path is checked against it across hand-selected and deterministic
generated transitions, comparing patches, complete buffers, hit maps, retained
geometry, and committed renderer state. This evidence also exposed and fixed a
latent keyed-child reorder invalidation gap that complete layout had masked.

The widget unit tests independently verify checkbox activation and that a dialog
owns focus, handles Escape, and replaces lower pointer targets.

Run the focused evidence with:

```console
cargo test -p arborui-runtime --all-features
cargo test -p arborui-test --all-features
cargo test -p arborui-widgets --all-features
INSTA_UPDATE=no cargo test -p arborui-example-focus-queue --all-features
just focus-queue-ingress-metrics
just focus-queue-slow-sink-metrics
INSTA_UPDATE=no cargo test -p arborui-example-collection-lab --all-features
just collection-lab-damage-metrics
cargo bench -p arborui-example-collection-lab --bench visible_ranges -- --noplot
just comparison-memory-metrics
just comparison-phase-metrics
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

The external-work slice required no transport-policy change. `EventProxy` is
sufficient for an external producer to submit owned messages while application
updates remain serialized. The live-ingress evidence extends its shared metrics
but retains reject-new admission and recovered-message ownership. Cancellation
is necessarily cooperative at this layer, and generation checks are still
required because a producer may race the cancel request with an already prepared
item or terminal result.

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

The overlay evidence shows that structural overlay turns expose layout, paint,
and serialization costs, while focus movement and unchanged background
activation use no-layout paths. No optimization is selected yet; the remaining
workloads must establish whether these costs generalize.

The Unicode evidence confirms atomic clipping and wide-to-narrow cleanup through
the public application boundary. Its active turns show the same layout-and-paint
shape as structural overlay work, with paint dominant in the phase report. This
closes the planned Unicode-heavy evidence slice without changing the renderer;
the matched resize storm establishes repeated resize churn, and the live-ingress
probe now measures queue latency under a bounded-depth threaded burst. The
slow-sink probe closes the corresponding admission-to-successful-flush evidence
without identifying a local renderer bottleneck.

## Limits And Next Evidence

This slice does not complete the production-scale proof. It leaves these
requirements open:

- Select and reusable table controls driven by application requirements
- Form validation and broader loading or error recovery
- Application-level code-size measurement and broader memory workloads
- Integration with a real service, subprocess, or async executor rather than the
  demonstration thread producer

The measured incremental path now reuses retained whole-frame geometry when no
layout-affecting change occurred, committed logical content when no change at all
occurred, and merged full-width damaged-row regions for paint-only work. The table
slice broadens evidence to responsive columns, Unicode cells, resize, and
deterministic background updates without stabilizing a widget API. The bounded
scrolling-log slice adds chronological history, follow-tail behavior, paused
viewport anchoring through eviction, deterministic append batches, and flat
construction through one million records. Paused append skips ArborUI layout
and backend output, while active scrolling remains substantially faster in the
direct Ratatui adapter. The matched overlay slice adds exact modal character and
semantic parity plus latency, output, memory, and phase evidence. The matched
Unicode slice adds combining, wide, joined, flag, variation-selector, and
ambiguous content at clipping boundaries. The matched resize storms show that
layout and paint weight varies by workload.
Live ingress now establishes queue-latency and backpressure costs under the
measured burst. Slow-sink evidence establishes that imposed synchronous flush
delay transfers directly into input-to-write completion while pressure remains
bounded and observable. Neither slice selects another local renderer
optimization. Tracked output and slow-sink regression limits now run in
scheduled CI. The finer-grained damage slice closes Milestone 12; select and
reusable table requirements can extend the pilot separately.
