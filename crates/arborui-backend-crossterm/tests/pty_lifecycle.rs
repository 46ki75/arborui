//! Process-isolated terminal lifecycle tests for native PTYs and ConPTY.

use std::{
    env,
    error::Error,
    io::{self, Read, Write},
    thread,
    time::{Duration, Instant},
};

use arborui_backend_crossterm::CrosstermBackend;
use arborui_runtime::{Application, Command, UpdateContext};
use arborui_terminal::{MouseMode, ScreenMode, TerminalBackend, TerminalSession, TerminalState};
use arborui_ui::Element;
#[cfg(windows)]
use crossterm::{
    Command as _,
    event::{DisableMouseCapture, EnableMouseCapture},
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const FIXTURE_ENV: &str = "ARBORUI_PTY_LIFECYCLE_FIXTURE";
const RAW_RECOVERY_FIXTURE_ENV: &str = "ARBORUI_PTY_RAW_RECOVERY_FIXTURE";
const MOUSE_RECOVERY_FIXTURE_ENV: &str = "ARBORUI_PTY_MOUSE_RECOVERY_FIXTURE";
const PANIC_FIXTURE_ENV: &str = "ARBORUI_PTY_PANIC_FIXTURE";
const ACTIVE_MARKER: &str = "ARBORUI_PTY_ACTIVE";
const RESTORED_MARKER: &str = "ARBORUI_PTY_RESTORED";
const PANIC_MARKER: &str = "ARBORUI_PTY_PANIC";
const PANIC_REPORT_PREFIX: &str = "arborui: terminal restored after panic: ";

struct PanicApp;

impl Application for PanicApp {
    type Message = ();

    fn update(
        &mut self,
        _message: Self::Message,
        _context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        panic!("{PANIC_MARKER}")
    }
}

struct FailFlushOnce<W> {
    writer: W,
    flushes: usize,
    fail_on_flush: usize,
}

impl<W: Write> Write for FailFlushOnce<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.writer.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        if self.flushes == self.fail_on_flush {
            return Err(io::Error::other("injected flush failure"));
        }
        self.writer.flush()
    }
}

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
fn pty_fixture_reenables_raw_mode_after_failed_restore_flush() -> Result<(), Box<dyn Error>> {
    if env::var_os(RAW_RECOVERY_FIXTURE_ENV).is_none() {
        return Ok(());
    }

    let writer = FailFlushOnce {
        writer: io::stdout(),
        flushes: 0,
        fail_on_flush: 2,
    };
    let mut backend = CrosstermBackend::new(writer)?;
    let desired = TerminalState {
        raw_mode: true,
        ..TerminalState::default()
    };
    backend.apply_state(&desired)?;
    assert!(crossterm::terminal::is_raw_mode_enabled()?);

    assert!(backend.restore().is_err());
    assert!(!crossterm::terminal::is_raw_mode_enabled()?);
    backend.apply_state(&desired)?;
    assert!(
        crossterm::terminal::is_raw_mode_enabled()?,
        "raw mode was successfully disabled despite the restore flush failure, so recovery \
         must re-enable it"
    );
    backend.restore()?;
    Ok(())
}

#[test]
fn pty_fixture_restores_after_first_mouse_enable_flush_failure() -> Result<(), Box<dyn Error>> {
    if env::var_os(MOUSE_RECOVERY_FIXTURE_ENV).is_none() {
        return Ok(());
    }

    #[cfg(windows)]
    {
        // A fresh process must not inherit Crossterm's saved mouse console mode.
        assert!(!EnableMouseCapture.is_ansi_code_supported());
        let error = DisableMouseCapture
            .execute_winapi()
            .expect_err("mouse capture must not have initialized the saved console mode yet");
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "Initial console modes not set");
    }

    let writer = FailFlushOnce {
        writer: io::stdout(),
        flushes: 0,
        fail_on_flush: 2,
    };
    let mut backend = CrosstermBackend::new(writer)?;
    let alternate = TerminalState {
        screen: ScreenMode::Alternate,
        ..TerminalState::default()
    };
    backend.apply_state(&alternate)?;
    println!("{ACTIVE_MARKER}");
    io::stdout().flush()?;

    let capture = TerminalState {
        mouse: MouseMode::Capture,
        ..alternate
    };
    let error = backend
        .apply_state(&capture)
        .expect_err("the first mouse enable must fail at the injected flush");
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "injected flush failure");

    #[cfg(windows)]
    {
        // QueueableCommand flushes before execute_winapi, so enable never ran.
        let error = DisableMouseCapture
            .execute_winapi()
            .expect_err("the failed pre-enable flush must leave the saved mode uninitialized");
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "Initial console modes not set");
    }

    backend.restore()?;
    backend.restore()?;
    println!("{RESTORED_MARKER}");
    io::stdout().flush()?;

    backend.apply_state(&capture)?;
    println!("{ACTIVE_MARKER}");
    io::stdout().flush()?;
    backend.restore()?;
    println!("{RESTORED_MARKER}");
    io::stdout().flush()?;
    Ok(())
}

