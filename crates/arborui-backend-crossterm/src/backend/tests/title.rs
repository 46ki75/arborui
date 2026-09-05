use arborui_terminal::TerminalSession;

use super::*;

const PARTIAL_TITLE: &[u8] = b"\x1b[?1049h\x1b]0;ti";

fn desired() -> TerminalState {
    TerminalState {
        screen: ScreenMode::Alternate,
        title: Some(String::from("title")),
        ..TerminalState::default()
    }
}

// Model the relevant Kitty parser behavior: CSI bytes inside OSC are title
// content, not mode changes. Only BEL or ST ends the string. The ignored native
// test below checks this contract against Kitty rather than relying on the model.
#[derive(Debug, Default, Eq, PartialEq)]
enum ParserState {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

#[derive(Debug, Default)]
struct TitleParser {
    state: ParserState,
    control: Vec<u8>,
    alternate: bool,
    title: String,
    text: String,
}

impl TitleParser {
    fn parse(bytes: &[u8]) -> Self {
        use ParserState::{Csi, Escape, Ground, Osc, OscEscape};

        let mut parser = Self::default();
        for &byte in bytes {
            match parser.state {
                Osc | OscEscape => {
                    if byte == 7 || (parser.state == OscEscape && byte == b'\\') {
                        if parser.control.starts_with(b"0;") {
                            parser.title = String::from_utf8_lossy(&parser.control[2..]).into();
                        }
                        parser.state = Ground;
                    } else {
                        if parser.state == OscEscape {
                            parser.control.push(0x1b);
                        }
                        if byte != 0x1b {
                            parser.control.push(byte);
                        }
                        parser.state = if byte == 0x1b { OscEscape } else { Osc };
                    }
                }
                _ if byte == 0x1b => parser.state = Escape,
                Escape => {
                    parser.control.clear();
                    parser.state = match byte {
                        b'[' => Csi,
                        b']' => Osc,
                        _ => Ground,
                    };
                }
                Csi => {
                    parser.control.push(byte);
                    if (0x40..=0x7e).contains(&byte) {
                        match parser.control.as_slice() {
                            b"?1049h" => parser.alternate = true,
                            b"?1049l" => parser.alternate = false,
                            _ => {}
                        }
                        parser.state = Ground;
                    }
                }
                _ => parser.text.push(char::from(byte)),
            }
        }
        parser
    }

    fn assert_restored(&self) {
        assert!(!self.alternate, "alternate screen remains active: {self:?}");
        assert_eq!(self.state, ParserState::Ground);
        assert_eq!(self.title, "", "even a partially written title is owned");
    }
}

#[test]
fn partial_title_is_terminated_before_restore() -> io::Result<()> {
    let mut backend = CrosstermBackend::new(FailWriteOnceAfter {
        fail_after: Some(PARTIAL_TITLE.len()),
        ..FailWriteOnceAfter::default()
    })?
    .with_kitty_graphics(KittyGraphicsMode::Disabled);

    assert!(backend.apply_state(&desired()).is_err());
    assert_eq!(backend.writer.bytes, PARTIAL_TITLE);
    let parser = TitleParser::parse(&backend.writer.bytes);
    assert!(parser.alternate);
    assert_eq!(parser.state, ParserState::Osc);

    backend.restore()?;

    TitleParser::parse(&backend.writer.bytes).assert_restored();
    Ok(())
}

#[test]
fn partial_title_failed_open_restores_parser() -> io::Result<()> {
    let mut writer = FailWriteOnceAfter {
        fail_after: Some(PARTIAL_TITLE.len()),
        ..FailWriteOnceAfter::default()
    };
    let backend =
        CrosstermBackend::new(&mut writer)?.with_kitty_graphics(KittyGraphicsMode::Disabled);

    assert!(TerminalSession::open(backend, desired()).is_err());

    TitleParser::parse(&writer.bytes).assert_restored();
    Ok(())
}

#[test]
#[ignore = "requires Kitty; set ARBORUI_TEST_KITTY to its executable path"]
fn partial_title_native_parser() -> Result<(), Box<dyn std::error::Error>> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let mut ordinary =
        CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Disabled);
    ordinary.apply_state(&desired())?;
    let mut cases = vec![(true, "title", ordinary.writer.clone())];
    cases.push((false, "", ordinary.into_inner()?));

    for recovery_failure in [None, Some(0), Some(1), Some(2)] {
        let mut backend = CrosstermBackend::new(FailWriteOnceAfter {
            fail_after: Some(PARTIAL_TITLE.len()),
            ..FailWriteOnceAfter::default()
        })?
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
        assert!(backend.apply_state(&desired()).is_err());
        if let Some(bytes) = recovery_failure {
            if bytes < 2 {
                backend.writer.fail_after = Some(bytes);
            } else {
                backend.writer.fail_flush = true;
            }
            assert!(backend.restore().is_err());
        }
        backend.restore()?;
        cases.push((false, "", backend.writer.bytes));
    }

    for recovery_bytes in 0..=2 {
        let mut backend = CrosstermBackend::new(BufferedTitleWriter {
            output: FailWriteOnceAfter {
                fail_after: Some(PARTIAL_TITLE.len()),
                ..FailWriteOnceAfter::default()
            },
            ..BufferedTitleWriter::default()
        })?
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
        assert!(backend.apply_state(&desired()).is_err());
        if recovery_bytes < 2 {
            backend.writer.output.fail_after = Some(recovery_bytes);
        } else {
            backend.writer.output.fail_flush = true;
        }
        assert!(backend.restore().is_err());
        backend.restore()?;
        cases.push((false, "", backend.writer.output.bytes));
    }

    let mut command = std::process::Command::new(
        env::var_os("ARBORUI_TEST_KITTY").unwrap_or_else(|| "kitty".into()),
    );
    command.args(["+runpy", include_str!("title_parser.py")]);
    for (alternate, title, bytes) in cases {
        command.args([alternate.to_string(), title.into(), STANDARD.encode(bytes)]);
    }
    let output = command.output()?;
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout).contains("ARBORUI_KITTY_PARSER_OK"),
        "Kitty parser failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    Ok(())
}

