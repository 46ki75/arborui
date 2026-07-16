# TUI Library Research Strategy

## Purpose

This research examines prominent terminal UI libraries to understand:

- The applications and users each project serves best
- The project's central architectural choices and strengths
- The limitations, tradeoffs, and extension boundaries that frustrate users
- How maintainers and users mitigate those limitations
- Which testing strategies make terminal behavior reproducible and trustworthy
- Which lessons should influence ArborUI's design, priorities, and positioning

The goal is not to rank every project on a single scale. A library can be an
excellent solution for its intended use while being unsuitable for a different
application model. The useful question is:

> What requirement falls below a library's extension boundary, forcing users to
> work around, fork, or replace it?

## Research Principles

### Compare Requirements, Not Popularity

Evaluate a design relative to explicit application requirements. Stars,
download counts, issue counts, and age may provide ecosystem context, but they
do not establish architectural quality or suitability.

### Distinguish Kinds Of Problems

Classify each negative finding before drawing a lesson:

| Classification | Meaning |
| --- | --- |
| Bug | The implementation violates its intended contract |
| Limitation | A requirement is intentionally unsupported |
| Tradeoff | A choice benefits one use case while making another harder |
| Extension failure | Required behavior cannot be changed cleanly outside the framework |
| Historical issue | The problem applied to an older version but has since changed |
| Maturity problem | The architecture permits the feature, but the implementation or ecosystem does not provide it |
| Governance problem | Maintenance, review, ownership, or release authority is the limiting factor |

Do not present an architectural preference as a defect without connecting it to
a concrete requirement and observable cost.

### Study The Workaround

A limitation with a small application-level workaround is different from one
that requires replacing rendering, scheduling, input parsing, or terminal
lifecycle management. Record both the workaround and its cost.

### Separate Current And Historical Behavior

Record the version, revision, and date examined. An old migration report can
explain an important architectural failure, but it must not be presented as a
claim about the current release without verification.

### Challenge ArborUI's Assumptions

The research should try to falsify ArborUI's design hypotheses, not merely find
evidence that supports them. Architectural rigor has a cost, and the reports
should determine whether that cost produces benefits visible to application
authors.

## Scope Classification

The inventory contains projects at different abstraction levels. Classify each
project before comparing it with ArborUI.

| Category | Typical responsibilities | Examples |
| --- | --- | --- |
| Terminal substrate | Input decoding, output, capabilities, lifecycle | tcell, notcurses |
| Rendering or widget library | Buffers, layout, widgets, frame rendering | Ratatui, FTXUI, tview, gocui |
| Application framework | State, events, scheduling, rendering, testing | Textual, Bubble Tea, Ink, OpenTUI, iocraft, Terminal.Gui |
| Presentation or CLI toolkit | Formatting and bounded interactive prompts | Rich, PTerm, Spectre.Console, gum |

A project may receive more than one tag. The classification determines which
comparison dimensions are relevant; a presentation toolkit should not be
criticized for intentionally lacking a full application runtime.

## ArborUI Research Scope

This section defines the working product scope against which findings should be
interpreted. It reflects ArborUI's current implementation and documented goals;
it is not a promise that every future mode will share one rendering contract.

### Primary Application Profile

ArborUI primarily targets long-running, stateful, interactive terminal
applications that need:

- A full-screen viewport owned by the application
- Structured layout and reusable widgets
- Keyboard, focus, mouse, overlay, and text-input interaction
- Serialized model updates with asynchronous effects
- Correct Unicode text measurement and rendering
- Deterministic headless application testing
- Reliable terminal restoration and recovery after uncertain output

Representative applications include dashboards, forms, administrative tools,
interactive development tools, and text-heavy applications with dynamic
content. These are the main scenarios for direct comparisons.

### Current Screen-Mode Baseline

The implemented high-level runtime owns and repaints a complete viewport in the
alternate screen. This is the baseline for evaluating rendering, interaction,
scheduling, lifecycle, and testing. A library should not receive credit for an
inline or native-scrollback feature as though it solved the same ownership
problem; each mode must be evaluated under its own contract.