#[test]
fn pty_fixture_restores_before_reporting_panic() -> Result<(), Box<dyn Error>> {
    if env::var_os(PANIC_FIXTURE_ENV).is_none() {
        return Ok(());
    }

    let backend = CrosstermBackend::new(io::stdout())?;
    match arborui_runtime::run(
        PanicApp,
        backend,
        TerminalState::fullscreen(),
        Duration::ZERO,
    ) {
        Ok(_) => Err("panic fixture unexpectedly returned an application".into()),
        Err(error) => Err(error.into()),
    }
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

    let reader = pair.master.try_clone_reader()?;
    #[cfg(windows)]
    let output_thread = capture_pty_output(reader, pair.master.take_writer()?);
    #[cfg(not(windows))]
    let output_thread = capture_pty_output(reader);
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

#[test]
#[ignore = "requires a native PTY or ConPTY"]
fn reenables_raw_mode_after_failed_restore_flush_in_native_pty() -> Result<(), Box<dyn Error>> {
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
    command.arg("pty_fixture_reenables_raw_mode_after_failed_restore_flush");
    command.arg("--nocapture");
    command.env(RAW_RECOVERY_FIXTURE_ENV, "1");
    command.env("TERM", "xterm-256color");

    let reader = pair.master.try_clone_reader()?;
    #[cfg(windows)]
    let output_thread = capture_pty_output(reader, pair.master.take_writer()?);
    #[cfg(not(windows))]
    let output_thread = capture_pty_output(reader);
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
    Ok(())
}

#[test]
#[ignore = "requires a native PTY or ConPTY"]
fn restores_after_first_mouse_enable_flush_failure_in_native_pty() -> Result<(), Box<dyn Error>> {
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
    command.arg("pty_fixture_restores_after_first_mouse_enable_flush_failure");
    command.arg("--nocapture");
    command.env(MOUSE_RECOVERY_FIXTURE_ENV, "1");
    command.env("TERM", "xterm-256color");

    let reader = pair.master.try_clone_reader()?;
    #[cfg(windows)]
    let output_thread = capture_pty_output(reader, pair.master.take_writer()?);
    #[cfg(not(windows))]
    let output_thread = capture_pty_output(reader);
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
            b"\x1b[?1049h",
            ACTIVE_MARKER.as_bytes(),
            b"\x1b[?1049l",
            RESTORED_MARKER.as_bytes(),
        ],
    )?;
    Ok(())
}

#[test]
#[ignore = "requires a native PTY or ConPTY"]
fn restores_before_reporting_panic_in_native_pty() -> Result<(), Box<dyn Error>> {
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
    command.arg("pty_fixture_restores_before_reporting_panic");
    command.arg("--nocapture");
    command.env(PANIC_FIXTURE_ENV, "1");
    command.env("TERM", "xterm-256color");

    let reader = pair.master.try_clone_reader()?;
    #[cfg(windows)]
    let output_thread = capture_pty_output(reader, pair.master.take_writer()?);
    #[cfg(not(windows))]
    let output_thread = capture_pty_output(reader);
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
    assert!(
        !status.success(),
        "panic fixture unexpectedly passed: {output_text}"
    );
    assert_in_order(
        &output,
        &[
            b"\x1b[?1049h",
            b"\x1b[?1049l",
            PANIC_REPORT_PREFIX.as_bytes(),
            PANIC_MARKER.as_bytes(),
        ],
    )?;
    Ok(())
}

fn capture_pty_output(
    mut reader: Box<dyn Read + Send>,
    #[cfg(windows)] mut responder: Box<dyn Write + Send>,
) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0; 1024];
        #[cfg(windows)]
        let mut search_from = 0;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            output.extend_from_slice(&buffer[..count]);

            #[cfg(windows)]
            {
                // ConPTY expects its host terminal emulator to answer cursor queries.
                const CURSOR_POSITION_QUERY: &[u8] = b"\x1b[6n";
                while let Some(position) = output[search_from..]
                    .windows(CURSOR_POSITION_QUERY.len())
                    .position(|window| window == CURSOR_POSITION_QUERY)
                {
                    search_from += position + CURSOR_POSITION_QUERY.len();
                    responder.write_all(b"\x1b[1;1R")?;
                    responder.flush()?;
                }
                search_from =
                    search_from.max(output.len().saturating_sub(CURSOR_POSITION_QUERY.len() - 1));
            }
        }
        Ok(output)
    })
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
