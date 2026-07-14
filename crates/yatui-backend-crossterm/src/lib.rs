//! Crossterm implementation of the `yatui` terminal backend contract.
//!
//! Crossterm raw mode and event reading are process-global. Applications must
//! not create concurrent local event readers.

mod backend;
mod events;
mod output;

pub use backend::CrosstermBackend;
