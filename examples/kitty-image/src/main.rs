//! Launches the Kitty graphics example with a configurable capability policy.

use std::{env, error::Error, io, time::Duration};

use arborui::{
    CrosstermBackend, KittyGraphicsMode, TerminalBackend, TerminalState, run, terminal::MouseMode,
};
use arborui_example_kitty_image::KittyImageDemo;

fn main() -> Result<(), Box<dyn Error>> {
    let Some((mode, policy)) = graphics_mode()? else {
        return Ok(());
    };
    let backend = CrosstermBackend::new(io::stdout())?.with_kitty_graphics(mode);
    let status = format!(
        "Policy: {policy}; Kitty graphics: {}",
        if backend.capabilities().kitty_graphics {
            "enabled"
        } else {
            "disabled"
        }
    );
    let mut terminal_state = TerminalState::fullscreen();
    terminal_state.mouse = MouseMode::Capture;
    terminal_state.title = Some("ArborUI Kitty Image Lab".to_owned());

    run(
        KittyImageDemo::new(status)?,
        backend,
        terminal_state,
        Duration::from_millis(16),
    )?;
    Ok(())
}

fn graphics_mode() -> io::Result<Option<(KittyGraphicsMode, &'static str)>> {
    let mut arguments = env::args().skip(1);
    let argument = arguments.next();
    if arguments.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected at most one of --auto, --kitty, or --no-kitty",
        ));
    }

    match argument.as_deref() {
        None | Some("--auto") => Ok(Some((KittyGraphicsMode::Auto, "auto"))),
        Some("--kitty") => Ok(Some((KittyGraphicsMode::Enabled, "forced enabled"))),
        Some("--no-kitty") => Ok(Some((KittyGraphicsMode::Disabled, "forced disabled"))),
        Some("--help" | "-h") => {
            println!("Usage: arborui-example-kitty-image [--auto|--kitty|--no-kitty]");
            Ok(None)
        }
        Some(other) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown graphics mode {other:?}"),
        )),
    }
}
