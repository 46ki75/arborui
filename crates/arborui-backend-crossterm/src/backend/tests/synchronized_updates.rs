use super::*;

const BEGIN: &[u8] = b"\x1b[?2026h";
const END: &[u8] = b"\x1b[?2026l";
const HIDE: &[u8] = b"\x1b[?25l";
const ST: &[u8] = b"\x1b\\";
const RECOVERY: &[u8] = b"\x1b\\\x1b[?2026l";

fn cursor_patch() -> FramePatch {
    FramePatch {
        size: Size::new(1, 1),
        runs: Vec::new(),
        cursor: CursorState::HIDDEN,
        cursor_changed: true,
        full_repaint: false,
        images: None,
    }
}

fn backend() -> io::Result<CrosstermBackend<FailWriteOnceAfter>> {
    let mut backend = CrosstermBackend::new(FailWriteOnceAfter::default())?
        .with_capabilities(Capabilities {
            synchronized_updates: true,
            ..Capabilities::default()
        })
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
    backend.apply_state(&TerminalState {
        synchronized_updates: true,
        ..TerminalState::default()
    })?;
    backend.writer.bytes.clear();
    Ok(backend)
}

fn synchronized(bytes: &[u8]) -> bool {
    // Like the title parser tests, ignore CSI inside OSC/APC strings. Counting
    // Begin/End bytes alone would mistake a swallowed End for successful cleanup.
    let mut enabled = false;
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = &bytes[offset..];
        if remaining.starts_with(b"\x1b]") || remaining.starts_with(b"\x1b_") {
            let osc = remaining[1] == b']';
            offset += 2;
            while offset < bytes.len() {
                if bytes[offset..].starts_with(ST) {
                    offset += ST.len();
                    break;
                }
                if osc && bytes[offset] == 7 {
                    offset += 1;
                    break;
                }
                offset += 1;
            }
        } else {
            if remaining.starts_with(BEGIN) {
                enabled = true;
            } else if remaining.starts_with(END) {
                enabled = false;
            }
            offset += 1;
        }
    }
    enabled
}

#[test]
fn failed_sync_end_is_retried_by_restore() -> io::Result<()> {
    let mut backend = backend()?;
    backend.writer.fail_after = Some(BEGIN.len() + HIDE.len());

    assert!(backend.write_patch(&cursor_patch()).is_err());
    assert_eq!(backend.writer.bytes, [BEGIN, HIDE].concat());
    assert!(synchronized(&backend.writer.bytes));

    backend.restore()?;

    assert!(
        !synchronized(&backend.writer.bytes),
        "restore must end the synchronized update even without Kitty images"
    );
    Ok(())
}

#[test]
fn sync_parser_ignores_ends_inside_control_strings() {
    for string in [b"\x1b]0;".as_slice(), b"\x1b_G"] {
        assert!(synchronized(&[BEGIN, string, END, ST].concat()));
        assert!(!synchronized(&[BEGIN, string, END, ST, END, END].concat()));
    }
}

#[test]
fn partial_sync_begin_is_recovered() -> io::Result<()> {
    for accepted in 0..BEGIN.len() {
        let mut backend = backend()?;
        backend.writer.fail_after = Some(accepted);
        assert!(backend.write_patch(&cursor_patch()).is_err());
        assert_eq!(backend.writer.bytes, BEGIN[..accepted]);
        assert!(backend.synchronized_update_pending);

        backend.restore()?;

        assert!(backend.writer.bytes[accepted..].starts_with(RECOVERY));
        assert!(!backend.synchronized_update_pending);
        assert!(!synchronized(&backend.writer.bytes));
    }
    Ok(())
}

