use std::{future::Future, pin::Pin, time::Duration};

pub(crate) type CommandFuture<Message> = Pin<Box<dyn Future<Output = Message> + Send + 'static>>;

pub(crate) enum CommandAction<Message> {
    Message(Message),
    Perform(CommandFuture<Message>),
    After(Duration, Message),
    Quit,
}

/// An opaque collection of effects produced by one application update.
pub struct Command<Message> {
    pub(crate) actions: Vec<CommandAction<Message>>,
}

impl<Message> Command<Message> {
    /// Creates a command that performs no work.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// Enqueues one message after the current update.
    #[must_use]
    pub fn message(message: Message) -> Self {
        Self {
            actions: vec![CommandAction::Message(message)],
        }
    }

    /// Combines commands in declaration order.
    ///
    /// Immediate messages retain this order. Future outputs are delivered when
    /// their futures complete and may therefore arrive after later actions.
    #[must_use]
    pub fn batch(commands: impl IntoIterator<Item = Self>) -> Self {
        Self {
            actions: commands
                .into_iter()
                .flat_map(|command| command.actions)
                .collect(),
        }
    }

    /// Polls a runtime-neutral future and maps its output to a message.
    #[must_use]
    pub fn perform<FutureType, Output, Map>(future: FutureType, map_output: Map) -> Self
    where
        FutureType: Future<Output = Output> + Send + 'static,
        Output: Send + 'static,
        Map: FnOnce(Output) -> Message + Send + 'static,
    {
        let mapped = async move { map_output(future.await) };
        Self {
            actions: vec![CommandAction::Perform(Box::pin(mapped))],
        }
    }

    /// Enqueues a message after at least `delay` without requiring an async reactor.
    #[must_use]
    pub fn after(delay: Duration, message: Message) -> Self {
        Self {
            actions: vec![CommandAction::After(delay, message)],
        }
    }

    /// Requests orderly runtime shutdown.
    #[must_use]
    pub fn quit() -> Self {
        Self {
            actions: vec![CommandAction::Quit],
        }
    }
}

impl<Message> Default for Command<Message> {
    fn default() -> Self {
        Self::none()
    }
}
