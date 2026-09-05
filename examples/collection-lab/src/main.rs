//! Launches the facade-only collection virtualization experiment.

use std::{error::Error, io, time::Duration};

use arborui::{CrosstermBackend, TerminalBackend, TerminalState, run, terminal::MouseMode};
use arborui_example_collection_lab::CollectionLab;

fn main() -> Result<(), Box<dyn Error>> {
    let backend = CrosstermBackend::new(io::stdout())?;
    let application = CollectionLab::from_terminal_size(backend.size()?);
    let mut terminal_state = TerminalState::fullscreen();
    terminal_state.mouse = MouseMode::Capture;
    terminal_state.title = Some("ArborUI Collection Lab".to_owned());
    run(
        application,
        backend,
        terminal_state,
        Duration::from_millis(16),
    )?;
    Ok(())
}