#[test]
fn partial_title_failed_session_apply_restores_on_drop() -> io::Result<()> {
    let mut writer = FailWriteOnceAfter::default();
    {
        let backend =
            CrosstermBackend::new(&mut writer)?.with_kitty_graphics(KittyGraphicsMode::Disabled);
        let mut session = TerminalSession::open(
            backend,
            TerminalState {
                title: None,
                ..desired()
            },
        )?;
        session.backend_mut().writer.fail_after = Some(b"\x1b]0;ti".len());
        assert!(session.apply_state(desired()).is_err());
    }
    TitleParser::parse(&writer.bytes).assert_restored();
    Ok(())
}

#[test]
fn partial_title_retry_repairs_parser_before_modes() -> io::Result<()> {
    let mut backend = CrosstermBackend::new(FailWriteOnceAfter {
        fail_after: Some(PARTIAL_TITLE.len()),
        ..FailWriteOnceAfter::default()
    })?
    .with_kitty_graphics(KittyGraphicsMode::Disabled);
    assert!(backend.apply_state(&desired()).is_err());

    backend.apply_state(&TerminalState {
        title: Some(String::from("replacement")),
        ..TerminalState::default()
    })?;

    let parser = TitleParser::parse(&backend.writer.bytes);
    assert!(
        !parser.alternate,
        "screen exit must not be swallowed by OSC"
    );
    assert_eq!(parser.title, "replacement");
    assert_eq!(parser.state, ParserState::Ground);
    backend.restore()?;
    TitleParser::parse(&backend.writer.bytes).assert_restored();
    Ok(())
}

#[test]
fn partial_title_reverting_to_confirmed_value_after_empty_patch_is_reapplied() -> io::Result<()> {
    let mut backend = CrosstermBackend::new(FailWriteOnceAfter::default())?
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
    backend.apply_state(&desired())?;
    backend.writer.fail_after = Some(b"\x1b]0;ch".len());
    assert!(
        backend
            .apply_state(&TerminalState {
                title: Some(String::from("changed")),
                ..desired()
            })
            .is_err()
    );

    // The requested value equals the old confirmed title, but neither the
    // failed OSC nor parser recovery establishes that it is physically set.
    backend.writer.fail_after = Some(b"\x1b\\\x1b]0;ti".len());
    assert!(backend.apply_state(&desired()).is_err());
    let empty = FramePatch {
        size: Size::new(1, 1),
        runs: Vec::new(),
        cursor: CursorState::HIDDEN,
        cursor_changed: false,
        full_repaint: false,
        images: None,
    };
    backend.writer.fail_flush = true;
    assert!(backend.write_patch(&empty).is_err());
    let retry_start = backend.writer.bytes.len();
    assert_eq!(backend.write_patch(&empty)?, WriteOutcome::Applied);
    assert_eq!(&backend.writer.bytes[retry_start..], b"\x1b\\");
    backend.apply_state(&desired())?;

    assert_eq!(TitleParser::parse(&backend.writer.bytes).title, "title");
    backend.restore()?;
    TitleParser::parse(&backend.writer.bytes).assert_restored();
    Ok(())
}

