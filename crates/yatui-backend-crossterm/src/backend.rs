use std::{env, io, io::Write, time::Duration};

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
use yatui_core::{CursorState, CursorVisibility, Size};
use yatui_render::FramePatch;
use yatui_terminal::{
    AutowrapMode, Capabilities, ColorCapability, KeyboardCapability, KeyboardMode, MouseCapability,
    MouseMode, ScreenMode, TerminalBackend, TerminalEvent, TerminalState, WriteOutcome,
};

use crate::{events::translate_event, output};

/// Crossterm-backed terminal input, output, and lifecycle implementation.
pub struct CrosstermBackend<W: Write + Send> {
    writer: W,
    capabilities: Capabilities,
    active: TerminalState,
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
            active,
            original_raw_mode,
        })
    }

    /// Overrides detected capabilities.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
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
        if desired.screen != self.active.screen {
            match desired.screen {
                ScreenMode::Main => self.writer.queue(LeaveAlternateScreen)?,
                ScreenMode::Alternate => self.writer.queue(EnterAlternateScreen)?,
            };
            self.active.screen = desired.screen;
        }
        if desired.mouse != self.active.mouse {
            match desired.mouse {
                MouseMode::Disabled => self.writer.queue(DisableMouseCapture)?,
                MouseMode::Capture => self.writer.queue(EnableMouseCapture)?,
            };
            self.active.mouse = desired.mouse;
        }
        if desired.focus_reporting != self.active.focus_reporting {
            if desired.focus_reporting {
                self.writer.queue(EnableFocusChange)?;
            } else {
                self.writer.queue(DisableFocusChange)?;
            }
            self.active.focus_reporting = desired.focus_reporting;
        }
        if desired.bracketed_paste != self.active.bracketed_paste {
            if desired.bracketed_paste {
                self.writer.queue(EnableBracketedPaste)?;
            } else {
                self.writer.queue(DisableBracketedPaste)?;
            }
            self.active.bracketed_paste = desired.bracketed_paste;
        }
        if desired.keyboard != self.active.keyboard {
            match desired.keyboard {
                KeyboardMode::Legacy => self.writer.queue(PopKeyboardEnhancementFlags)?,
                KeyboardMode::Enhanced => self.writer.queue(PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES,
                ))?,
            };
            self.active.keyboard = desired.keyboard;
        }
        if desired.autowrap != self.active.autowrap {
            match desired.autowrap {
                AutowrapMode::Disabled => self.writer.queue(DisableLineWrap)?,
                AutowrapMode::Enabled | AutowrapMode::Preserve => {
                    self.writer.queue(EnableLineWrap)?
                }
            };
            self.active.autowrap = desired.autowrap;
        }
        if desired.title != self.active.title {
            self.writer
                .queue(SetTitle(desired.title.as_deref().unwrap_or_default()))?;
            self.active.title.clone_from(&desired.title);
        }
        if desired.cursor != self.active.cursor {
            self.apply_cursor(desired.cursor)?;
        }
        self.active.synchronized_updates = desired.synchronized_updates;
        self.writer.flush()?;

        if !desired.raw_mode && self.active.raw_mode {
            disable_raw_mode()?;
            self.active.raw_mode = false;
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
        self.active.cursor = patch.cursor;
        Ok(WriteOutcome::Applied)
    }

    fn restore(&mut self) -> Result<(), Self::Error> {
        let output_result = (|| -> io::Result<()> {
            if self.active.keyboard == KeyboardMode::Enhanced {
                self.writer.queue(PopKeyboardEnhancementFlags)?;
            }
            if self.active.mouse == MouseMode::Capture {
                self.writer.queue(DisableMouseCapture)?;
            }
            if self.active.bracketed_paste {
                self.writer.queue(DisableBracketedPaste)?;
            }
            if self.active.focus_reporting {
                self.writer.queue(DisableFocusChange)?;
            }
            if self.active.screen == ScreenMode::Alternate {
                self.writer.queue(LeaveAlternateScreen)?;
            }
            if self.active.autowrap == AutowrapMode::Disabled {
                self.writer.queue(EnableLineWrap)?;
            }
            if self.active.title.is_some() {
                self.writer.queue(SetTitle(""))?;
            }
            self.writer.queue(SetAttribute(Attribute::Reset))?;
            self.writer.queue(SetForegroundColor(Color::Reset))?;
            self.writer.queue(SetBackgroundColor(Color::Reset))?;
            self.writer.queue(SetCursorStyle::DefaultUserShape)?;
            self.writer.queue(Show)?;
            self.writer.flush()
        })();

        let raw_result = if self.active.raw_mode && !self.original_raw_mode {
            disable_raw_mode()
        } else {
            Ok(())
        };

        self.active = TerminalState {
            raw_mode: self.original_raw_mode,
            ..TerminalState::default()
        };
        output_result.and(raw_result)
    }
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
    use yatui_core::{Point, Style};
    use yatui_render::Renderer;
    use yatui_text::WidthPolicy;

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
            title: Some(String::from("yatui test")),
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
}
