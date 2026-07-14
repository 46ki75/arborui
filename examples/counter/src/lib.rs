//! Minimal counter application using only the `arborui` facade.

use arborui::prelude::*;

/// Messages accepted by [`Counter`].
pub enum Message {
    /// Increase the displayed count.
    Increment,
    /// Request orderly application shutdown.
    Quit,
}

/// Small model-update-view application used by examples and downstream tests.
pub struct Counter {
    count: usize,
    label: String,
}

impl Default for Counter {
    fn default() -> Self {
        Self {
            count: 0,
            label: "Count: 0".to_owned(),
        }
    }
}

impl Counter {
    /// Returns the current count.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.count
    }
}

impl Application for Counter {
    type Message = Message;

    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        match message {
            Message::Increment => {
                self.count += 1;
                self.label = format!("Count: {}", self.count);
                context.invalidate(Invalidation::Paint);
                Command::none()
            }
            Message::Quit => Command::quit(),
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        column_with_gap(
            [
                text(&self.label),
                button("Increment", || Message::Increment)
                    .build()
                    .key("increment"),
                button("Quit", || Message::Quit).build().key("quit"),
            ],
            1,
        )
    }
}
