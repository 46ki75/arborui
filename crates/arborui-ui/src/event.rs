use std::ops::{BitOr, BitOrAssign};

use arborui_core::{Point, Size};

use crate::{Invalidation, NodeId};

/// Phase in the capture-target-bubble dispatch sequence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventPhase {
    /// Nodes from the root through the target.
    Capture,
    /// The selected target only.
    Target,
    /// Nodes from the target back through the root.
    Bubble,
}

/// A key understood by default UI behavior.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UiKey {
    /// Backspace.
    Backspace,
    /// Tab traversal key.
    Tab,
    /// Enter or return.
    Enter,
    /// Escape.
    Escape,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Start of a line or collection.
    Home,
    /// End of a line or collection.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Forward delete.
    Delete,
    /// Insert.
    Insert,
    /// Function key.
    Function(u8),
    /// A Unicode character.
    Character(char),
    /// A key without built-in UI semantics.
    Other,
}

/// Active modifiers for a UI key event.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// Shift modifier.
    pub const SHIFT: Self = Self(1 << 0);
    /// Control modifier.
    pub const CONTROL: Self = Self(1 << 1);
    /// Alt modifier.
    pub const ALT: Self = Self(1 << 2);
    /// Super or command modifier.
    pub const SUPER: Self = Self(1 << 3);
    /// Hyper modifier.
    pub const HYPER: Self = Self(1 << 4);
    /// Meta modifier.
    pub const META: Self = Self(1 << 5);

    /// Returns whether every modifier in `other` is active.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns whether no modifiers are active.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for KeyModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Key press lifecycle.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum KeyAction {
    /// Initial key press.
    #[default]
    Press,
    /// Automatic repeat.
    Repeat,
    /// Key release.
    Release,
}

/// Keyboard input routed to the focused node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UiKeyEvent {
    /// Logical key.
    pub key: UiKey,
    /// Active modifiers.
    pub modifiers: KeyModifiers,
    /// Press lifecycle.
    pub action: KeyAction,
}

/// Pointer button.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PointerButton {
    /// Primary button.
    Primary,
    /// Secondary button.
    Secondary,
    /// Middle button.
    Middle,
}

/// Pointer action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PointerEventKind {
    /// Button press.
    Down(PointerButton),
    /// Button release.
    Up(PointerButton),
    /// Movement while a button is pressed.
    Drag(PointerButton),
    /// Movement without a pressed button.
    Moved,
    /// Vertical scroll amount.
    Scroll(i16),
    /// Horizontal scroll amount.
    ScrollHorizontal(i16),
}

/// Spatial pointer input in viewport coordinates.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PointerEvent {
    /// Pointer action.
    pub kind: PointerEventKind,
    /// Viewport position.
    pub position: Point,
    /// Active keyboard modifiers.
    pub modifiers: KeyModifiers,
}

/// Event routed through the retained UI tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UiEvent {
    /// Keyboard input.
    Key(UiKeyEvent),
    /// Text input that is not represented as a control key.
    Text(String),
    /// Complete bracketed-paste payload.
    Paste(String),
    /// Pointer input.
    Pointer(PointerEvent),
    /// Logical focus entered a node.
    FocusGained,
    /// Logical focus left a node.
    FocusLost,
    /// Pointer hover entered a node.
    PointerEntered,
    /// Pointer hover left a node.
    PointerLeft,
    /// Terminal window gained focus.
    TerminalFocusGained,
    /// Terminal window lost focus.
    TerminalFocusLost,
    /// Viewport resized.
    Resize(Size),
    /// Scheduler animation or idle tick.
    Tick,
    /// Application-defined event identity.
    Custom(u64),
}

impl UiEvent {
    pub(crate) const fn pointer(&self) -> Option<PointerEvent> {
        match self {
            Self::Pointer(event) => Some(*event),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum EventRequest {
    Focus(NodeId),
    CapturePointer(NodeId),
    ReleasePointer,
    Invalidate(NodeId, Invalidation),
}

pub(crate) struct DispatchState<Message> {
    pub(crate) messages: Vec<Message>,
    pub(crate) handled: bool,
    pub(crate) default_prevented: bool,
    pub(crate) propagation_stopped: bool,
    pub(crate) requests: Vec<EventRequest>,
}

impl<Message> DispatchState<Message> {
    pub(crate) const fn new() -> Self {
        Self {
            messages: Vec::new(),
            handled: false,
            default_prevented: false,
            propagation_stopped: false,
            requests: Vec::new(),
        }
    }
}

/// Mutable controls available to one ephemeral event handler.
pub struct EventContext<'a, Message> {
    node: NodeId,
    phase: EventPhase,
    state: &'a mut DispatchState<Message>,
}

impl<'a, Message> EventContext<'a, Message> {
    pub(crate) fn new(
        node: NodeId,
        phase: EventPhase,
        state: &'a mut DispatchState<Message>,
    ) -> Self {
        Self { node, phase, state }
    }

    /// Returns the retained node handling the event.
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Returns the current dispatch phase.
    #[must_use]
    pub const fn phase(&self) -> EventPhase {
        self.phase
    }

    /// Emits an application message in handler invocation order.
    pub fn emit(&mut self, message: Message) {
        self.state.messages.push(message);
    }

    /// Marks the event handled without stopping propagation.
    pub fn mark_handled(&mut self) {
        self.state.handled = true;
    }

    /// Prevents built-in focus behavior without stopping propagation.
    pub fn prevent_default(&mut self) {
        self.state.default_prevented = true;
    }

    /// Stops all later handlers, including handlers on the current node.
    pub fn stop_propagation(&mut self) {
        self.state.propagation_stopped = true;
    }

    /// Requests focus for the current node after routing completes.
    pub fn request_focus(&mut self) {
        self.state.requests.push(EventRequest::Focus(self.node));
    }

    /// Captures later drag and release events for the current node.
    pub fn capture_pointer(&mut self) {
        self.state
            .requests
            .push(EventRequest::CapturePointer(self.node));
    }

    /// Releases UI pointer capture after routing completes.
    pub fn release_pointer(&mut self) {
        self.state.requests.push(EventRequest::ReleasePointer);
    }

    /// Requests visual work for the current node.
    pub fn invalidate(&mut self, invalidation: Invalidation) {
        self.state
            .requests
            .push(EventRequest::Invalidate(self.node, invalidation));
    }
}

/// Result of one synchronous event dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchOutcome<Message> {
    /// Selected retained target.
    pub target: Option<NodeId>,
    /// Messages emitted in deterministic handler order.
    pub messages: Vec<Message>,
    /// Whether any handler marked the event handled.
    pub handled: bool,
    /// Whether any handler prevented built-in behavior.
    pub default_prevented: bool,
    /// Whether propagation was stopped.
    pub propagation_stopped: bool,
}

impl<Message> DispatchOutcome<Message> {
    pub(crate) fn from_state(target: Option<NodeId>, state: DispatchState<Message>) -> Self {
        Self {
            target,
            messages: state.messages,
            handled: state.handled,
            default_prevented: state.default_prevented,
            propagation_stopped: state.propagation_stopped,
        }
    }
}