#[test]
fn failed_sync_recovery_is_retried_before_any_new_output() -> io::Result<()> {
    for action in ["restore", "empty", "patch", "state"] {
        for accepted in 0..=RECOVERY.len() {
            let mut backend = backend()?;
            backend.writer.fail_after = Some(BEGIN.len() + HIDE.len());
            assert!(backend.write_patch(&cursor_patch()).is_err());
            if action == "state" {
                backend.capabilities.synchronized_updates = false;
            }
            let recover = |backend: &mut CrosstermBackend<FailWriteOnceAfter>| match action {
                "restore" => backend.restore(),
                "state" => backend.apply_state(&TerminalState {
                    screen: ScreenMode::Alternate,
                    synchronized_updates: false,
                    ..TerminalState::default()
                }),
                _ => backend
                    .write_patch(&FramePatch {
                        cursor_changed: action == "patch",
                        ..cursor_patch()
                    })
                    .map(|_| ()),
            };

            let recovery_start = backend.writer.bytes.len();
            if accepted < RECOVERY.len() {
                backend.writer.fail_after = Some(accepted);
            } else {
                backend.writer.fail_flush = true;
            }
            assert!(recover(&mut backend).is_err(), "{action}, {accepted}");
            assert_eq!(
                &backend.writer.bytes[recovery_start..],
                &RECOVERY[..accepted],
                "new output must wait for recovery's successful flush"
            );
            assert!(backend.synchronized_update_pending);
            assert!(!backend.kitty.stream_uncertain());

            let retry_start = backend.writer.bytes.len();
            recover(&mut backend)?;

            let retry = &backend.writer.bytes[retry_start..];
            assert!(retry.starts_with(RECOVERY), "{action}, {accepted}");
            if action == "patch" {
                assert!(retry[RECOVERY.len()..].starts_with(BEGIN));
            } else if action == "empty" {
                assert_eq!(retry, RECOVERY);
            } else if action == "state" {
                assert!(retry[RECOVERY.len()..].starts_with(b"\x1b[?1049h"));
                assert!(!backend.active.synchronized_updates);
            }
            assert!(!backend.synchronized_update_pending);
            assert!(!synchronized(&backend.writer.bytes));
        }
    }
    Ok(())
}

#[test]
fn sync_frame_flush_failure_is_recovered() -> io::Result<()> {
    let mut backend = backend()?;
    backend.writer.fail_flush = true;
    assert!(backend.write_patch(&cursor_patch()).is_err());
    assert!(backend.synchronized_update_pending);
    let retry_start = backend.writer.bytes.len();

    backend.restore()?;

    assert!(backend.writer.bytes[retry_start..].starts_with(RECOVERY));
    assert!(!backend.synchronized_update_pending);
    assert!(!synchronized(&backend.writer.bytes));
    Ok(())
}

#[test]
fn partial_buffered_sync_frame_flush_is_recovered() -> io::Result<()> {
    let mut backend = CrosstermBackend::new(io::BufWriter::new(FailWriteOnceAfter::default()))?
        .with_capabilities(Capabilities {
            synchronized_updates: true,
            ..Capabilities::default()
        })
        .with_kitty_graphics(KittyGraphicsMode::Disabled);
    backend.apply_state(&TerminalState {
        synchronized_updates: true,
        ..TerminalState::default()
    })?;
    backend.writer.get_mut().fail_after = Some(BEGIN.len() + HIDE.len());

    assert!(backend.write_patch(&cursor_patch()).is_err());
    assert!(synchronized(&backend.writer.get_ref().bytes));
    assert!(backend.synchronized_update_pending);

    backend.restore()?;

    assert!(!synchronized(&backend.writer.get_ref().bytes));
    assert!(!backend.synchronized_update_pending);
    Ok(())
}

#[test]
fn sync_body_and_end_failures_still_attempt_end_and_flush() -> io::Result<()> {
    for accepted in [BEGIN.len() + 2, BEGIN.len() + HIDE.len()] {
        let mut backend = backend()?;
        backend.writer.fail_after = Some(accepted);
        backend.writer.fail_flush = true;

        let error = backend
            .write_patch(&cursor_patch())
            .expect_err("write fails");

        assert_eq!(error.to_string(), "injected partial write failure");
        assert!(!backend.writer.fail_flush, "flush must still be attempted");
        if accepted < BEGIN.len() + HIDE.len() {
            assert!(backend.writer.bytes.ends_with(END));
        }
        assert!(backend.synchronized_update_pending);
        backend.restore()?;
        assert!(!synchronized(&backend.writer.bytes));
    }
    Ok(())
}

