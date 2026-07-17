use std::{env, io, io::Write, time::Duration};

use arborui_core::{CursorState, CursorVisibility, Size};
use arborui_render::FramePatch;
use arborui_terminal::{
    AutowrapMode, Capabilities, ColorCapability, KeyboardCapability, KeyboardMode, MouseCapability,
    MouseMode, ScreenMode, TerminalBackend, TerminalEvent, TerminalState, WriteOutcome,
};
use crossterm::{
    QueueableCommand,
    cursor::{Hide, SetCursorStyle, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    style::{Attribute, Color, SetAttribute, SetBackgroundColor, SetForegroundColor},
    terminal::{
        DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
        disable_raw_mode, enable_raw_mode, is_raw_mode_enabled,
    },
};

use crate::{events::translate_event, output};

/// Crossterm-backed terminal input, output, and lifecycle implementation.
pub struct CrosstermBackend<W: Write + Send> {
    writer: W,
    capabilities: Capabilities,
    active: TerminalState,
    confirmed: TerminalState,
    keyboard_pushed: bool,
    original_raw_mode: bool,
}

impl<W: Write + Send> CrosstermBackend<W> {
    /// Creates a backend using conservative environment-based capabilities.
    pub fn new(writer: W) -> io::Result<Self> {
        let original_raw_mode = is_raw_mode_enabled()?;
        let active = TerminalState {
            raw_mode: original_raw_mode,
            ..TerminalState::default()
        };
        Ok(Self {
            writer,
            capabilities: detect_capabilities(),
            confirmed: active.clone(),
            active,
            keyboard_pushed: false,
            original_raw_mode,
        })
    }

    /// Overrides detected capabilities implemented by this backend.
    ///
    /// Unsupported output features remain disabled even when requested.
    #[must_use]
    pub fn with_capabilities(mut self, mut capabilities: Capabilities) -> Self {
        capabilities.hyperlinks = false;
        self.capabilities = capabilities;
        self
    }

    /// Restores terminal state and returns the wrapped output writer.
    pub fn into_inner(mut self) -> io::Result<W> {
        self.restore()?;
        Ok(self.writer)
    }

    fn effective_state(&self, desired: &TerminalState) -> TerminalState {
        let mut effective = desired.clone();
        effective.raw_mode |= self.original_raw_mode;
        if self.capabilities.mouse == MouseCapability::None {
            effective.mouse = MouseMode::Disabled;
        }
        if self.capabilities.keyboard == KeyboardCapability::Legacy {
            effective.keyboard = KeyboardMode::Legacy;
        }
        effective.bracketed_paste &= self.capabilities.bracketed_paste;
        effective.focus_reporting &= self.capabilities.focus_reporting;
        effective.synchronized_updates &= self.capabilities.synchronized_updates;
        effective
    }

    fn apply_cursor(&mut self, cursor: CursorState) -> io::Result<()> {
        if cursor.visibility == CursorVisibility::Hidden {
            self.writer.queue(Hide)?;
        } else {
            output::apply_cursor(&mut self.writer, cursor)?;
        }
        self.active.cursor = cursor;
        Ok(())
    }

    fn restore_with<F>(&mut self, disable_raw: F) -> io::Result<()>
    where
        F: FnOnce() -> io::Result<()>,
    {
        let output_result = (|| -> io::Result<()> {
            if self.keyboard_pushed {
                self.writer.queue(PopKeyboardEnhancementFlags)?;
                self.keyboard_pushed = false;
            }
            if self.active.mouse == MouseMode::Capture || self.confirmed.mouse == MouseMode::Capture
            {
                self.writer.queue(DisableMouseCapture)?;
            }
            if self.active.bracketed_paste || self.confirmed.bracketed_paste {
                self.writer.queue(DisableBracketedPaste)?;
            }
            if self.active.focus_reporting || self.confirmed.focus_reporting {
                self.writer.queue(DisableFocusChange)?;
            }
            if self.active.screen == ScreenMode::Alternate
                || self.confirmed.screen == ScreenMode::Alternate
            {
                self.writer.queue(LeaveAlternateScreen)?;
            }
            if self.active.autowrap == AutowrapMode::Disabled
                || self.confirmed.autowrap == AutowrapMode::Disabled
            {
                self.writer.queue(EnableLineWrap)?;
            }
            if self.active.title.is_some() || self.confirmed.title.is_some() {
                self.writer.queue(SetTitle(""))?;
            }
            self.writer.queue(SetAttribute(Attribute::Reset))?;
            self.writer.queue(SetForegroundColor(Color::Reset))?;
            self.writer.queue(SetBackgroundColor(Color::Reset))?;
            self.writer.queue(SetCursorStyle::DefaultUserShape)?;
            self.writer.queue(Show)?;
            self.writer.flush()
        })();

        if output_result.is_ok() {
            // Output state is physically restored even if the separate raw-mode
            // operation fails, so a later activation must reapply every mode.
            self.active = TerminalState {
                raw_mode: self.active.raw_mode,
                ..TerminalState::default()
            };
            self.confirmed = self.active.clone();
        }

        let raw_result = if self.active.raw_mode && !self.original_raw_mode {
            let result = disable_raw();
            if result.is_ok() {
                self.active.raw_mode = false;
                self.confirmed.raw_mode = false;
            }
            result
        } else {
            Ok(())
        };

        if output_result.is_err() {
            self.confirmed = TerminalState {
                raw_mode: self.active.raw_mode,
                ..TerminalState::default()
            };
        }
        let result = output_result.and(raw_result);
        if result.is_ok() {
            self.active = TerminalState {
                raw_mode: self.original_raw_mode,
                ..TerminalState::default()
            };
            self.confirmed = self.active.clone();
        }
        result
    }
}

impl<W: Write + Send> TerminalBackend for CrosstermBackend<W> {
    type Error = io::Error;

    fn size(&self) -> Result<Size, Self::Error> {
        let (width, height) = crossterm::terminal::size()?;
        Ok(Size::new(width, height))
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn poll_event(&mut self, timeout: Duration) -> Result<Option<TerminalEvent>, Self::Error> {
        if !crossterm::event::poll(timeout)? {
            return Ok(None);
        }
        Ok(Some(translate_event(crossterm::event::read()?)))
    }

    fn apply_state(&mut self, desired: &TerminalState) -> Result<(), Self::Error> {
        let desired = self.effective_state(desired);

        if desired.raw_mode && !self.active.raw_mode {
            enable_raw_mode()?;
            self.active.raw_mode = true;
        }
        if state_changed(desired.screen, self.active.screen, self.confirmed.screen) {
            match desired.screen {
                ScreenMode::Main => self.writer.queue(LeaveAlternateScreen)?,
                ScreenMode::Alternate => self.writer.queue(EnterAlternateScreen)?,
            };
            self.active.screen = desired.screen;
        }
        if state_changed(desired.mouse, self.active.mouse, self.confirmed.mouse) {
            match desired.mouse {
                MouseMode::Disabled => self.writer.queue(DisableMouseCapture)?,
                MouseMode::Capture => self.writer.queue(EnableMouseCapture)?,
            };
            self.active.mouse = desired.mouse;
        }
        if state_changed(
            desired.focus_reporting,
            self.active.focus_reporting,
            self.confirmed.focus_reporting,
        ) {
            if desired.focus_reporting {
                self.writer.queue(EnableFocusChange)?;
            } else {
                self.writer.queue(DisableFocusChange)?;
            }
            self.active.focus_reporting = desired.focus_reporting;
        }
        if state_changed(
            desired.bracketed_paste,
            self.active.bracketed_paste,
            self.confirmed.bracketed_paste,
        ) {
            if desired.bracketed_paste {
                self.writer.queue(EnableBracketedPaste)?;
            } else {
                self.writer.queue(DisableBracketedPaste)?;
            }
            self.active.bracketed_paste = desired.bracketed_paste;
        }
        if state_changed(
            desired.keyboard,
            self.active.keyboard,
            self.confirmed.keyboard,
        ) {
            match desired.keyboard {
                KeyboardMode::Legacy if self.keyboard_pushed => {
                    self.writer.queue(PopKeyboardEnhancementFlags)?;
                    self.keyboard_pushed = false;
                }
                KeyboardMode::Enhanced if !self.keyboard_pushed => {
                    self.writer.queue(PushKeyboardEnhancementFlags(
                        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                            | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                            | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES,
                    ))?;
                    self.keyboard_pushed = true;
                }
                KeyboardMode::Legacy | KeyboardMode::Enhanced => {}
            }
            self.active.keyboard = desired.keyboard;
        }
        if state_changed(
            desired.autowrap,
            self.active.autowrap,
            self.confirmed.autowrap,
        ) {
            match desired.autowrap {
                AutowrapMode::Disabled => self.writer.queue(DisableLineWrap)?,
                AutowrapMode::Enabled | AutowrapMode::Preserve => {
                    self.writer.queue(EnableLineWrap)?
                }
            };
            self.active.autowrap = desired.autowrap;
        }
        if state_changed(&desired.title, &self.active.title, &self.confirmed.title) {
            let title = sanitized_title(desired.title.as_deref().unwrap_or_default());
            self.writer.queue(SetTitle(title))?;
            self.active.title.clone_from(&desired.title);
        }
        if state_changed(desired.cursor, self.active.cursor, self.confirmed.cursor) {
            self.apply_cursor(desired.cursor)?;
        }
        self.active.synchronized_updates = desired.synchronized_updates;
        self.writer.flush()?;
        self.confirmed = desired.clone();
        self.confirmed.raw_mode = self.active.raw_mode;

        if !desired.raw_mode && self.active.raw_mode {
            disable_raw_mode()?;
            self.active.raw_mode = false;
            self.confirmed.raw_mode = false;
        }
        Ok(())
    }

    fn write_patch(&mut self, patch: &FramePatch) -> Result<WriteOutcome, Self::Error> {
        output::write_patch(
            &mut self.writer,
            patch,
            &Capabilities {
                synchronized_updates: self.active.synchronized_updates,
                ..self.capabilities
            },
        )?;
        if !patch.is_empty() {
            self.active.cursor = patch.cursor;
            self.confirmed.cursor = patch.cursor;
        }
        Ok(WriteOutcome::Applied)
    }

    fn restore(&mut self) -> Result<(), Self::Error> {
        self.restore_with(disable_raw_mode)
    }
}

fn state_changed<T: PartialEq>(desired: T, active: T, confirmed: T) -> bool {
    desired != active || desired != confirmed
}

fn sanitized_title(title: &str) -> String {
    title
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

fn detect_capabilities() -> Capabilities {
    let color = match env::var("COLORTERM") {
        Ok(value)
            if value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit") =>
        {
            ColorCapability::TrueColor
        }
        _ if env::var("TERM").is_ok_and(|value| value.contains("256color")) => {
            ColorCapability::Ansi256
        }
        _ => ColorCapability::Ansi16,
    };

    Capabilities {
        color,
        ..Capabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use arborui_core::{Point, Style};
    use arborui_render::Renderer;
    use arborui_text::WidthPolicy;

    use super::*;

    #[test]
    fn writes_frame_patch_to_wrapped_writer() -> Result<(), Box<dyn std::error::Error>> {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;
        let mut backend = CrosstermBackend::new(Vec::new())?;

        assert_eq!(backend.write_patch(frame.patch())?, WriteOutcome::Applied);
        assert!(backend.into_inner()?.contains(&b'x'));
        Ok(())
    }

    #[test]
    fn configured_capabilities_are_reported() -> io::Result<()> {
        let capabilities = Capabilities {
            synchronized_updates: true,
            ..Capabilities::default()
        };
        let backend = CrosstermBackend::new(Vec::new())?.with_capabilities(capabilities);

        assert_eq!(backend.capabilities(), &capabilities);
        Ok(())
    }

    #[test]
    fn unsupported_hyperlink_capability_remains_disabled() -> io::Result<()> {
        let backend = CrosstermBackend::new(Vec::new())?.with_capabilities(Capabilities {
            hyperlinks: true,
            ..Capabilities::default()
        });

        assert!(!backend.capabilities().hyperlinks);
        Ok(())
    }

    #[test]
    fn applies_and_restores_owned_terminal_modes() -> io::Result<()> {
        let capabilities = Capabilities {
            synchronized_updates: true,
            ..Capabilities::default()
        };
        let mut backend = CrosstermBackend::new(Vec::new())?.with_capabilities(capabilities);
        let desired = TerminalState {
            cursor: CursorState::HIDDEN,
            mouse: MouseMode::Capture,
            bracketed_paste: true,
            focus_reporting: true,
            synchronized_updates: true,
            autowrap: AutowrapMode::Disabled,
            title: Some(String::from("arborui test")),
            ..TerminalState::default()
        };

        backend.apply_state(&desired)?;
        backend.restore()?;
        let output = backend.into_inner()?;

        assert!(output.windows(8).any(|window| window == b"\x1b[?1003h"));
        assert!(output.windows(8).any(|window| window == b"\x1b[?1003l"));
        assert!(output.windows(8).any(|window| window == b"\x1b[?2004h"));
        assert!(output.windows(8).any(|window| window == b"\x1b[?2004l"));
        Ok(())
    }

    #[derive(Default)]
    struct FailFlushOnce {
        bytes: Vec<u8>,
        flushes: usize,
        fail_on_flush: usize,
    }

    impl Write for FailFlushOnce {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            if self.flushes == self.fail_on_flush {
                return Err(io::Error::other("injected flush failure"));
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct BufferedFailFlushOnce {
        pending: Vec<u8>,
        flushed: Vec<u8>,
        flushes: usize,
        fail_on_flush: usize,
    }

    impl Write for BufferedFailFlushOnce {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.pending.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            if self.flushes == self.fail_on_flush {
                return Err(io::Error::other("injected flush failure"));
            }
            self.flushed.append(&mut self.pending);
            Ok(())
        }
    }

    #[test]
    fn empty_patch_does_not_update_tracked_cursor_state() -> io::Result<()> {
        let mut backend = CrosstermBackend::new(Vec::new())?;
        backend.apply_state(&TerminalState {
            cursor: CursorState::visible(Point::ORIGIN),
            ..TerminalState::default()
        })?;

        let empty = FramePatch {
            size: Size::new(1, 1),
            runs: Vec::new(),
            cursor: CursorState::HIDDEN,
            cursor_changed: false,
            full_repaint: false,
        };
        assert_eq!(backend.write_patch(&empty)?, WriteOutcome::Applied);

        backend.apply_state(&TerminalState {
            cursor: CursorState::HIDDEN,
            ..TerminalState::default()
        })?;
        let output = backend.into_inner()?;
        let hide: &[u8] = b"\x1b[?25l";
        assert!(
            output.windows(hide.len()).any(|window| window == hide),
            "the empty patch emitted no bytes, so hiding the cursor afterwards must \
             still send a hide sequence"
        );
        Ok(())
    }

    #[test]
    fn malformed_full_repaint_is_not_applied_or_written() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |_| Ok(()))?;
        let mut malformed = frame.patch().clone();
        malformed.runs.clear();
        let mut backend = CrosstermBackend::new(Vec::new())?.with_capabilities(Capabilities {
            synchronized_updates: true,
            ..Capabilities::default()
        });

        let error = backend
            .write_patch(&malformed)
            .expect_err("an incomplete full repaint must not be reported as applied");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(backend.writer.is_empty());
        Ok(())
    }

    #[test]
    fn failed_apply_state_flush_is_resent_on_retry() -> io::Result<()> {
        let writer = FailFlushOnce {
            fail_on_flush: 1,
            ..FailFlushOnce::default()
        };
        let mut backend = CrosstermBackend::new(writer)?;
        let desired = TerminalState {
            screen: ScreenMode::Alternate,
            ..TerminalState::default()
        };

        assert!(backend.apply_state(&desired).is_err());
        backend.apply_state(&desired)?;
        let output = backend.into_inner()?;
        let enter_alternate: &[u8] = b"\x1b[?1049h";
        let enters = output
            .bytes
            .windows(enter_alternate.len())
            .filter(|window| *window == enter_alternate)
            .count();
        assert_eq!(
            enters, 2,
            "a failed flush leaves delivery unconfirmed, so retrying the same desired \
             state must re-send the mode changes"
        );
        Ok(())
    }

    #[test]
    fn failed_restore_keeps_active_state_for_retry() -> io::Result<()> {
        let writer = FailFlushOnce {
            fail_on_flush: 2,
            ..FailFlushOnce::default()
        };
        let mut backend = CrosstermBackend::new(writer)?;
        let desired = TerminalState {
            screen: ScreenMode::Alternate,
            cursor: CursorState::HIDDEN,
            ..TerminalState::default()
        };
        backend.apply_state(&desired)?;

        assert!(backend.restore().is_err());
        assert_eq!(backend.active.screen, ScreenMode::Alternate);
        backend.restore()?;
        assert_eq!(backend.active, TerminalState::default());
        Ok(())
    }

    #[test]
    fn failed_restore_flush_requires_screen_mode_reapplication() -> io::Result<()> {
        let writer = BufferedFailFlushOnce {
            fail_on_flush: 2,
            ..BufferedFailFlushOnce::default()
        };
        let mut backend = CrosstermBackend::new(writer)?;
        let desired = TerminalState {
            screen: ScreenMode::Alternate,
            ..TerminalState::default()
        };
        backend.apply_state(&desired)?;

        assert!(backend.restore().is_err());
        backend.apply_state(&desired)?;

        let enter_alternate: &[u8] = b"\x1b[?1049h";
        let leave_alternate: &[u8] = b"\x1b[?1049l";
        let last_enter = backend
            .writer
            .flushed
            .windows(enter_alternate.len())
            .rposition(|window| window == enter_alternate);
        let last_leave = backend
            .writer
            .flushed
            .windows(leave_alternate.len())
            .rposition(|window| window == leave_alternate);
        assert!(
            matches!((last_enter, last_leave), (Some(enter), Some(leave)) if enter > leave),
            "a failed restore flush leaves a queued leave-alternate-screen sequence, so the \
             tracked screen mode must be invalidated and re-entered before output resumes"
        );
        Ok(())
    }

    #[test]
    fn successful_restore_output_is_reapplied_after_raw_mode_failure() -> io::Result<()> {
        let active = TerminalState {
            raw_mode: true,
            screen: ScreenMode::Alternate,
            ..TerminalState::default()
        };
        let mut backend = CrosstermBackend {
            writer: Vec::new(),
            capabilities: Capabilities::default(),
            active: active.clone(),
            confirmed: active,
            keyboard_pushed: false,
            original_raw_mode: false,
        };

        assert!(
            backend
                .restore_with(|| Err(io::Error::other("injected raw-mode failure")))
                .is_err()
        );
        backend.apply_state(&TerminalState {
            raw_mode: true,
            screen: ScreenMode::Alternate,
            ..TerminalState::default()
        })?;

        let enter_alternate: &[u8] = b"\x1b[?1049h";
        let leave_alternate: &[u8] = b"\x1b[?1049l";
        let last_enter = backend
            .writer
            .windows(enter_alternate.len())
            .rposition(|window| window == enter_alternate);
        let last_leave = backend
            .writer
            .windows(leave_alternate.len())
            .rposition(|window| window == leave_alternate);
        assert!(
            matches!((last_enter, last_leave), (Some(enter), Some(leave)) if enter > leave),
            "successful restore output physically left the alternate screen, so activation must \
             enter it again even when disabling raw mode failed"
        );
        Ok(())
    }

    #[test]
    fn keyboard_stack_commands_are_not_repeated_after_failed_flushes() -> io::Result<()> {
        let writer = FailFlushOnce {
            fail_on_flush: 1,
            ..FailFlushOnce::default()
        };
        let capabilities = Capabilities {
            keyboard: KeyboardCapability::Enhanced,
            ..Capabilities::default()
        };
        let mut backend = CrosstermBackend::new(writer)?.with_capabilities(capabilities);
        let desired = TerminalState {
            keyboard: KeyboardMode::Enhanced,
            screen: ScreenMode::Alternate,
            ..TerminalState::default()
        };

        assert!(backend.apply_state(&desired).is_err());
        backend.apply_state(&desired)?;
        backend.restore()?;
        let output = backend.into_inner()?;
        let pushes = output
            .bytes
            .windows(4)
            .filter(|window| window.starts_with(b"\x1b[>"))
            .count();
        let pops = output
            .bytes
            .windows(3)
            .filter(|window| *window == b"\x1b[<")
            .count();
        assert_eq!(pushes, 1);
        assert_eq!(pops, 1);
        Ok(())
    }

    #[test]
    fn terminal_titles_cannot_inject_control_sequences() -> io::Result<()> {
        let mut backend = CrosstermBackend::new(Vec::new())?;
        backend.apply_state(&TerminalState {
            title: Some(String::from("safe\x07\x1b]2;unsafe")),
            ..TerminalState::default()
        })?;
        let output = backend.into_inner()?;

        let injection: &[u8] = b"\x07\x1b]2;unsafe";
        assert!(
            !output
                .windows(injection.len())
                .any(|window| window == injection),
            "BEL and ESC from the requested title must not terminate its OSC sequence"
        );
        assert!(output.windows(10).any(|window| window == b"safe]2;uns"));
        Ok(())
    }

    #[test]
    fn effective_state_preserves_preexisting_raw_mode() {
        let backend = CrosstermBackend {
            writer: Vec::new(),
            capabilities: Capabilities::default(),
            active: TerminalState {
                raw_mode: true,
                ..TerminalState::default()
            },
            confirmed: TerminalState {
                raw_mode: true,
                ..TerminalState::default()
            },
            keyboard_pushed: false,
            original_raw_mode: true,
        };

        assert!(backend.effective_state(&TerminalState::default()).raw_mode);
    }
}