#[test]
fn partial_title_apply_retries_failed_recovery() -> io::Result<()> {
    for recovery_bytes in 0..=2 {
        let mut backend = CrosstermBackend::new(FailWriteOnceAfter {
            fail_after: Some(PARTIAL_TITLE.len()),
            ..FailWriteOnceAfter::default()
        })?
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
        assert!(backend.apply_state(&desired()).is_err());
        if recovery_bytes < 2 {
            backend.writer.fail_after = Some(recovery_bytes);
        } else {
            backend.writer.fail_flush = true;
        }
        assert!(backend.apply_state(&TerminalState::default()).is_err());
        backend.apply_state(&TerminalState::default())?;
        TitleParser::parse(&backend.writer.bytes).assert_restored();
    }
    Ok(())
}

#[test]
fn partial_title_recovery_precedes_patch_and_retries() -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
    let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
        canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
        Ok(())
    })?;
    for fail_after in [None, Some(0), Some(1)] {
        let mut backend = CrosstermBackend::new(FailWriteOnceAfter {
            fail_after: Some(PARTIAL_TITLE.len()),
            ..FailWriteOnceAfter::default()
        })?
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
        assert!(backend.apply_state(&desired()).is_err());
        if fail_after.is_some() {
            backend.writer.fail_after = fail_after;
            assert!(backend.write_patch(frame.patch()).is_err());
        }
        assert_eq!(backend.write_patch(frame.patch())?, WriteOutcome::Applied);

        let parser = TitleParser::parse(&backend.writer.bytes);
        assert_eq!(parser.text, "x", "patch text must not become title content");
        assert_eq!(parser.state, ParserState::Ground);
        backend.restore()?;
        TitleParser::parse(&backend.writer.bytes).assert_restored();
    }
    Ok(())
}

#[test]
fn partial_title_restore_retries_recovery_and_title_reset_failures() -> io::Result<()> {
    // Fail before ST, between its bytes, or in the later empty-title OSC.
    for fail_after in [0, 1, b"\x1b\\\x1b[?1049l\x1b]0".len()] {
        let mut backend = CrosstermBackend::new(FailWriteOnceAfter {
            fail_after: Some(PARTIAL_TITLE.len()),
            ..FailWriteOnceAfter::default()
        })?
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
        assert!(backend.apply_state(&desired()).is_err());
        backend.writer.fail_after = Some(fail_after);
        assert!(backend.restore().is_err());

        backend.restore()?;

        TitleParser::parse(&backend.writer.bytes).assert_restored();
        let restored = backend.writer.bytes.len();
        backend.restore()?;
        assert!(!backend.writer.bytes[restored..].starts_with(b"\x1b\\"));
        TitleParser::parse(&backend.writer.bytes).assert_restored();
    }
    Ok(())
}

#[derive(Default)]
struct BufferedTitleWriter {
    pending: Vec<u8>,
    output: FailWriteOnceAfter,
}

impl Write for BufferedTitleWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // A failed flush may deliver only a prefix and lose the remainder.
        self.output.write_all(&std::mem::take(&mut self.pending))?;
        self.output.flush()
    }
}

#[test]
fn partial_title_flush_and_recovery_flush_failures_are_retryable() -> io::Result<()> {
    for recovery_bytes in 0..=2 {
        let mut backend = CrosstermBackend::new(BufferedTitleWriter {
            output: FailWriteOnceAfter {
                fail_after: Some(PARTIAL_TITLE.len()),
                ..FailWriteOnceAfter::default()
            },
            ..BufferedTitleWriter::default()
        })?
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
        assert!(backend.apply_state(&desired()).is_err());
        assert_eq!(backend.writer.output.bytes, PARTIAL_TITLE);

        if recovery_bytes < 2 {
            backend.writer.output.fail_after = Some(recovery_bytes);
        } else {
            backend.writer.output.fail_flush = true;
        }
        assert!(backend.restore().is_err());
        let retry_start = backend.writer.output.bytes.len();
        backend.restore()?;

        assert!(backend.writer.output.bytes[retry_start..].starts_with(b"\x1b\\"));
        TitleParser::parse(&backend.writer.output.bytes).assert_restored();
    }
    Ok(())
}

#[test]
fn ordinary_title_is_valid_and_restore_clears_it() -> io::Result<()> {
    let mut backend =
        CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Disabled);
    backend.apply_state(&desired())?;
    let parser = TitleParser::parse(&backend.writer);
    assert_eq!(parser.title, "title");
    assert_eq!(parser.state, ParserState::Ground);
    assert!(parser.alternate);
    assert!(!backend.writer.windows(2).any(|bytes| bytes == b"\x1b\\"));
    backend.restore()?;
    TitleParser::parse(&backend.writer).assert_restored();
    Ok(())
}