#[test]
fn sync_recovery_failure_still_attempts_raw_cleanup() -> io::Result<()> {
    let mut backend = backend()?;
    backend.writer.fail_after = Some(BEGIN.len() + HIDE.len());
    assert!(backend.write_patch(&cursor_patch()).is_err());
    backend.active.raw_mode = true;
    backend.original_raw_mode = false;
    backend.writer.fail_after = Some(0);
    let mut raw_cleanup_attempted = false;

    let error = backend
        .restore_with(|| {
            raw_cleanup_attempted = true;
            Err(io::Error::other("injected raw cleanup failure"))
        })
        .expect_err("output recovery fails");

    assert!(raw_cleanup_attempted);
    assert_eq!(error.to_string(), "injected partial write failure");
    assert!(backend.synchronized_update_pending);
    backend.restore_with(|| Ok(()))?;
    assert!(!synchronized(&backend.writer.bytes));
    Ok(())
}

#[test]
fn sync_recovery_does_not_depend_on_kitty_capability() -> io::Result<()> {
    let mut backend = backend()?.with_kitty_graphics(KittyGraphicsMode::Enabled);
    backend.apply_state(&TerminalState {
        screen: ScreenMode::Alternate,
        synchronized_updates: true,
        ..TerminalState::default()
    })?;
    backend.writer.fail_after = Some(BEGIN.len() + HIDE.len());
    assert!(backend.write_patch(&cursor_patch()).is_err());
    assert!(!backend.kitty.stream_uncertain());

    backend.restore()?;

    assert!(!synchronized(&backend.writer.bytes));
    assert!(
        !backend
            .writer
            .bytes
            .windows(3)
            .any(|bytes| bytes == b"\x1b_G")
    );
    Ok(())
}

#[test]
fn sync_recovery_escapes_partial_apc_without_forgetting_kitty_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let mut backend = backend()?.with_kitty_graphics(KittyGraphicsMode::Enabled);
    backend.apply_state(&TerminalState {
        screen: ScreenMode::Alternate,
        synchronized_updates: true,
        ..TerminalState::default()
    })?;
    let image = RgbaImage::new(1, 1, vec![0; 4])?;
    let patch = FramePatch {
        images: Some(ImageScene::from_placements([ImagePlacement::new(
            image,
            Rect::new(0, 0, 1, 1),
        )])),
        ..cursor_patch()
    };
    backend.writer.fail_after = Some(BEGIN.len() + b"\x1b[1;1H\x1b_Ga=T,".len());
    assert!(backend.write_patch(&patch).is_err());
    assert!(backend.writer.bytes.ends_with(END));
    assert!(synchronized(&backend.writer.bytes), "APC swallows the End");
    let ids = backend.kitty.cleanup_ids();
    assert!(!ids.is_empty());

    backend.write_patch(&FramePatch {
        cursor_changed: false,
        ..cursor_patch()
    })?;

    assert!(!synchronized(&backend.writer.bytes));
    assert_eq!(backend.kitty.cleanup_ids(), ids);
    assert!(backend.kitty.stream_uncertain());
    backend = backend.with_kitty_graphics(KittyGraphicsMode::Disabled);
    let restore_start = backend.writer.bytes.len();
    backend.restore()?;
    let restored = &backend.writer.bytes[restore_start..];
    let leave = restored
        .windows(8)
        .position(|bytes| bytes == b"\x1b[?1049l")
        .expect("restore leaves alternate screen");
    for id in ids {
        let deletion = format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\");
        assert!(
            restored[..leave]
                .windows(deletion.len())
                .any(|bytes| bytes == deletion.as_bytes())
        );
    }
    assert!(backend.kitty.cleanup_ids().is_empty());
    assert!(!backend.kitty.stream_uncertain());
    assert!(!synchronized(&backend.writer.bytes));
    Ok(())
}

