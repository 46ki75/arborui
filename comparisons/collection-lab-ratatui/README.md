# Collection Lab Ratatui Comparison

This package contains the matched Ratatui implementation for ArborUI's
Collection Lab experiment. It is excluded from the product workspace so Ratatui
does not become part of ArborUI's facade-only example dependency graph.

## Comparison Contract

Both implementations use the same application-owned visible-range providers,
generated labels, stable `u64` item keys, row measurements, overscan, viewport
dimensions, and action traces. The primary implementation constructs only the
visible and overscanned rows before painting.

The deterministic contract covers:

- Fixed and variable-height rendering at explicit terminal sizes
- Active and selected stable identity
- Home, End, line, page, selection, reverse, and resize actions
- Character-frame equivalence after a canonical variable-height trace
- Bounded construction for one million logical rows
- Explicit unchanged redraw as distinct from no requested draw

The timing benchmark measures a complete logical application turn: update,
visible-row construction, paint, and logical terminal diff. It excludes model
construction and terminal lifecycle. Alternating Down and Up keeps the measured
state bounded and avoids measuring an ever-growing scroll position.

The expanded action matrix uses 100,000 items and persistent fixtures. Untimed
actions restore a deterministic baseline between Page Down, End, resize,
selection, and reverse samples. Unchanged redraw sends Home while Home is
already active. Cold initial render is separate: it includes generated model
construction, logical terminal creation, and the first draw.

This is an application comparison, not a general framework ranking. The shared
visible-range algorithm is application policy and cannot be attributed to either
framework.

## Commands

From the repository root:

```text
just comparison-check
just comparison-bench-smoke
just comparison-bench
just comparison-output-metrics
just comparison-memory-metrics
just comparison-phase-metrics
```

Ratatui is pinned to 0.30.2, matching the research report dated 2026-07-16. The
comparison uses Rust 1.88.0 because that is Ratatui 0.30.2's MSRV; ArborUI's
product workspace remains pinned to Rust 1.85.0. Allocator, phase, latency, and
production ANSI probes remain separate because each instrumentation layer
changes the work being measured.

`comparison-output-metrics` passes real ArborUI patches and Ratatui buffer diffs
through each framework's Crossterm backend under fixed 48x12 ANSI16 conditions.
It reports bytes presented to the writer, writer callback counts, and flushes.
Writer callbacks are serializer operations, not operating-system syscall counts.
The resize case includes Ratatui's production clear before its full draw.

## First Local Result

One optimized run on 2026-07-17 used Rust 1.88.0 under Linux WSL2 on an Intel
Core Ultra 7 255H. Values below are Criterion point estimates for one alternating
Down or Up message through update, construction, paint, logical diff, and the
respective test backend.

| Rows | Mode | ArborUI | Ratatui |
| ---: | --- | ---: | ---: |
| 1,000 | Fixed | 83.9 us | 9.50 us |
| 100,000 | Fixed | 82.1 us | 9.53 us |
| 1,000,000 | Fixed | 80.5 us | 12.4 us |
| 1,000 | Variable | 95.2 us | 11.8 us |
| 100,000 | Variable | 94.9 us | 11.2 us |
| 1,000,000 | Variable | 93.3 us | 11.4 us |

The million-row fixed Ratatui sample had substantial outliers and a wide
11.3-13.6 us confidence interval. Both implementations remain approximately
flat as logical row count grows, which is the primary virtualization finding.
The latency difference is not an isolated renderer comparison: ArborUI includes
runtime settlement, retained reconciliation, layout, hit geometry, and cloned
test patches, while the Ratatui application directly updates and redraws its
immediate buffer.

## Expanded Local Result

The same machine produced these Criterion point estimates on 2026-07-17 for
100,000-item action cases. Cold initial render includes model construction;
other rows exclude untimed baseline resets.

| Scenario | ArborUI fixed | Ratatui fixed | ArborUI variable | Ratatui variable |
| --- | ---: | ---: | ---: | ---: |
| Cold initial render | 20.6 ms | 28.7 ms | 19.5 ms | 24.0 ms |
| Page Down | 118 us | 14.0 us | 143 us | 12.1 us |
| End | 97.7 us | 10.5 us | 103 us | 13.0 us |
| Resize 48x12 to 48x16 | 142 us | 20.0 us | 143 us | 22.9 us |
| Selection | 80.1 us | 9.49 us | 91.4 us | 11.8 us |
| Reverse | 817 us | 755 us | 839 us | 780 us |
| Unchanged redraw | 78.8 us | 8.60 us | 87.9 us | 11.7 us |

Cold initial render and fixed Page Down had wide intervals and substantial
outliers. Reverse is primarily the shared O(n) application policy: reversing
100,000 items and rebuilding providers. The other cases retain the same
framework-level work differences described above.

The production serializer probe reports `bytes/writer calls/flushes`:

| Scenario | ArborUI fixed | Ratatui fixed | ArborUI variable | Ratatui variable |
| --- | ---: | ---: | ---: | ---: |
| Initial render | 5265/3722/1 | 861/542/1 | 5259/3722/1 | 1047/689/1 |
| Page Down | 875/623/1 | 189/159/1 | 1243/905/1 | 249/216/1 |
| End | 1055/767/1 | 207/183/1 | 1095/797/1 | 247/213/1 |
| Resize | 7161/4986/1 | 1101/695/2 | 7155/4986/1 | 1407/926/2 |
| Selection | 785/588/1 | 157/132/1 | 1145/864/1 | 209/183/1 |
| Reverse | 875/623/1 | 189/159/1 | 899/641/1 | 213/177/1 |
| Unchanged redraw | 0/0/0 | 19/12/1 | 0/0/0 | 19/12/1 |

ArborUI's runtime suppresses an empty prepared patch before backend output.
Ratatui still invokes its production draw path for an empty diff, which emits
reset commands and flushes. These figures measure deterministic serialization,
not terminal-driver buffering or transport syscalls.

## Allocation And Retained Memory

`comparison-memory-metrics` runs every case in a separate release-mode process
using DHAT 0.3.3. The profiler starts immediately before the named operation,
then records total allocations, allocated bytes, peak live bytes, and bytes
still retained while the result is alive. Dropping every measured result
returned the tracked live block and byte counts to zero.

The model and initial-render cases deliberately use different boundaries.
`model` measures generated application data and providers. `initial-render`
constructs that model before profiling, so its retained bytes represent the
framework harness and first settled frame rather than the O(n) item model:

| Items | Model retained | ArborUI fixed | ArborUI variable | Ratatui fixed | Ratatui variable |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 148,987 | 97,484 | 92,988 | 82,944 | 82,944 |
| 100,000 | 14,899,987 | 97,484 | 92,988 | 82,944 | 82,944 |
| 1,000,000 | 148,999,987 | 97,484 | 92,988 | 82,944 | 82,944 |

Application-model memory scales linearly as expected. First-frame framework
memory is identical across all three logical collection sizes, which is the
memory-side bounded-virtualization result.

At 100,000 items, cells below are `allocated bytes/retained bytes` for each
isolated operation. Cold includes model construction; the other action fixtures
are constructed before profiling.

| Scenario | ArborUI fixed | Ratatui fixed | ArborUI variable | Ratatui variable |
| --- | ---: | ---: | ---: | ---: |
| Cold | 26,196,910/14,997,471 | 25,982,881/14,982,931 | 26,170,015/14,992,975 | 25,982,881/14,982,931 |
| Page Down | 122,177/44,884 | 0/0 | 106,354/42,772 | 0/0 |
| Resize | 302,653/123,428 | 165,888/165,888 | 267,462/118,988 | 165,888/165,888 |
| Reverse | 2,520,281/2,444,860 | 2,400,008/2,400,008 | 2,498,714/2,440,700 | 2,400,008/2,400,008 |
| Unchanged redraw | 101,873/39,892 | 0/0 | 78,978/35,492 | 0/0 |

## ArborUI Phase Attribution

ArborUI exposes opt-in timings for view construction, staged reconciliation,
layout, paint, diff, commit, post-commit refresh, and combined terminal backend
validation/serialization/write. Untimed rendering does not read the clock. The
headless comparison report averages 100 action samples and 20 initial-render
samples; initial render excludes model construction. Selected columns are shown
below in nanoseconds, while `comparison-phase-metrics` prints every phase.

| Mode | Scenario | Update | Stage/reconcile | Layout | Paint | Diff | Render total |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Fixed | Initial render | 0 | 13,140 | 53,427 | 47,233 | 24,832 | 159,096 |
| Fixed | Page Down | 418 | 3,834 | 25,047 | 34,439 | 4,365 | 75,990 |
| Fixed | End | 638 | 3,809 | 24,080 | 34,668 | 5,407 | 78,242 |
| Fixed | Resize | 2,319 | 3,830 | 26,879 | 41,185 | 15,141 | 98,014 |
| Fixed | Selection | 409 | 3,739 | 24,904 | 34,258 | 4,272 | 75,719 |
| Fixed | Reverse | 689,174 | 7,194 | 30,239 | 37,569 | 5,402 | 92,629 |
| Fixed | Unchanged redraw | 414 | 3,612 | 22,557 | 33,279 | 2,100 | 69,743 |
| Variable | Initial render | 0 | 12,062 | 54,826 | 55,012 | 13,338 | 157,143 |
| Variable | Page Down | 441 | 2,922 | 36,796 | 39,219 | 5,461 | 92,809 |
| Variable | End | 763 | 3,186 | 28,148 | 42,849 | 5,447 | 88,496 |
| Variable | Resize | 1,959 | 3,847 | 32,739 | 51,679 | 15,890 | 115,668 |
| Variable | Selection | 456 | 2,933 | 27,152 | 39,917 | 5,316 | 83,700 |
| Variable | Reverse | 807,145 | 9,328 | 44,099 | 53,865 | 8,053 | 132,019 |
| Variable | Unchanged redraw | 581 | 3,726 | 34,163 | 50,324 | 2,928 | 100,991 |

Paint and layout are the largest steady render phases in this application.
Reverse remains dominated by the application-owned O(n) update. Ratatui's
internal phase boundaries are not exposed, so its comparison remains the
complete-turn Criterion result rather than a fabricated phase split.
