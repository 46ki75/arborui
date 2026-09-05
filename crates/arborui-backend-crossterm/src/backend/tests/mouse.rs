use arborui_terminal::TerminalSession;

use super::*;

const CAPTURE: &[u8] = b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h";
const DISABLE: &[u8] = b"\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l";

fn desired(mouse: MouseMode) -> TerminalState {
    TerminalState {
        mouse,
        ..TerminalState::default()
    }
}

#[test]
fn partial_mouse_enable_is_restored() -> io::Result<()> {
    for boundary in [8, 16, 24, 32] {
        let mut writer = FailWriteOnceAfter {
            fail_after: Some(boundary),
            ..FailWriteOnceAfter::default()
        };
        let backend = CrosstermBackend::new(&mut writer)?;

        assert!(TerminalSession::open(backend, desired(MouseMode::Capture)).is_err());
        assert_eq!(&writer.bytes[..boundary], &CAPTURE[..boundary]);

        let cleanup = &writer.bytes[boundary..];
        for mode in [1000, 1002, 1003, 1015, 1006] {
            let inverse = format!("\x1b[?{mode}l");
            assert!(
                cleanup
                    .windows(inverse.len())
                    .any(|bytes| bytes == inverse.as_bytes()),
                "failed-open cleanup after {boundary} bytes must disable mouse mode {mode}"
            );
        }
    }
    Ok(())
}

#[test]
fn failed_mouse_enable_retries_requested_mode() -> io::Result<()> {
    for boundary in [8, 16, 24, 32, 40] {
        for (mouse, command) in [
            (MouseMode::Capture, CAPTURE),
            (MouseMode::Disabled, DISABLE),
        ] {
            let mut backend = CrosstermBackend::new(FailWriteOnceAfter {
                fail_after: (boundary < CAPTURE.len()).then_some(boundary),
                fail_flush: boundary == CAPTURE.len(),
                ..FailWriteOnceAfter::default()
            })?;
            assert!(backend.apply_state(&desired(MouseMode::Capture)).is_err());
            assert_eq!(backend.writer.bytes, &CAPTURE[..boundary]);

            let retry_start = backend.writer.bytes.len();
            backend.apply_state(&desired(mouse))?;
            assert_eq!(&backend.writer.bytes[retry_start..], command);
            let confirmed = backend.writer.bytes.len();
            backend.apply_state(&desired(mouse))?;
            assert_eq!(backend.writer.bytes.len(), confirmed);
            backend.restore()?;
        }
    }
    Ok(())
}

#[test]
fn partial_mouse_enable_cleanup_failures_are_retryable() -> io::Result<()> {
    for boundary in [8, 16, 24, 32] {
        let mut backend = CrosstermBackend::new(FailWriteOnceAfter {
            fail_after: Some(boundary),
            ..FailWriteOnceAfter::default()
        })?;
        assert!(backend.apply_state(&desired(MouseMode::Capture)).is_err());

        // Keep retrying through partial disables, a later cleanup write, and flush.
        for fail_after in [
            Some(0),
            Some(8),
            Some(16),
            Some(24),
            Some(32),
            Some(40),
            None,
        ] {
            backend.writer.fail_after = fail_after;
            backend.writer.fail_flush = fail_after.is_none();
            let cleanup_start = backend.writer.bytes.len();
            assert!(backend.restore().is_err());
            let written = fail_after.unwrap_or(DISABLE.len());
            assert_eq!(
                &backend.writer.bytes[cleanup_start..cleanup_start + written],
                &DISABLE[..written],
            );
        }

        let retry_start = backend.writer.bytes.len();
        backend.restore()?;
        assert!(backend.writer.bytes[retry_start..].starts_with(DISABLE));
        let confirmed = backend.writer.bytes.len();
        backend.restore()?;
        assert!(
            !backend.writer.bytes[confirmed..]
                .windows(DISABLE.len())
                .any(|bytes| bytes == DISABLE)
        );
    }
    Ok(())
}

#[test]
fn failed_mouse_disable_retries_requested_mode() -> io::Result<()> {
    for restoring in [false, true] {
        for fail_after in [Some(0), Some(8), Some(16), Some(24), Some(32), None] {
            for (mouse, command) in [
                (MouseMode::Capture, CAPTURE),
                (MouseMode::Disabled, DISABLE),
            ] {
                let mut backend = CrosstermBackend::new(FailWriteOnceAfter::default())?;
                backend.apply_state(&desired(MouseMode::Capture))?;
                backend.writer.fail_after = fail_after;
                backend.writer.fail_flush = fail_after.is_none();

                let result = if restoring {
                    backend.restore()
                } else {
                    backend.apply_state(&desired(MouseMode::Disabled))
                };
                assert!(result.is_err());

                let retry_start = backend.writer.bytes.len();
                backend.apply_state(&desired(mouse))?;
                assert_eq!(&backend.writer.bytes[retry_start..], command);
                let confirmed = backend.writer.bytes.len();
                backend.apply_state(&desired(mouse))?;
                assert_eq!(backend.writer.bytes.len(), confirmed);
                backend.restore()?;
            }
        }
    }
    Ok(())
}

