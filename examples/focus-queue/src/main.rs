//! Launches the focus queue pilot with the default Crossterm backend.

use std::{error::Error, io, num::NonZeroUsize, time::Duration};

use arborui::{
    CrosstermBackend, RuntimeOptions, TerminalState, run_with_options, terminal::MouseMode,
};
use arborui_example_focus_queue::FocusQueue;

fn main() -> Result<(), Box<dyn Error>> {
    let backend = CrosstermBackend::new(io::stdout())?;
    let mut terminal_state = TerminalState::fullscreen();
    terminal_state.mouse = MouseMode::Capture;
    terminal_state.title = Some("ArborUI Focus Queue".to_owned());

    let runtime_options = RuntimeOptions::new()
        .with_event_ingress_capacity(NonZeroUsize::new(8).unwrap_or(NonZeroUsize::MIN));
    run_with_options(
        FocusQueue::default(),
        backend,
        terminal_state,
        Duration::from_millis(16),
        runtime_options,
    )?;
    Ok(())
}