### Future Modes To Investigate

ArborUI's architecture intends to leave room for additional output contexts,
but their ownership and recovery semantics are not yet stabilized:

- Inline regions on the main screen
- Applications that preserve native terminal scrollback
- Remote terminal transports
- Embedded backends
- Headless rendering and application testing

Research should identify proven contracts and failure modes for these contexts,
especially scrolling, resize recovery, immutable history, cursor ownership,
external output, suspension, and physical-screen resynchronization. Findings
may result in separate explicit modes rather than a configurable variant of the
full-screen renderer.

### Secondary And Adjacent Use Cases

Presentation toolkits, progress displays, short-lived prompts, and shell helpers
are adjacent rather than direct competitors. Study them for focused lessons in
formatting, composability, terminal degradation, and ergonomics, but do not use
their narrower scope as evidence against a full application framework.

Rich text editors, image or sixel rendering, foreign-language bindings, CSS
cascading, and a mandatory async runtime are not current ArborUI goals. They may
expose useful subsystem techniques, but they should not drive the comparative
verdict.

### Scope Interpretation

Alternate-screen, inline-region, and native-scrollback applications have
fundamentally different ownership and recovery semantics. Every report should
state which mode an observation concerns and whether it applies to ArborUI's
current baseline, a future mode, or only an adjacent use case.

## Comparison Dimensions

Every substantial report should investigate the applicable dimensions below.

| Dimension | Questions |
| --- | --- |
| Intended use | What kind of application and developer is the project optimized for? |
| Core proposition | What does it make unusually easy, reliable, or expressive? |
| Application model | Is it immediate-mode, retained, Elm-style, React-style, callback-driven, or another model? |
| State ownership | Where do application, widget, focus, layout, renderer, and terminal states live? |
| Rendering | Does it use full rendering, line or cell diffing, damage tracking, or a retained scene? |
| Layout and composition | How are reusable components, overlays, clipping, scrolling, and large collections expressed? |
| Terminal ownership | Does it use the alternate screen, inline regions, native scrollback, or explicit separate modes? |
| Text correctness | How does it handle graphemes, wide cells, ambiguous width, combining marks, and the final column? |
| Interaction | How are focus, event propagation, mouse capture, paste, and keyboard protocols handled? |
| Scheduling | How are timers, asynchronous work, external events, cancellation, idle work, and backpressure handled? |
| Lifecycle | What happens on errors, panic, suspension, signals, resize, partial writes, and restoration? |
| Extension boundary | What can application, widget, backend, and integration authors replace or customize? |
| Performance | What work scales with tree size, frame size, update rate, output volume, or session length? |
| Ergonomics | What boilerplate, diagnostics, composition rules, or language constraints affect users? |
| Maturity | How complete are widgets, documentation, releases, governance, and compatibility policies? |
| Testability | What can be tested deterministically, and which production paths are exercised? |

Use categorical conclusions such as supported, partial, unsupported, and
unknown. Avoid a single aggregate score that hides incompatible priorities.

## Common Evaluation Scenarios

Use a shared set of scenarios to make reports comparable:

- A form with focus traversal, validation, and a modal
- A large scrollable collection with stable item identity
- Streaming output driven by external events
- Unicode-heavy text input and editing
- An overlay with clipping and mouse interaction
- Resize during active updates
- Deferred, partial, or failed terminal output
- Suspension to a child process followed by resume
- An application that remains idle for long periods
- A conversation that preserves native terminal scrollback

Documentation and existing tests should be examined first. Implement a small
prototype only when an important architectural or ergonomic claim remains
unclear.

## Report Depth

Use different levels of detail so that effort follows relevance.

| Tier | Expected detail | Approximate length | Purpose |
| --- | --- | ---: | --- |
| Deep dive | Architecture, source and test inspection, failure cases, application evidence, and ArborUI implications | 3,000-6,000 words | Direct competitors and important architectural alternatives |
| Standard profile | Main design, strengths, limitations, testing approach, and lessons | 1,200-2,500 words | Relevant libraries with narrower lessons |
| Brief survey | Scope, distinguishing idea, one important limitation, testing observation, and references | 400-800 words | Adjacent tools and weak comparisons |

