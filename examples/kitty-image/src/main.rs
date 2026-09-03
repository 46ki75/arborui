//! Launches the Kitty graphics example with a configurable capability policy.

use std::{env, error::Error, ffi::OsStr, io, path::PathBuf, time::Duration};

use arborui::{
    CrosstermBackend, KittyGraphicsMode, TerminalBackend, TerminalState, run, terminal::MouseMode,
};
use arborui_example_kitty_image::KittyImageDemo;

fn main() -> Result<(), Box<dyn Error>> {
    let Some(options) = launch_options()? else {
        return Ok(());
    };
    let backend = CrosstermBackend::new(io::stdout())?.with_kitty_graphics(options.graphics_mode);
    let status = format!(
        "Policy: {}; Kitty graphics: {}",
        options.graphics_policy,
        if backend.capabilities().kitty_graphics {
            "enabled"
        } else {
            "disabled"
        }
    );
    let mut terminal_state = TerminalState::fullscreen();
    terminal_state.mouse = MouseMode::Capture;
    terminal_state.title = Some("ArborUI Kitty Image Lab".to_owned());
    let application = match options.image {
        Some(path) => KittyImageDemo::with_image(status, arborui::image_decoder::load(path)?)?,
        None => KittyImageDemo::new(status)?,
    };

    run(
        application,
        backend,
        terminal_state,
        Duration::from_millis(16),
    )?;
    Ok(())
}

struct LaunchOptions {
    graphics_mode: KittyGraphicsMode,
    graphics_policy: &'static str,
    image: Option<PathBuf>,
}

fn launch_options() -> io::Result<Option<LaunchOptions>> {
    let mut options = LaunchOptions {
        graphics_mode: KittyGraphicsMode::Auto,
        graphics_policy: "auto",
        image: None,
    };
    let mut mode_selected = false;
    let mut positional_only = false;
    for argument in env::args_os().skip(1) {
        if !positional_only && argument == OsStr::new("--") {
            positional_only = true;
            continue;
        }
        if !positional_only && matches!(argument.to_str(), Some("--help" | "-h")) {
            println!("Usage: arborui-example-kitty-image [--auto|--kitty|--no-kitty] [IMAGE]");
            return Ok(None);
        }
        let mode = if positional_only {
            None
        } else {
            match argument.to_str() {
                Some("--auto") => Some((KittyGraphicsMode::Auto, "auto")),
                Some("--kitty") => Some((KittyGraphicsMode::Enabled, "forced enabled")),
                Some("--no-kitty") => Some((KittyGraphicsMode::Disabled, "forced disabled")),
                _ => None,
            }
        };
        if let Some((graphics_mode, graphics_policy)) = mode {
            if mode_selected {
                return Err(invalid_arguments(
                    "graphics mode was supplied more than once",
                ));
            }
            mode_selected = true;
            options.graphics_mode = graphics_mode;
            options.graphics_policy = graphics_policy;
            continue;
        }
        if !positional_only && argument.to_string_lossy().starts_with('-') {
            return Err(invalid_arguments(format!(
                "unknown option {:?}",
                argument.to_string_lossy()
            )));
        }
        if options.image.replace(PathBuf::from(argument)).is_some() {
            return Err(invalid_arguments("expected at most one image path"));
        }
    }
    Ok(Some(options))
}

fn invalid_arguments(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