#[test]
fn sync_recovery_preserves_unconfirmed_title_value() -> io::Result<()> {
    let mut backend = backend()?;
    let desired = TerminalState {
        title: Some(String::from("title")),
        synchronized_updates: true,
        ..TerminalState::default()
    };
    backend.apply_state(&desired)?;
    backend.writer.fail_after = Some(b"\x1b]0;ch".len());
    assert!(
        backend
            .apply_state(&TerminalState {
                title: Some(String::from("changed")),
                ..desired.clone()
            })
            .is_err()
    );

    backend.writer.fail_after = Some(ST.len() + BEGIN.len() + HIDE.len());
    assert!(backend.write_patch(&cursor_patch()).is_err());
    assert!(synchronized(&backend.writer.bytes));
    let retry_start = backend.writer.bytes.len();
    backend.write_patch(&FramePatch {
        cursor_changed: false,
        ..cursor_patch()
    })?;

    assert_eq!(&backend.writer.bytes[retry_start..], RECOVERY);
    assert!(!synchronized(&backend.writer.bytes));
    assert!(backend.title_unconfirmed);
    assert!(!backend.lifecycle_stream_uncertain);
    backend.apply_state(&desired)?;
    assert!(backend.writer.bytes.ends_with(b"\x1b]0;title\x07"));
    assert!(!backend.title_unconfirmed);
    let restore_start = backend.writer.bytes.len();
    backend.restore()?;
    assert!(
        backend.writer.bytes[restore_start..]
            .windows(5)
            .any(|bytes| bytes == b"\x1b]0;\x07")
    );
    Ok(())
}

#[test]
fn invalid_patch_does_not_attempt_pending_sync_recovery() -> io::Result<()> {
    let mut backend = backend()?;
    backend.writer.fail_after = Some(BEGIN.len() + HIDE.len());
    assert!(backend.write_patch(&cursor_patch()).is_err());
    let start = backend.writer.bytes.len();
    backend.writer.fail_flush = true;

    let error = backend
        .write_patch(&FramePatch {
            full_repaint: true,
            ..cursor_patch()
        })
        .expect_err("full repaint requires cells");

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(backend.writer.bytes.len(), start);
    assert!(backend.writer.fail_flush);
    assert!(backend.synchronized_update_pending);
    Ok(())
}

#[test]
fn clean_empty_patch_neither_arms_sync_nor_flushes() -> io::Result<()> {
    for mode in [KittyGraphicsMode::Disabled, KittyGraphicsMode::Enabled] {
        for images in [None, Some(ImageScene::new())] {
            let mut backend = backend()?.with_kitty_graphics(mode);
            backend.apply_state(&TerminalState {
                screen: ScreenMode::Alternate,
                synchronized_updates: true,
                ..TerminalState::default()
            })?;
            for after_frame in [false, true] {
                if after_frame {
                    backend.writer.fail_flush = false;
                    backend.write_patch(&cursor_patch())?;
                }
                let start = backend.writer.bytes.len();
                backend.writer.fail_flush = true;
                backend.write_patch(&FramePatch {
                    cursor_changed: false,
                    images: images.clone(),
                    ..cursor_patch()
                })?;
                assert_eq!(backend.writer.bytes.len(), start);
                assert!(backend.writer.fail_flush);
                assert!(!backend.synchronized_update_pending);
            }
        }
    }
    Ok(())
}

#[test]
fn unsynchronized_failure_does_not_arm_sync_recovery() -> io::Result<()> {
    let mut backend = backend()?;
    backend.apply_state(&TerminalState::default())?;
    backend.writer.fail_after = Some(0);
    assert!(backend.write_patch(&cursor_patch()).is_err());
    assert!(!backend.synchronized_update_pending);
    assert!(!backend.kitty.stream_uncertain());
    backend.writer.fail_flush = true;
    let start = backend.writer.bytes.len();
    backend.write_patch(&FramePatch {
        cursor_changed: false,
        ..cursor_patch()
    })?;
    assert_eq!(backend.writer.bytes.len(), start);
    assert!(backend.writer.fail_flush);
    Ok(())
}