These lengths are guardrails rather than quotas. Stop when the conclusions are
supported by sufficient evidence. Promote a project when it reveals an unusual
testing strategy, documented migration, important failure mode, or relevant
extension boundary.

### Initial Tier Assignment

Deep dives:

- Ratatui
- iocraft
- Bubble Tea
- Textual
- Ink
- OpenTUI

Standard profiles:

- FTXUI
- Terminal.Gui
- Python Prompt Toolkit
- tcell
- notcurses
- tview
- blessed

Brief surveys:

- Rich
- PTerm
- Spectre.Console
- gum
- gocui
- pytermgui

The initial inventory and first-pass evidence may justify changing these tiers.

## Substantial Report Template

### Project Snapshot

Target length: 100-200 words.

Record:

- Language and implementation technologies
- Project category and intended applications
- Whether the project presents itself as a library, framework, toolkit, or backend
- Version or revision examined
- Maintenance and release status
- Important ecosystem context

### Core Proposition

Target length: 150-300 words.

Explain:

- The problem the project is designed to solve
- Its central abstraction or developer experience
- Its strongest use case
- How it differs from lower-level terminal or formatting libraries

### Architecture

Target length: 300-700 words.

Cover:

- Application and state model
- Rendering and layout model
- Events, effects, and scheduling
- Terminal ownership and lifecycle
- Public extension points

Use diagrams or short code examples when they reveal a contract more clearly
than prose.

### Core Strengths

Target length: 300-600 words.

Identify three to five consequential strengths. Support each with architecture,
implementation, tests, or evidence from substantial applications rather than
marketing claims alone.

### Limitations And Frustrations

Target length: 500-1,000 words.

For each important finding, record:

```text
Classification:
Requirement:
Library assumption:
Observable failure or friction:
Root architectural cause:
Available workaround:
Cost of workaround:
Upstream response:
Current status and version:
Evidence:
Confidence:
```

Prefer three well-supported findings over a long list of minor complaints.

### Testing Strategy

Target length: 400-800 words.

Describe the project's testing model, representative tests, user-facing testing
APIs, strengths, and gaps. This section is mandatory for every deep dive and
standard profile.

### Lessons For ArborUI

Target length: 300-600 words.

Conclude with:

- Practices ArborUI should adopt
- Failure modes ArborUI should avoid
- Problems ArborUI already approaches differently
- Claims ArborUI has not yet proven
- Follow-up prototypes, benchmarks, tests, or architecture decisions

### Evidence Appendix

List:

- Documentation and architecture references
- Source and test files
- Releases, revisions, and access dates
- Issues and maintainer discussions
- Substantial applications or migration reports
- Confidence qualifications

## Brief Survey Template

Use this compact form for projects that do not warrant a substantial report:

```text
Project and version:
Category and scope:
Distinctive strength:
Important limitation or tradeoff:
Testing observation:
Relevance to ArborUI:
Best sources:
```

## Testing Research Framework

Testing is a primary research subject, not a row to fill with a test count.
Evaluate each applicable layer separately.

| Layer | Evidence to seek |
| --- | --- |
| Pure unit tests | Geometry, layout, style, input decoding, and event routing |
| Reference-model tests | Optimized algorithms compared with simple implementations |
| Property and fuzz tests | Unicode, clipping, edits, fragmented input, and frame replay |
| Headless widget tests | Rendering at explicit sizes with normalized event injection |
| Application harness | The real runtime, scheduler, focus, commands, and renderer driven together |
| Failure injection | Partial writes, unknown physical state, backpressure, and interrupted input |
| Snapshot tests | Stable representations, semantic assertions, and deliberate review rules |
| Virtual terminal tests | ANSI output applied to an emulator and inspected semantically |
| PTY tests | Raw mode, alternate screen, panic, signals, suspend, resume, and restoration |
| Compatibility tests | Platforms, emulators, multiplexers, protocols, and width policies |
| Performance contracts | Emitted bytes, patch shape, allocations, latency, and idle CPU |

