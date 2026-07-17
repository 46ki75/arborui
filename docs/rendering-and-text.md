# Rendering And Text

## Scope

This document defines the contracts between `arborui-text`, `arborui-render`, and
the terminal backend. The primary concern is correctness for Unicode text and
terminal cells before performance optimizations are introduced.

## Text Model

Terminal text has at least three distinct coordinate systems:

| Coordinate | Use |
| --- | --- |
| UTF-8 byte offset | Storage and interchange |
| Grapheme index | User-visible cursor and deletion behavior |
| Display column | Layout and terminal placement |

Public editing APIs should use typed positions instead of unlabelled integers.
Byte offsets may be exposed for interoperability, but they are not cursor
positions.

```rust
pub struct ByteOffset(pub usize);
pub struct GraphemeIndex(pub usize);
pub struct DisplayColumn(pub u32);
```

### Width Policy

Grapheme segmentation and terminal display width are related but separate.

```rust
pub enum WidthPolicy {
    Unicode,
    Cjk,
    WcWidth,
}
```

The active policy is selected by the terminal session and passed into text
measurement and frame construction. Every buffer being compared or composed
must use compatible width semantics.

`Unicode` uses sequence-aware width and keeps East Asian ambiguous characters
narrow. `Cjk` uses sequence-aware width and makes ambiguous characters wide.
`WcWidth` sums individual code point widths for compatibility with traditional
terminal behavior.

Environment overrides may be added for terminals whose behavior cannot be
detected reliably.

### Text Editing

`TextBuffer` eventually provides:

- Grapheme-aware insertion and deletion
- Horizontal and vertical movement
- Desired-column preservation during vertical movement
- Selection and range replacement
- Word-boundary movement
- Transactional undo and redo
- Styled ranges and highlights
- Viewport measurement

Undo history records logical operations and cursor state. Related edits can be
grouped into one transaction. The storage implementation may begin as a
`String` and later move to a rope if benchmarks justify it; the public editing
contract should not expose the storage choice.

## Grapheme Store

Cells reference complete UTF-8 grapheme strings through a renderer-owned
store. IDs are generation-protected or otherwise guaranteed not to alias a
stale value. Renderer-generated IDs remain associated with the same grapheme
text within that renderer's patch stream. Producers of manually constructed
patch streams must likewise preserve each ID-to-text mapping across patches.

Required properties:

- IDs remain valid while referenced by either frame.
- Arbitrary grapheme byte lengths are supported subject to memory limits.
- Equality can compare logical grapheme content across frames.
- Reference cleanup cannot invalidate a grapheme still used by a continuation cell.
- Malformed UTF-8 is rejected or replaced before entering the store.

## Cell Model

```rust
pub enum CellContent {
    Empty,
    Grapheme {
        id: GraphemeId,
        width: u8,
    },
    Continuation {
        id: GraphemeId,
        offset: u8,
    },
}

pub struct Cell {
    pub content: CellContent,
    pub style: Style,
    pub hyperlink: Option<HyperlinkId>,
}
```

Continuation cells carry matching visual metadata but are never emitted as
independent text.

### Cell Invariants

- A grapheme start is followed by exactly `width - 1` matching continuation
  cells.
- A continuation refers to the same grapheme as its start cell.
- Overwriting any cell in an old span clears the complete old span first.
- Clipping cannot leave a visible partial grapheme.
- A grapheme wider than the available line is replaced or omitted according to
  an explicit policy.
- Style and hyperlink comparisons cover every occupied cell.
- The last terminal column is handled without relying on autowrap.

These invariants should be checked with debug assertions and property tests.

## Buffer And Canvas

`Buffer` owns a rectangular cell grid and a compatible grapheme store. `Canvas`
is a clipped mutable view used by widgets and surfaces.

```rust
pub struct Buffer;

pub struct Canvas<'a> {
    buffer: &'a mut Buffer,
    clip: Rect,
    origin: Point,
}
```

Initial drawing operations include:

- Set grapheme
- Draw text
- Fill rectangle
- Draw horizontal or vertical line
- Draw border
- Apply style
- Blit another buffer

Drawing outside the clip is a no-op. Invalid geometry returns an error only
when it indicates an API misuse rather than ordinary clipping.

## Surfaces And Composition

A surface is an independently painted region with placement and clipping:

```rust
pub struct Surface {
    pub buffer: Buffer,
    pub position: Point,
    pub clip: Rect,
    pub z_index: i32,
    pub opacity: Opacity,
}
```

Surfaces support overlays, popups, scroll viewports, tooltips, and custom
drawing without requiring each widget to know the final paint order.

The first implementation may use opaque overwrite composition. The API should
leave room for transparent cells and color blending without requiring them in
the first milestone.

The compositor also produces a hit map containing the topmost interactive node
at each visible cell. Clipping and z-order therefore affect visuals and input
consistently.

