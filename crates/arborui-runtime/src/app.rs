use arborui_ui::{Element, Invalidation};

use crate::{Command, EventProxy};

/// A single-owner model updated by serialized messages.
pub trait Application {
    /// Message type accepted by the application and its event handlers.
    type Message: Send + 'static;

    /// Applies one message and describes any resulting effects.
    ///
    /// Visible model changes must call [`UpdateContext::invalidate`]. Request
    /// [`Invalidation::Paint`] for visual-only changes, [`Invalidation::Layout`]
    /// for geometry changes, or [`Invalidation::Recompose`] for structural changes.
    ///
    /// The terminal runtime recovers from an accidentally missed structural
    /// invalidation when the next event detects it, but recovery requires an
    /// extra recomposition before that event can be dispatched. Explicit
    /// invalidation remains part of the application performance contract.
    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message>;

    /// Builds the current frame-local UI declaration.
    fn view(&self) -> Element<'_, Self::Message>;
}

/// Explicit requests available while applying an application message.
pub struct UpdateContext<Message> {
    invalidation: Invalidation,
    proxy: EventProxy<Message>,
}

impl<Message> UpdateContext<Message> {
    pub(crate) const fn new(proxy: EventProxy<Message>) -> Self {
        Self {
            invalidation: Invalidation::None,
            proxy,
        }
    }

    /// Requests visual work, coalescing this with stronger requests in the same turn.
    pub fn invalidate(&mut self, invalidation: Invalidation) {
        self.invalidation.request(invalidation);
    }

    /// Returns a cloneable bounded sender for work that outlives this update.
    ///
    /// Sending is fallible when ingress is full or the runner has closed.
    #[must_use]
    pub fn event_proxy(&self) -> EventProxy<Message> {
        self.proxy.clone()
    }

    pub(crate) const fn requested_invalidation(&self) -> Invalidation {
        self.invalidation
    }
}