Every substantial report should answer:

- Can users test a complete application without opening a real terminal?
- Does the harness exercise production code or a parallel fake path?
- Can tests inject key, mouse, paste, resize, and external events?
- Can tests control clocks, timers, asynchronous completion, and settling?
- Can tests inspect focus, cursor, styles, hit targets, patches, and model state?
- Are visual snapshots supplemented by semantic assertions?
- Are Unicode and layout boundary cases represented?
- Can output failures and partial or deferred writes be simulated?
- Are PTYs or terminal emulators used for behavior mocks cannot represent?
- Are platform, emulator, and terminal protocol differences tested?
- Are optimized paths checked against a simple correctness oracle?
- Are performance contracts deterministic, or based only on timing benchmarks?

Include one or two representative test examples when possible. The purpose is
to understand the testing model and its blind spots, not to count test files.

## Evidence Policy

The reports must make it possible to distinguish current verified behavior from
project intent, historical reports, and researcher inference.

### Research Baseline

Every report begins with an evidence header containing:

```text
Research date:
Project version:
Project revision:
Repository:
Documentation version:
Primary platform examined:
Report depth:
```

Use the latest stable release available when research begins as the default
baseline. Record the exact release and source revision. If unreleased behavior
on the main branch is relevant, discuss it separately and pin it to a commit;
do not silently combine release and development behavior.

Pin documentation separately from source. When a website documents only the
current development branch, record its revision or access date and verify
release-sensitive claims against versioned API documentation or tagged source.
Do not assume the current website describes the selected stable release.

When a project has no stable release, pin the repository revision and state
that the report examines a development snapshot. If platform-specific behavior
is material, record the operating system, terminal, multiplexer, and relevant
environment configuration.

### Source Priority

Prefer evidence in this order:

1. Implementation and tests at the recorded revision
2. Version-matched architecture and API documentation
3. Maintainer explanations and migration postmortems
4. Reproducible issues with minimal examples
5. Workarounds used by substantial applications
6. README claims and secondary commentary

Marketing descriptions may establish project intent, but they do not verify
implementation behavior. The absence of a documented feature is not proof that
the feature is impossible; check implementation, tests, issues, and extension
points before making a negative claim.

When reporting that a repository contains no test facility, benchmark, harness,
or other mechanism, state the searched revision and scope. Phrase the result as
"not found at the recorded revision" rather than claiming universal absence.

### Evidence Status

Assign one status to every consequential finding:

| Status | Meaning |
| --- | --- |
| Verified | Confirmed in current implementation or reproduced against the recorded version |
| Supported | Established by current primary documentation or multiple consistent primary sources, but not reproduced |
| Reported | Described in a credible issue, maintainer statement, or application postmortem but not independently verified |
| Inferred | Deduced from architecture or source without an explicit contract or reproduction |
| Unknown | Available evidence is insufficient or contradictory |

Use high confidence only for verified findings, medium confidence for supported
findings, and low confidence for reported or inferred findings. Unknown findings
remain open questions and must not appear as conclusions.

### Citation Requirements

Use descriptive inline Markdown links at the point where evidence supports a
claim. The evidence appendix records the complete context:

| Field | Required content |
| --- | --- |
| Claim | The specific conclusion supported by the source |
| Source | Descriptive link to code, test, documentation, issue, or application |
| Version or revision | Release, tag, or commit to which the evidence applies |
| Source date | Publication, commit, or issue date when available |
| Accessed | Date the source was examined |
| Status | Verified, supported, reported, inferred, or unknown |
| Notes | Reproduction result, issue state, platform limits, or later changes |

Prefer immutable links to tagged documentation or commit-pinned source lines.
For issues and discussions, record whether they are open or closed and whether a
linked change actually shipped. Keep direct quotations short, exact, and in
their original context.

