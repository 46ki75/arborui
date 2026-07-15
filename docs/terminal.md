# Terminal

## Scope

`arborui-terminal` defines backend-neutral terminal contracts and lifecycle
state. Concrete integrations such as Crossterm live in separate backend
crates.

## Backend Contract

An initial backend trait may resemble:

```rust
pub trait TerminalBackend {
    type Error: std::error::Error + Send + Sync + 'static;

    fn size(&self) -> Result<Size, Self::Error>;

    fn capabilities(&self) -> &Capabilities;

    fn poll_event(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<TerminalEvent>, Self::Error>;

    fn apply_state(
        &mut self,
        desired: &TerminalState,
    ) -> Result<(), Self::Error>;

    fn write_patch(
        &mut self,
        patch: &FramePatch,
    ) -> Result<WriteOutcome, Self::Error>;

    fn restore(&mut self) -> Result<(), Self::Error>;
}
```

The final trait may split input, output, and lifecycle into separate traits for
remote and embedded use cases. The first implementation should keep the common
case simple while avoiding backend-specific associated types in UI APIs.

## Normalized Input

```rust
pub enum TerminalEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize(Size),
    FocusGained,
    FocusLost,
}
```

`KeyEvent` distinguishes press, repeat, and release when the terminal protocol
provides that information. Legacy protocols may report only presses.

Input parsing must handle fragmented reads, multiple events in one read,
bracketed paste, terminal responses, CSI, SS3, Kitty keyboard sequences, and
mouse protocols. A timeout may be required to distinguish Escape from an
incomplete sequence; it must be configurable and tested.

Backend event types are translated before they leave the backend crate.

## Capabilities

```rust
pub struct Capabilities {
    pub color: ColorCapability,
    pub keyboard: KeyboardCapability,
    pub mouse: MouseCapability,
    pub synchronized_updates: bool,
    pub bracketed_paste: bool,
    pub focus_reporting: bool,
    pub hyperlinks: bool,
    pub clipboard: ClipboardCapability,
    pub explicit_width: bool,
    pub width_policy: WidthPolicy,
}
```

Capabilities come from environment inspection, protocol queries, known
terminal behavior, user overrides, and backend limitations. Detection must not
be scattered across widgets or ANSI serialization code.

Applications may inspect capabilities, but widgets should normally express
semantic intent and let rendering choose a fallback.

## Desired Terminal State

```rust
pub struct TerminalState {
    pub screen: ScreenMode,
    pub cursor: CursorState,
    pub mouse: MouseMode,
    pub keyboard: KeyboardMode,
    pub bracketed_paste: bool,
    pub focus_reporting: bool,
    pub synchronized_updates: bool,
    pub title: Option<String>,
    pub autowrap: AutowrapMode,
}
```

The runtime declares desired state. The session compares it with active state
and emits only required transitions.

Initial screen modes:

```rust
pub enum ScreenMode {
    Main,
    Alternate,
}
```

An OpenTUI-style split footer is a planned extension. Its API should not be
finalized until main-screen scrolling and immutable scrollback have dedicated
integration tests.

## Terminal Session

`TerminalSession<B>` owns a backend and all terminal modes enabled through it.
It is an RAII guard with idempotent restoration.

```rust
let session = TerminalSession::builder(backend)
    .screen(ScreenMode::Alternate)
    .mouse(MouseMode::CellMotion)
    .bracketed_paste(true)
    .open()?;
```

Restoration covers:

- Raw or cooked input mode
- Alternate screen
- Cursor visibility, shape, and style
- Mouse reporting
- Bracketed paste
- Focus reporting
- Enhanced keyboard protocols
- Synchronized update mode
- Autowrap changes
- Active text styles and hyperlinks

`Drop` performs best-effort restoration. Explicit `restore` returns errors and
is preferred during orderly shutdown.

## Output Contract

```rust
pub enum WriteOutcome {
    Applied,
    Deferred,
    StateUnknown,
}
```

The meanings are strict:

| Outcome | Meaning | Renderer action |
| --- | --- | --- |
| `Applied` | The complete patch was accepted in order | Commit prepared frame |
| `Deferred` | No bytes were applied | Retry or discard |
| `StateUnknown` | Output may be partial | Force full repaint |

A backend must not report `Deferred` after applying a prefix of a patch.
Returning an error also means output may be partial. `TerminalSession` records
a full-repaint requirement before forwarding that error to its caller.

Local terminal output may use blocking `write_all`. Remote or buffered backends
may queue bytes and report `Applied` once they assume responsibility for
ordered delivery. Backends must document whether queued output can later fail
and how that invalidates screen state.

No-op frames emit no bytes, including no synchronized-update envelope.

## ANSI Serialization

The Crossterm or future native ANSI backend serializes cell runs while tracking
logical output state:

- Cursor position
- Foreground and background colors
- Text attributes
- Hyperlink state
- Synchronized update state
- Autowrap policy

Serialization must not rely on automatic wrapping. Runs crossing the final
column are split or followed by explicit positioning. Wide and ambiguous-width
graphemes may require cursor repositioning according to capabilities.

Absolute cursor placement is preferred for independently changed runs because
it limits propagation from an incorrect cursor-advance assumption.

## Suspend And Resume

Applications must be able to hand the terminal to a child program such as an
editor, pager, shell, or fuzzy finder.

Suspend performs:

1. Finish or invalidate pending output.
2. Disable mouse and extended keyboard protocols.
3. Disable paste and focus reporting.
4. Restore cursor and styles.
5. Leave the alternate screen when required.
6. Restore cooked input mode.
7. Stop consuming terminal input.

Resume performs:

1. Reacquire input ownership.
2. Restore raw mode.
3. Probe or restore required capabilities and modes.
4. Re-enter the configured screen.
5. Force a complete repaint.

Resume never trusts the old committed framebuffer because the child process
may have changed any terminal cell or mode.

## Signals And Process Lifecycle

Unix job-control integration is not implemented yet. The target contract is:

- `SIGTSTP` follows the suspend sequence before stopping.
- `SIGCONT` follows the resume sequence after continuing.
- Termination signals attempt best-effort restoration before delegating or exiting.

Signal integration must avoid process-global handler conflicts. The design
should support one explicit owner and make limitations clear when multiple
sessions exist in one process.

Panic hooks are not sufficient by themselves. The RAII session remains the
primary restoration mechanism, with optional process-level integration in the
runtime.

## Crossterm Backend

`arborui-backend-crossterm` is the first implementation because it provides a
practical cross-platform baseline.

The adapter owns:

- Crossterm command and event translation
- Windows console differences
- Raw mode calls
- Event polling
- ANSI or Crossterm output selection
- Backend-specific error mapping

Crossterm types do not escape the crate. Raw mode and event reading are
process-global; applications must enforce a single active local event reader.
See [Compatibility](compatibility.md) for the tested lifecycle boundary and
known restoration limitations.