#[test]
fn mouse_cleanup_survives_title_cleanup_and_parser_recovery() -> io::Result<()> {
    let mut backend = CrosstermBackend::new(FailWriteOnceAfter::default())?
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
    let state = TerminalState {
        title: Some(String::from("title")),
        ..TerminalState::default()
    };
    backend.apply_state(&state)?;
    backend.writer.fail_after = Some(8);
    assert!(
        backend
            .apply_state(&TerminalState {
                mouse: MouseMode::Capture,
                ..state
            })
            .is_err()
    );

    backend.writer.fail_after = Some(DISABLE.len() + b"\x1b]0;".len());
    assert!(backend.restore().is_err());
    assert!(backend.title_unconfirmed);
    assert!(backend.lifecycle_stream_uncertain);
    let empty = FramePatch {
        size: Size::new(1, 1),
        runs: Vec::new(),
        cursor: CursorState::HIDDEN,
        cursor_changed: false,
        full_repaint: false,
        images: None,
    };
    let recovery_start = backend.writer.bytes.len();
    assert_eq!(backend.write_patch(&empty)?, WriteOutcome::Applied);
    assert_eq!(&backend.writer.bytes[recovery_start..], b"\x1b\\");

    // Flushing parser recovery does not confirm either the mouse mode or title.
    let retry_start = backend.writer.bytes.len();
    backend.apply_state(&TerminalState::default())?;
    assert_eq!(
        &backend.writer.bytes[retry_start..],
        [DISABLE, b"\x1b]0;\x07"].concat(),
    );
    assert!(!backend.title_unconfirmed);
    assert!(!backend.lifecycle_stream_uncertain);
    Ok(())
}

#[test]
fn confirmed_mouse_modes_do_not_emit_repeated_commands() -> io::Result<()> {
    let mut backend = CrosstermBackend::new(Vec::new())?;
    backend.apply_state(&TerminalState::default())?;
    assert!(backend.writer.is_empty());

    for (mouse, command) in [
        (MouseMode::Capture, CAPTURE),
        (MouseMode::Disabled, DISABLE),
    ] {
        let start = backend.writer.len();
        backend.apply_state(&desired(mouse))?;
        backend.apply_state(&desired(mouse))?;
        assert_eq!(&backend.writer[start..], command);
    }
    let start = backend.writer.len();
    backend.restore()?;
    backend.restore()?;
    assert!(
        !backend.writer[start..]
            .windows(DISABLE.len())
            .any(|bytes| bytes == DISABLE)
    );
    Ok(())
}

#[test]
fn successful_mouse_cleanup_is_confirmed_despite_raw_mode_failure() -> io::Result<()> {
    let mut backend = CrosstermBackend::new(Vec::new())?;
    backend.apply_state(&desired(MouseMode::Capture))?;
    // Inject raw-mode state without changing the test process's terminal.
    backend.original_raw_mode = false;
    backend.active.raw_mode = true;
    backend.confirmed.raw_mode = true;

    let cleanup_start = backend.writer.len();
    assert!(
        backend
            .restore_with(|| Err(io::Error::other("injected raw-mode failure")))
            .is_err()
    );
    assert!(backend.writer[cleanup_start..].starts_with(DISABLE));
    assert_eq!(backend.active.mouse, MouseMode::Disabled);
    assert_eq!(backend.confirmed.mouse, MouseMode::Disabled);
    assert!(backend.active.raw_mode);

    let retry_start = backend.writer.len();
    backend.restore_with(|| Ok(()))?;
    assert!(
        !backend.writer[retry_start..]
            .windows(DISABLE.len())
            .any(|bytes| bytes == DISABLE)
    );
    let activation_start = backend.writer.len();
    backend.apply_state(&desired(MouseMode::Capture))?;
    assert_eq!(&backend.writer[activation_start..], CAPTURE);
    backend.restore()?;
    Ok(())
}

#[test]
fn failed_mouse_cleanup_is_not_confirmed_by_raw_mode_success() -> io::Result<()> {
    let mut backend = CrosstermBackend::new(FailWriteOnceAfter {
        fail_after: Some(8),
        ..FailWriteOnceAfter::default()
    })?;
    assert!(backend.apply_state(&desired(MouseMode::Capture)).is_err());
    backend.original_raw_mode = false;
    backend.active.raw_mode = true;
    backend.confirmed.raw_mode = true;
    backend.writer.fail_flush = true;

    assert!(backend.restore_with(|| Ok(())).is_err());
    assert!(!backend.active.raw_mode);
    let retry_start = backend.writer.bytes.len();
    backend.restore_with(|| panic!("raw mode was already restored"))?;
    assert!(backend.writer.bytes[retry_start..].starts_with(DISABLE));
    Ok(())
}