### Negative And Historical Claims

Before reporting a flaw or missing capability:

- State the application requirement it affects
- Confirm that the finding applies to the recorded baseline
- Search for an official extension point or documented workaround
- Check open and closed issues for later fixes or rejected designs
- Classify the finding using the problem types defined above
- Record the workaround and its cost

Historical evidence must be labeled with the affected version and followed by a
current-status check. Historical failures remain useful when they reveal an
architectural tradeoff, migration trigger, or testing lesson, but they are not
evidence that the current project still has the same behavior.

Issue counts and isolated complaints are not evidence of general quality.
Search for recurring failure patterns, blocked architectural changes, forks,
and workarounds that cross subsystem boundaries.

### Reproduction Policy

Reproduce a claim when it is consequential to ArborUI, disputed, poorly
documented, or central to a comparative conclusion. Do not reproduce every
minor issue.

For each reproduction, record:

- Exact project version or revision
- Platform, terminal size, terminal emulator, and relevant environment
- Minimal source or fixture
- Commands and configuration
- Expected and observed behavior
- Whether the result was repeatable
- Any generated snapshot, trace, benchmark, or terminal capture

Store reusable fixtures or prototypes alongside the report only when they add
evidence that cannot be represented clearly in prose. Do not commit credentials,
machine-specific paths, or unreviewed generated artifacts.

### Performance Evidence

Treat performance as a reproducible claim rather than a general impression.
Record the workload, application state, dimensions, update pattern, hardware,
software versions, build mode, sample count, and measured metrics. Prefer
end-to-end latency, emitted bytes, allocations, memory, and idle work over an
isolated throughput number.

Do not compare benchmark numbers from different projects unless the workload
and environment are materially equivalent. A project-authored benchmark can
explain its own optimization but does not establish cross-project superiority.

## Research Workflow

1. Inventory every project and assign initial category and report depth.
2. Record the current version, documentation, repository, and representative applications.
3. Produce a brief first-pass survey before beginning deep source inspection.
4. Promote projects whose relevance or evidence justifies deeper work.
5. For substantial reports, inspect architecture, implementation, tests, issues, and application usage.
6. Apply the common evaluation scenarios where the project's scope permits.
7. Reproduce only the most consequential or uncertain claims with focused prototypes.
8. Review testing strategies independently across projects to identify reusable patterns.
9. Build a cross-project matrix of requirements, assumptions, extension boundaries, and evidence.
10. Convert findings into ArborUI experiments, roadmap changes, non-goals, or architecture decisions.

## ArborUI Questions To Test

The synthesis should specifically examine:

- Whether Ratatui plus an application layer can provide most of ArborUI's value
- Whether transactional prepared-frame commits address failures users encounter in practice
- Whether explicit invalidation creates unacceptable application ergonomics
- Whether ephemeral borrowed elements plus retained identity justify their complexity
- Whether full layout and painting become a practical performance ceiling
- Whether grapheme-level correctness produces visible compatibility benefits
- Whether runtime and backend independence meaningfully improve integration
- Whether a public deterministic application harness changes user testing quality
- Whether alternate-screen-only high-level behavior is too narrow
- Whether widget and ecosystem maturity outweigh architectural guarantees for adopters

Negative or inconclusive answers are valuable findings. They should narrow
ArborUI's scope or prevent unnecessary complexity.

## Deliverables

The research should produce:

1. A classified inventory of all candidate projects
2. Individual reports at the assigned depth
3. A cross-project capability and extension-boundary matrix
4. A testing-strategy comparison and pattern catalog
5. A catalog of recurring failure modes and successful mitigations
6. ArborUI recommendations classified as adopt, avoid, investigate, defer, or non-goal
7. Follow-up experiments, benchmarks, tests, and architecture decision records

The final synthesis should support a concise position statement:

> ArborUI is for applications that need X. Existing libraries optimize for Y
> and become difficult when Z is required. ArborUI accepts costs A and B to
> provide guarantees C and D.

That conclusion should emerge from evidence rather than being selected in
advance.
