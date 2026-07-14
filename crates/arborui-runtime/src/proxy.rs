use std::{
    fmt,
    sync::{Arc, mpsc},
};

use crate::scheduler::WakeSignal;

/// Error returned when the application runner no longer receives messages.
pub struct EventProxySendError<Message> {
    message: Message,
}

impl<Message> EventProxySendError<Message> {
    /// Recovers the message that could not be delivered.
    #[must_use]
    pub fn into_inner(self) -> Message {
        self.message
    }
}

impl<Message> fmt::Debug for EventProxySendError<Message> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EventProxySendError { .. }")
    }
}

impl<Message> fmt::Display for EventProxySendError<Message> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("application runner is no longer receiving messages")
    }
}

impl<Message> std::error::Error for EventProxySendError<Message> {}

/// A thread-safe, cloneable application message sender.
pub struct EventProxy<Message> {
    sender: mpsc::Sender<Message>,
    wake: Arc<WakeSignal>,
}

impl<Message> EventProxy<Message> {
    pub(crate) const fn new(sender: mpsc::Sender<Message>, wake: Arc<WakeSignal>) -> Self {
        Self { sender, wake }
    }

    /// Sends a message and wakes a runner waiting through its scheduler.
    ///
    /// A runner currently inside a synchronous terminal backend poll observes
    /// the message when that poll's configured timeout expires.
    pub fn send(&self, message: Message) -> Result<(), EventProxySendError<Message>> {
        self.sender
            .send(message)
            .map_err(|error| EventProxySendError { message: error.0 })?;
        self.wake.notify();
        Ok(())
    }
}

impl<Message> Clone for EventProxy<Message> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            wake: Arc::clone(&self.wake),
        }
    }
}

impl<Message> fmt::Debug for EventProxy<Message> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EventProxy { .. }")
    }
}
