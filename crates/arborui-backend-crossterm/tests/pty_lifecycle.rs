//! Process-isolated terminal lifecycle tests for native PTYs and ConPTY.

use std::{
    env,
    error::Error,
    io::{self, Read, Write},
    thread,
    time::{Duration, Instant},
};

use arborui_backend_crossterm::CrosstermBackend;
use arborui_terminal::{TerminalSession, TerminalState};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const FIXTURE_ENV: &str = "ARBORUI_PTY_LIFECYCLE_FIXTURE";
const ACTIVE_MARKER: &str = "ARBORUI_PTY_ACTIVE";
const RESTORED_MARKER: &str = "ARBORUI_PTY_RESTORED";

#[test]
fn pty_fixture_restores_after_drop() -> Result<(), Box<dyn Error>> {
    if env::var_os(FIXTURE_ENV).is_none() {
        return Ok(());
    }

    let backend = CrosstermBackend::new(io::stdout())?;
    {
        let _session = TerminalSession::open(backend, TerminalState::fullscreen())?;
        println!("{ACTIVE_MARKER}");
        io::stdout().flush()?;
    }
    println!("{RESTORED_MARKER}");
    io::stdout().flush()?;
    Ok(())
}

#[test]
#[ignore = "requires a native PTY or ConPTY"]
fn restores_terminal_modes_in_native_pty() -> Result<(), Box<dyn Error>> {
    let pair = native_pty_system().openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    #[cfg(unix)]
    let baseline_termios = pair.master.get_termios();
    let mut command = CommandBuilder::new(env::current_exe()?);
    command.arg("--exact");
    command.arg("pty_fixture_restores_after_drop");
    command.arg("--nocapture");
    command.env(FIXTURE_ENV, "1");
    command.env("TERM", "xterm-256color");

    let mut reader = pair.master.try_clone_reader()?;
    let output_thread = thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output).map(|_| output)
    });
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    #[cfg(unix)]
    assert_eq!(pair.master.get_termios(), baseline_termios);
    drop(pair.master);
    let output = output_thread
        .join()
        .map_err(|_| "PTY output reader panicked")??;
    let output_text = String::from_utf8_lossy(&output);

    let Some(status) = status else {
        return Err(format!("PTY fixture timed out: {output_text}").into());
    };
    assert!(status.success(), "fixture failed: {output_text}");
    assert_in_order(
        &output,
        &[
            b"\x1b[?1049h",
            ACTIVE_MARKER.as_bytes(),
            b"\x1b[?1049l",
            RESTORED_MARKER.as_bytes(),
        ],
    )?;
    Ok(())
}

fn assert_in_order(output: &[u8], expected: &[&[u8]]) -> Result<(), Box<dyn Error>> {
    let mut remaining = output;
    for sequence in expected {
        let Some(position) = remaining
            .windows(sequence.len())
            .position(|window| window == *sequence)
        else {
            return Err(format!(
                "missing sequence {sequence:?} in {:?}",
                String::from_utf8_lossy(output)
            )
            .into());
        };
        remaining = &remaining[position + sequence.len()..];
    }
    Ok(())
}