## Frame Pipeline

```text
clear next frame
      |
paint root surfaces
      |
compose overlays and hit map
      |
validate wide-cell invariants
      |
compare committed and next frames
      |
produce FramePatch
      |
backend writes complete patch
      |
commit or invalidate
```

The renderer normally performs complete painting and a complete buffer scan. UI
preparation may reuse committed whole-frame geometry when reconciliation proves
that no layout-affecting change occurred. For paint-only work against the exact
committed renderer generation, it clones committed logical state, merges
overlapping or adjacent invalid row spans, clears each resulting full-width
region, and replays intersecting painters in normal order. Keeping each region
full width avoids cutting old or new wide grapheme spans; replaying every
intersecting layer preserves overlap and hit-map ordering. Clean rows between
separated invalidations remain untouched. Focus transitions mark both the
previous and current focus nodes for paint because their focused styles can
affect complete descendant content.

If reconciliation reports no change against that same generation, preparation
reuses the clone without invoking paint callbacks. Renderer mismatch, resize,
layout, or recomposition falls back to complete painting. `UiTree::prepare_full`
always computes layout and paints from scratch to provide the reference output
for optimized preparation. `Renderer::prepare_from_current` exposes the owned
clone-and-repaint transaction primitive; callers must clear changed regions and
replay every painter that can affect them.

Opt-in `UiPreparationTimings` and application-level `RenderTimings` report
`repaint_regions` and `repaint_cells`. Complete logical painting reports the
viewport as one region, unchanged committed-frame reuse reports zero, and
paint-only work reports the merged row regions actually cleared and replayed.
These deterministic work counters complement scheduler-sensitive phase
durations.

## Frame Patch

`FramePatch` describes terminal-independent changed runs:

```rust
pub struct CellRun {
    pub position: Point,
    pub cells: Vec<Cell>,
}

pub struct FramePatch {
    pub runs: Vec<CellRun>,
    pub cursor: CursorState,
    pub full_repaint: bool,
}
```

Wide grapheme spans are atomic within a `CellRun`. A `Grapheme` of width `n`
is immediately followed by `n - 1` `Continuation` cells in that same run. The
continuations have matching grapheme identity, style, and hyperlink metadata,
and offsets `1..n`; a run therefore never starts with a continuation. Backends
emit the leading grapheme once and skip those covered continuation cells.

`FramePatch::validate` checks this contract and run geometry for manually
constructed patches. Runs are globally row-major and non-overlapping. A full
repaint of a nonempty frame has exact coverage: one complete run for each row.
A zero-area full repaint needs no runs and remains an empty patch.

Renderer-generated patches are always valid, expanding a changed range to
include a complete wide span when necessary. Structural validation deliberately
does not inspect grapheme text, so logical replay through `apply_to` does not
need a terminal width policy. `FramePatch::validate_for_width_policy` adds the
backend preflight: each text value must be exactly one printable grapheme whose
measured width under the active policy matches its declared width, and one ID
cannot map to conflicting text values visible within that patch. This per-patch
check cannot detect an ID being reused for different text in separate manually
constructed patches; the stream producer must uphold cross-patch identity.
Renderers use their active policy for debug validation, and backends must use
their terminal capability policy before producing any output.

The final representation may borrow cells from a prepared frame to reduce
allocation. That optimization must not permit the renderer to mutate or free
the data before the backend finishes writing it.

Changed cells should be grouped into runs while considering:

- Cursor movement cost
- Style transition cost
- Hyperlink transitions
- Wide grapheme boundaries
- Clearing stale content
- Right-edge autowrap behavior

The backend decides how to encode the patch for a specific terminal.

## Prepared Frame Transaction

```rust
let prepared = renderer.prepare(viewport, |canvas| {
    ui.paint(canvas);
})?;

match backend.write_patch(prepared.patch())? {
    WriteOutcome::Applied => renderer.commit(prepared),
    WriteOutcome::Deferred => renderer.discard(prepared),
    WriteOutcome::StateUnknown => {
        renderer.discard(prepared);
        renderer.force_full_repaint();
    }
}
```

The committed buffer models what is believed to be physically visible. It is
updated only after complete output. Resume, resize, external output, or a
partial write invalidates that belief and forces a full repaint.

## Optimization Sequence

Optimizations should be considered in this order:

1. Avoid rendering while idle.
2. Coalesce multiple updates before a frame.
3. Reduce bytes with correct cell-run diffing.
4. Cache text measurement.
5. Reuse buffers and grapheme allocations.
6. Reuse clean whole-frame geometry, then skip clean layout subtrees.
7. Reuse unchanged committed logical state and repaint retained damaged rows.
8. Restrict diff scanning to damaged rows or regions.

Each optimization must produce the same logical frame and patch replay result
as the reference implementation.
