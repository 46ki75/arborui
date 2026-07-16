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

This is an application comparison, not a general framework ranking. The shared
visible-range algorithm is application policy and cannot be attributed to either
framework.

## Commands

From the repository root:

```text
just comparison-check
just comparison-bench-smoke
just comparison-bench
```

Ratatui is pinned to 0.30.2, matching the research report dated 2026-07-16. The
comparison uses Rust 1.88.0 because that is Ratatui 0.30.2's MSRV; ArborUI's
product workspace remains pinned to Rust 1.85.0. Allocator and production
ANSI-byte adapters remain separate follow-up measurements because allocator
instrumentation perturbs latency and logical test backends do not exercise
production serialization.

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
