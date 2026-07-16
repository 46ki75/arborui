# Provisional ArborUI Position

Research baseline: 2026-07-16

## Position Statement

> ArborUI is for long-running, stateful Rust terminal applications that need
> coherent layout and interaction identity, serialized model updates,
> deterministic full-application tests, grapheme-aware logical rendering, and a
> recoverable boundary between prepared frames and accepted terminal output.
> Existing libraries often optimize for smaller rendering APIs, mature widget or
> language ecosystems, prompt and presentation workflows, host-runtime freedom,
> or native terminal capabilities. Those designs become expensive for an
> application when its requirements cross several extension boundaries at once:
> focus and pointer routing, effect settlement, large-data identity, terminal
> lifecycle, and output recovery. ArborUI accepts more framework policy, explicit
> invalidation, transaction state, and a smaller early ecosystem to provide joint
> UI/render commitment, backend-independent contracts, routed interaction, and a
> production-path test harness.

This position is provisional. The reports establish the compared project
contracts more strongly than they establish ArborUI's adoption value.

## What The Evidence Supports

### A Coherent Application Contract Is Distinct From A Renderer

Ratatui, tcell, notcurses, and Prompt Toolkit are strong at their chosen layers.
Applications can and do build successful frameworks above them. Their reports
support ArborUI's layering, but do not prove that every user wants the added
runtime policy.

ArborUI's relevant proposition is the integration of:

- Keyed identity and borrowed ephemeral view descriptions
- Layout, clipping, hit geometry, focus, capture, and event propagation
- Serialized messages, timers, futures, invalidation, and render scheduling
- Prepared renderer state and explicit backend outcomes
- A public harness that drives those production paths together

### Output Recovery Is A Real Architectural Difference

Across language and architecture families, renderers commonly advance logical
state before or without complete backend acceptance. Manual refresh, restart, or
fatal termination are normal mitigations. ArborUI's applied, deferred, and
unknown outcomes therefore address a real extension boundary.

The evidence does not show how frequently users need continued-session recovery
or whether they will accept the complexity. ArborUI should describe the guarantee
precisely without presenting other projects' fatal-error contracts as defects.

### Testing Is A Product Capability

Textual, Terminal.Gui, OpenTUI, iocraft, Ratatui, and Prompt Toolkit show that
test ergonomics shape application architecture. ArborUI's public `TestApp` is a
credible differentiator because it drives the real runtime and transaction path,
not just isolated widgets.

That claim must remain bounded. Headless logical frames do not prove ANSI
interpretation, raw-mode lifecycle, signal handling, partial writes, or physical
Unicode behavior. Emulator, fault, PTY, and compatibility layers remain required.

### Stable Identity Helps Interaction And Large Data, But Is Not Virtualization

Textual shows the coherence of retained geometry and interaction. iocraft shows a
Rust-compatible borrowed-description model. Ink and Bubble Tea show the maturity
benefits of familiar component ecosystems. tview shows the usefulness of a
specialized external data provider.

ArborUI's stable identity supports focus, capture, keyed state, and future
collections. It does not by itself make full layout or paint bounded. A visible-
range collection remains an experiment, not a completed differentiator.

### Logical Unicode Guarantees Need Physical Qualifications

Multiple projects independently support grapheme-bearing cells and explicit
width measurement. ArborUI's logical invariants are well motivated. No project,
including ArborUI, can infer universal physical agreement from width tables and
snapshots. Compatibility claims require selected terminal, multiplexer, font,
Unicode-version, and final-column evidence.

## What ArborUI Is Not

ArborUI is not currently intended to be:

- A replacement for bounded presentation libraries such as Rich, PTerm, or
  Spectre.Console
- A shell-composition tool like Gum
- A minimal terminal substrate like tcell or notcurses
- A React-compatible renderer like Ink
- A CSS-driven application platform like Textual
- A universal rich text editor or terminal multimedia framework
- One renderer contract spanning alternate screen, inline regions, and native
  scrollback

These boundaries are positioning choices, not judgments about the other projects.

## Costs ArborUI Accepts

- More framework policy than a rendering or terminal library
- Explicit invalidation and effect scheduling rules
- Retained identity and committed interaction geometry
- Prepared-frame transaction state and backend outcome handling
- Additional test layers for logical, fault, emulator, and native behavior
- A smaller widget and application ecosystem during early development
- The obligation to keep application, widget, backend, and test surfaces coherent

If these costs do not produce measurable application-level benefits, ArborUI
should narrow its guarantees rather than preserve complexity for its own sake.

## Claims Still To Prove

1. Transactional frame recovery prevents realistic failures often enough to
   matter to application authors.
2. Explicit invalidation remains understandable in a substantial application.
3. Borrowed ephemeral elements reduce Rust ownership friction versus retained
   durable views.
4. Full layout and painting remain practical, or can be optimized without losing
   a simple correctness oracle.
5. The public application harness materially improves downstream test quality.
6. Runtime and backend independence enables integrations users actually need.
7. Grapheme-level correctness produces visible compatibility improvements across
   supported terminals.
8. A focused core plus ecosystem widgets can compete with mature catalogs.
9. Full-screen-only high-level behavior is a sufficiently useful initial scope.

## Evidence Needed Next

The shortest path from research to a defensible product position is:

1. Build one substantial facade-only application with forms, modal interaction,
   streaming effects, dynamic keyed rows, Unicode editing, and deterministic
   tests.
2. Build the closest practical equivalent with Ratatui plus an application layer
   and compare code, ownership, tests, and failure handling.
3. Prototype fixed and variable-height visible-range collections.
4. Add systematic output-fault injection, a virtual-terminal oracle, and broader
   PTY lifecycle tests.
5. Measure frame latency, allocation, output bytes, idle work, and collection
   scaling before selecting incremental rendering architecture.
6. Review the results as architecture decisions, including negative findings and
   scope reductions.

The bounded application slices are recorded in the
[`Facade-Only Application Proof`](../experiments/application-proof.md). It
now establishes the modal form, second-screen state, external producer path,
cooperative cancellation, stale-result rejection, explicit settlement, and
bounded observable reject-new ingress, but does not complete item 1.

## Synthesis Index

- [`lesson-ledger.yaml`](lesson-ledger.yaml): atomic project observations and
  ArborUI implications
- [`capability-matrix.md`](capability-matrix.md): capability and extension
  boundaries by project layer
- [`testing-patterns.md`](testing-patterns.md): test layers, project patterns,
  and recommended additions
- [`failure-modes.md`](failure-modes.md): recurring mechanisms, workarounds, and
  mitigations
- [`arborui-recommendations.md`](arborui-recommendations.md): preserve, adopt,
  avoid, investigate, defer, and non-goal decisions
