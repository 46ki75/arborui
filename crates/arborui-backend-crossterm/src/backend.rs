use std::{env, io, io::Write, time::Duration};

use arborui_core::{CursorState, CursorVisibility, Size};
use arborui_render::FramePatch;
use arborui_terminal::{
    AutowrapMode, Capabilities, ColorCapability, KeyboardCapability, KeyboardMode, MouseCapability,
    MouseMode, ScreenMode, TerminalBackend, TerminalEvent, TerminalPixelSize, TerminalState,
    TerminalViewport, WriteOutcome,
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

use crate::{events::translate_event, kitty, output};

/// Policy for enabling Kitty graphics output.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum KittyGraphicsMode {
    /// Enable only for conservatively recognized direct terminal sessions.
    #[default]
    Auto,
    /// Never emit Kitty graphics commands.
    Disabled,
    /// Emit Kitty graphics commands without probing the terminal.
    Enabled,
}

/// Crossterm-backed terminal input, output, and lifecycle implementation.
pub struct CrosstermBackend<W: Write + Send> {
    writer: W,
    capabilities: Capabilities,
    active: TerminalState,
    confirmed: TerminalState,
    keyboard_pushed: bool,
    original_raw_mode: bool,
    lifecycle_stream_uncertain: bool,
    title_unconfirmed: bool,
    kitty: kitty::KittyState,
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
            lifecycle_stream_uncertain: false,
            title_unconfirmed: false,
            capabilities: detect_capabilities(),
            confirmed: active.clone(),
            active,
            keyboard_pushed: false,
            original_raw_mode,
            kitty: kitty::KittyState::new(kitty_single_command_from(
                env::var("TERM_PROGRAM").ok().as_deref(),
            )),
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

    /// Configures whether this backend emits Kitty graphics commands.
    ///
    /// Automatic mode uses conservative environment hints and deliberately
    /// disables graphics through SSH and terminal multiplexers. It does not
    /// actively query the terminal.
    #[must_use]
    pub fn with_kitty_graphics(mut self, mode: KittyGraphicsMode) -> Self {
        self.capabilities.kitty_graphics = match mode {
            KittyGraphicsMode::Auto => detect_kitty_graphics(),
            KittyGraphicsMode::Disabled => false,
            KittyGraphicsMode::Enabled => true,
        };
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

    fn recover_lifecycle_stream(&mut self) -> io::Result<()> {
        if self.lifecycle_stream_uncertain {
            // A partial title OSC can swallow subsequent modes and output even
            // without Kitty graphics. Retain recovery until ST is flushed.
            self.writer.write_all(b"\x1b\\")?;
            self.writer.flush()?;
            self.lifecycle_stream_uncertain = false;
        }
        Ok(())
    }

    fn settle_kitty_state(&mut self, screen: ScreenMode) -> io::Result<()> {
        let recover_stream = self.kitty.stream_uncertain();
        let cleanup_ids = self.kitty.cleanup_ids();
        let keyboard_maybe_pushed = self.keyboard_pushed
            || self.active.keyboard == KeyboardMode::Enhanced
            || self.confirmed.keyboard == KeyboardMode::Enhanced;
        let output_result = (|| -> io::Result<()> {
            kitty::write_recovery_if_needed(&mut self.writer, recover_stream)?;
            self.writer.queue(EnterAlternateScreen)?;
            kitty::write_deletions(&mut self.writer, &cleanup_ids)?;
            if keyboard_maybe_pushed {
                self.writer.queue(PopKeyboardEnhancementFlags)?;
            }
            if screen == ScreenMode::Main {
                self.writer.queue(LeaveAlternateScreen)?;
            }
            self.writer.flush()
        })();
        if let Err(error) = output_result {
            self.kitty.mark_stream_uncertain();
            return Err(error);
        }

        self.kitty.confirm_cleanup();
        self.keyboard_pushed = false;
        self.active.screen = screen;
        self.confirmed = TerminalState {
            raw_mode: self.active.raw_mode,
            screen,
            ..TerminalState::default()
        };
        Ok(())
    }

    fn restore_with<F>(&mut self, disable_raw: F) -> io::Result<()>
    where
        F: FnOnce() -> io::Result<()>,
    {
        let kitty_cleanup = self.kitty.cleanup_ids();
        let recover_kitty_stream = self.kitty.stream_uncertain();
        let output_result = (|| -> io::Result<()> {
            self.recover_lifecycle_stream()?;
            kitty::write_recovery_if_needed(&mut self.writer, recover_kitty_stream)?;
            if recover_kitty_stream {
                self.writer.queue(EnterAlternateScreen)?;
            }
            kitty::write_deletions(&mut self.writer, &kitty_cleanup)?;
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
            if recover_kitty_stream
                || self.active.screen == ScreenMode::Alternate
                || self.confirmed.screen == ScreenMode::Alternate
            {
                self.writer.queue(LeaveAlternateScreen)?;
            }
            if self.active.autowrap == AutowrapMode::Disabled
                || self.confirmed.autowrap == AutowrapMode::Disabled
            {
                self.writer.queue(EnableLineWrap)?;
            }
            if self.title_unconfirmed
                || self.active.title.is_some()
                || self.confirmed.title.is_some()
            {
                self.title_unconfirmed = true;
                self.lifecycle_stream_uncertain = true;
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
            self.lifecycle_stream_uncertain = false;
            self.title_unconfirmed = false;
            self.kitty.confirm_cleanup();
            // Output state is physically restored even if the separate raw-mode
            // operation fails, so a later activation must reapply every mode.
            self.active = TerminalState {
                raw_mode: self.active.raw_mode,
                ..TerminalState::default()
            };
            self.confirmed = self.active.clone();
        } else if recover_kitty_stream || !kitty_cleanup.is_empty() {
            self.kitty.mark_stream_uncertain();
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

    fn viewport(&self) -> Result<TerminalViewport, Self::Error> {
        if let Ok(window) = crossterm::terminal::window_size() {
            if let Some(viewport) = reported_viewport(window) {
                return Ok(viewport);
            }
        }
        self.size().map(TerminalViewport::from_cells)
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
        output::validate_cursor(desired.cursor)?;
        self.recover_lifecycle_stream()?;
        let desired = self.effective_state(desired);
        let leaving_alternate = desired.screen == ScreenMode::Main
            && (self.active.screen == ScreenMode::Alternate
                || self.confirmed.screen == ScreenMode::Alternate);
        if self.kitty.stream_uncertain()
            || (leaving_alternate && !self.kitty.cleanup_ids().is_empty())
        {
            self.settle_kitty_state(desired.screen)?;
        }
        let cleanup_kitty = desired.screen == ScreenMode::Main
            && (self.active.screen == ScreenMode::Alternate
                || self.confirmed.screen == ScreenMode::Alternate);
        let kitty_cleanup = cleanup_kitty.then(|| self.kitty.cleanup_ids());

        let output_result = (|| -> io::Result<()> {
            if desired.raw_mode && !self.active.raw_mode {
                enable_raw_mode()?;
                self.active.raw_mode = true;
            }
            if state_changed(desired.screen, self.active.screen, self.confirmed.screen) {
                if self.keyboard_pushed {
                    self.writer.queue(PopKeyboardEnhancementFlags)?;
                    self.keyboard_pushed = false;
                }
                match desired.screen {
                    ScreenMode::Main => {
                        if let Some(ids) = &kitty_cleanup {
                            kitty::write_deletions(&mut self.writer, ids)?;
                        }
                        self.writer.queue(LeaveAlternateScreen)?;
                    }
                    ScreenMode::Alternate => {
                        self.writer.queue(EnterAlternateScreen)?;
                    }
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
            ) || (desired.keyboard == KeyboardMode::Enhanced && !self.keyboard_pushed)
            {
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
            if self.title_unconfirmed
                || state_changed(&desired.title, &self.active.title, &self.confirmed.title)
            {
                let title = sanitized_title(desired.title.as_deref().unwrap_or_default());
                // Parser recovery alone does not confirm the title's value or
                // discharge our obligation to clear even a partially sent title.
                self.title_unconfirmed = true;
                self.lifecycle_stream_uncertain = true;
                self.writer.queue(SetTitle(title))?;
                self.active.title.clone_from(&desired.title);
            }
            if state_changed(desired.cursor, self.active.cursor, self.confirmed.cursor) {
                self.apply_cursor(desired.cursor)?;
            }
            self.active.synchronized_updates = desired.synchronized_updates;
            self.writer.flush()
        })();
        if output_result.is_err() && kitty_cleanup.as_ref().is_some_and(|ids| !ids.is_empty()) {
            self.kitty.mark_stream_uncertain();
        }
        output_result?;
        self.lifecycle_stream_uncertain = false;
        self.title_unconfirmed = false;
        if cleanup_kitty {
            self.kitty.confirm_cleanup();
        }
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
        patch
            .validate_for_width_policy(self.capabilities.width_policy)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let empty_scene = arborui_render::ImageScene::new();
        let image_scene = (self.capabilities.kitty_graphics
            && self.active.screen == ScreenMode::Alternate)
            .then(|| {
                patch
                    .images
                    .as_ref()
                    .or_else(|| patch.full_repaint.then_some(&empty_scene))
            })
            .flatten();
        let image_viewport = image_scene.and_then(|_| {
            crossterm::terminal::window_size()
                .ok()
                .and_then(reported_viewport)
        });
        let image_update = image_scene
            .map(|scene| self.kitty.prepare_with_viewport(scene, image_viewport))
            .transpose()?;
        // Preparation distinguishes actual image output from fallback-only scenes.
        // Reject an emitted cursor before even parser recovery writes any bytes.
        output::validate_patch_cursor(
            patch,
            image_update
                .as_ref()
                .is_some_and(kitty::PreparedUpdate::has_output),
        )?;
        self.recover_lifecycle_stream()?;
        let output_result = output::write_patch_with_images(
            &mut self.writer,
            patch,
            &Capabilities {
                synchronized_updates: self.active.synchronized_updates,
                ..self.capabilities
            },
            image_update.as_ref(),
        );
        if output_result.is_err() && image_update.is_some() {
            self.kitty.mark_stream_uncertain();
        }
        output_result?;
        if let Some(update) = &image_update {
            self.kitty.confirm(update);
        }
        if !patch.runs.is_empty()
            || patch.cursor_changed
            || image_update
                .as_ref()
                .is_some_and(kitty::PreparedUpdate::has_output)
        {
            self.active.cursor = patch.cursor;
            self.confirmed.cursor = patch.cursor;
        }
        Ok(WriteOutcome::Applied)
    }

    fn restore(&mut self) -> Result<(), Self::Error> {
        self.restore_with(disable_raw_mode)
    }
}

fn reported_viewport(window: crossterm::terminal::WindowSize) -> Option<TerminalViewport> {
    let cells = Size::new(window.columns, window.rows);
    if cells.is_empty() {
        return None;
    }
    if window.width == 0 || window.height == 0 {
        return Some(TerminalViewport::from_cells(cells));
    }
    Some(TerminalViewport::with_pixels(
        cells,
        TerminalPixelSize::new(window.width, window.height),
    ))
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
        kitty_graphics: detect_kitty_graphics(),
        ..Capabilities::default()
    }
}

fn detect_kitty_graphics() -> bool {
    detect_kitty_graphics_from(
        env::var("TERM").ok().as_deref(),
        env::var("TERM_PROGRAM").ok().as_deref(),
        env::var_os("KITTY_WINDOW_ID").is_some(),
        env::var_os("TMUX").is_some()
            || env::var_os("STY").is_some()
            || env::var_os("ZELLIJ").is_some()
            || env::var_os("ZELLIJ_SESSION_NAME").is_some()
            || env::var_os("MOSH_IP").is_some()
            || env::var_os("MOSH_CLIENT_PID").is_some()
            || env::var_os("SSH_CONNECTION").is_some()
            || env::var_os("SSH_TTY").is_some(),
    )
}

fn kitty_single_command_from(term_program: Option<&str>) -> bool {
    term_program.is_some_and(|value| {
        value.eq_ignore_ascii_case("vscode") || value.eq_ignore_ascii_case("xterm.js")
    })
}

fn detect_kitty_graphics_from(
    term: Option<&str>,
    term_program: Option<&str>,
    kitty_window: bool,
    indirect: bool,
) -> bool {
    let term_is_multiplexer = term.is_some_and(|value| {
        let value = value.to_ascii_lowercase();
        value.starts_with("tmux") || value.starts_with("screen")
    });
    if indirect || term_is_multiplexer {
        return false;
    }
    let recognized = |value: &str| {
        let value = value.to_ascii_lowercase();
        ["kitty", "ghostty", "wezterm", "vscode"]
            .iter()
            .any(|terminal| value.contains(terminal))
    };
    kitty_window || term.is_some_and(recognized) || term_program.is_some_and(recognized)
}

#[cfg(test)]
mod tests {
    mod title;

    use std::io::Write;

    use arborui_core::{Point, Rect, Style};
    use arborui_render::{ImagePlacement, ImageScene, Renderer, RgbaImage};
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
    fn reported_viewport_discards_missing_pixel_dimensions() {
        let cells = Size::new(80, 24);

        assert_eq!(
            reported_viewport(crossterm::terminal::WindowSize {
                columns: 80,
                rows: 24,
                width: 800,
                height: 600,
            }),
            Some(TerminalViewport::with_pixels(
                cells,
                TerminalPixelSize::new(800, 600),
            ))
        );
        assert_eq!(
            reported_viewport(crossterm::terminal::WindowSize {
                columns: 80,
                rows: 24,
                width: 0,
                height: 0,
            }),
            Some(TerminalViewport::from_cells(cells))
        );
        assert_eq!(
            reported_viewport(crossterm::terminal::WindowSize {
                columns: 0,
                rows: 24,
                width: 800,
                height: 600,
            }),
            None
        );
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
    fn kitty_graphics_mode_overrides_detection() -> io::Result<()> {
        let disabled =
            CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Disabled);
        let enabled =
            CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Enabled);

        assert!(!disabled.capabilities().kitty_graphics);
        assert!(enabled.capabilities().kitty_graphics);
        Ok(())
    }

    #[test]
    fn kitty_single_command_workaround_requires_an_xterm_js_hint() {
        for program in [
            None,
            Some(""),
            Some("kitty"),
            Some("Ghostty"),
            Some("WezTerm"),
            Some("xterm"),
        ] {
            assert!(!kitty_single_command_from(program), "{program:?}");
        }
        for program in ["vscode", "VSCode", "xterm.js", "XTERM.JS"] {
            assert!(kitty_single_command_from(Some(program)), "{program}");
        }
    }

    #[test]
    fn automatic_kitty_detection_recognizes_direct_and_rejects_indirect_sessions() {
        assert!(detect_kitty_graphics_from(
            Some("xterm-kitty"),
            None,
            false,
            false,
        ));
        assert!(detect_kitty_graphics_from(
            None,
            Some("Ghostty"),
            false,
            false,
        ));
        assert!(detect_kitty_graphics_from(
            Some("xterm-256color"),
            Some("vscode"),
            false,
            false,
        ));
        assert!(!detect_kitty_graphics_from(
            None,
            Some("Ghostty"),
            false,
            true,
        ));
        assert!(!detect_kitty_graphics_from(
            Some("xterm-kitty"),
            None,
            true,
            true,
        ));
        assert!(!detect_kitty_graphics_from(
            Some("tmux-256color"),
            Some("kitty"),
            false,
            false,
        ));
    }

    #[test]
    fn enabled_backend_writes_and_cleans_up_kitty_images() -> Result<(), Box<dyn std::error::Error>>
    {
        let image = RgbaImage::new(1, 1, vec![1, 2, 3, 4])?;
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_image(Rect::new(0, 0, 1, 1), &image)?;
            Ok(())
        })?;
        let mut backend =
            CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Enabled);
        backend.apply_state(&TerminalState {
            screen: ScreenMode::Alternate,
            ..TerminalState::default()
        })?;

        backend.write_patch(frame.patch())?;
        backend.restore()?;
        let output = backend.into_inner()?;

        let upload = b"\x1b_Ga=T,f=32,t=d,s=1,v=1,i=1,x=0,y=0,w=1,h=1,c=1,r=1,C=1,q=2,z=1;";
        let deletion = b"\x1b_Ga=d,d=I,i=1,q=2\x1b\\";
        let leave_alternate = b"\x1b[?1049l";
        assert!(output.windows(upload.len()).any(|window| window == upload));
        let delete_position = output
            .windows(deletion.len())
            .position(|window| window == deletion)
            .ok_or("missing Kitty deletion")?;
        let leave_position = output
            .windows(leave_alternate.len())
            .position(|window| window == leave_alternate)
            .ok_or("missing alternate-screen leave")?;
        assert!(delete_position < leave_position);
        Ok(())
    }

    #[test]
    fn disabled_backend_emits_only_cell_fallback() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1, 1, vec![1, 2, 3, 4])?;
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            canvas.draw_image(Rect::new(0, 0, 1, 1), &image)?;
            Ok(())
        })?;
        let mut backend =
            CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Disabled);

        backend.write_patch(frame.patch())?;
        let output = backend.into_inner()?;

        assert!(output.contains(&b'x'));
        assert!(!output.windows(3).any(|window| window == b"\x1b_G"));
        Ok(())
    }

    #[test]
    fn enabled_backend_suppresses_images_on_the_main_screen()
    -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1, 1, vec![1, 2, 3, 4])?;
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            canvas.draw_image(Rect::new(0, 0, 1, 1), &image)?;
            Ok(())
        })?;
        let mut backend =
            CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Enabled);

        backend.write_patch(frame.patch())?;

        assert!(backend.writer.contains(&b'x'));
        assert!(!backend.writer.windows(3).any(|window| window == b"\x1b_G"));
        Ok(())
    }

    #[test]
    fn failed_image_flush_is_deleted_before_retry() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1, 1, vec![0; 4])?;
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_image(Rect::new(0, 0, 1, 1), &image)?;
            Ok(())
        })?;
        let writer = FailFlushOnce {
            fail_on_flush: 2,
            ..FailFlushOnce::default()
        };
        let mut backend =
            CrosstermBackend::new(writer)?.with_kitty_graphics(KittyGraphicsMode::Enabled);
        backend.apply_state(&TerminalState {
            screen: ScreenMode::Alternate,
            ..TerminalState::default()
        })?;

        assert!(backend.write_patch(frame.patch()).is_err());
        assert_eq!(backend.write_patch(frame.patch())?, WriteOutcome::Applied);

        let deletion = b"\x1b_Ga=d,d=I,i=1,q=2\x1b\\";
        assert!(
            backend
                .writer
                .bytes
                .windows(deletion.len())
                .any(|window| window == deletion)
        );
        Ok(())
    }

    #[test]
    fn partial_image_write_is_aborted_before_retry_output() -> Result<(), Box<dyn std::error::Error>>
    {
        let pixels = (0..1_024 * 4)
            .scan(0x1234_5678_u32, |value, _| {
                *value ^= *value << 13;
                *value ^= *value >> 17;
                *value ^= *value << 5;
                Some(*value as u8)
            })
            .collect::<Vec<_>>();
        let image = RgbaImage::new(1_024, 1, pixels)?;
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_image(Rect::new(0, 0, 1, 1), &image)?;
            Ok(())
        })?;
        for fail_after in [200, 5_000] {
            let mut backend = CrosstermBackend::new(FailWriteOnceAfter::default())?
                .with_kitty_graphics(KittyGraphicsMode::Enabled);
            // Test both the first APC and a continuation regardless of terminal hints.
            backend.kitty = kitty::KittyState::default();
            backend.apply_state(&TerminalState {
                screen: ScreenMode::Alternate,
                ..TerminalState::default()
            })?;
            backend.writer.fail_after = Some(fail_after);

            assert!(backend.write_patch(frame.patch()).is_err());
            if fail_after == 5_000 {
                let continuation = b"\x1b_Gm=0,q=2;";
                assert!(
                    backend
                        .writer
                        .bytes
                        .windows(continuation.len())
                        .any(|bytes| bytes == continuation)
                );
            }
            let retry_start = backend.writer.bytes.len();
            assert_eq!(backend.write_patch(frame.patch())?, WriteOutcome::Applied);

            let recovery_and_delete = b"\x1b\\\x1b[?2026l\x1b_Ga=d,d=I,i=1,q=2\x1b\\";
            assert!(backend.writer.bytes[retry_start..].starts_with(recovery_and_delete));
        }
        Ok(())
    }

    #[test]
    fn ambiguous_image_cleanup_reenters_alternate_screen_before_deletion()
    -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1, 1, vec![0; 4])?;
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_image(Rect::new(0, 0, 1, 1), &image)?;
            Ok(())
        })?;
        let writer = FailFlushOnce {
            fail_on_flush: 3,
            ..FailFlushOnce::default()
        };
        let mut backend =
            CrosstermBackend::new(writer)?.with_kitty_graphics(KittyGraphicsMode::Enabled);
        backend.apply_state(&TerminalState {
            screen: ScreenMode::Alternate,
            ..TerminalState::default()
        })?;
        backend.write_patch(frame.patch())?;

        assert!(backend.restore().is_err());
        let retry_start = backend.writer.bytes.len();
        backend.restore()?;

        let recovery_and_reentry = b"\x1b\\\x1b[?2026l\x1b[?1049h\x1b_Ga=d,d=I,i=1,q=2\x1b\\";
        assert!(backend.writer.bytes[retry_start..].starts_with(recovery_and_reentry));
        Ok(())
    }

    #[test]
    fn uncertain_stream_is_recovered_before_resume_modes() -> Result<(), Box<dyn std::error::Error>>
    {
        let image = RgbaImage::new(1, 1, vec![0; 4])?;
        let scene =
            ImageScene::from_placements([ImagePlacement::new(image, Rect::new(0, 0, 1, 1))]);
        let mut backend =
            CrosstermBackend::new(Vec::new())?.with_kitty_graphics(KittyGraphicsMode::Enabled);
        let _update = backend.kitty.prepare_with_viewport(&scene, None)?;
        backend.kitty.mark_stream_uncertain();

        backend.apply_state(&TerminalState {
            screen: ScreenMode::Alternate,
            title: Some(String::from("resumed")),
            ..TerminalState::default()
        })?;

        let recovery = b"\x1b\\\x1b[?2026l\x1b[?1049h\x1b_Ga=d,d=I,i=1,q=2\x1b\\";
        assert!(backend.writer.starts_with(recovery));
        Ok(())
    }

    #[test]
    fn repeated_cleanup_failures_leave_the_reentered_alternate_screen()
    -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1, 1, vec![0; 4])?;
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_image(Rect::new(0, 0, 1, 1), &image)?;
            Ok(())
        })?;
        let writer = FailFlushes {
            bytes: Vec::new(),
            flushes: 0,
            fail_on: [3, 4],
        };
        let mut backend =
            CrosstermBackend::new(writer)?.with_kitty_graphics(KittyGraphicsMode::Enabled);
        backend.apply_state(&TerminalState {
            screen: ScreenMode::Alternate,
            ..TerminalState::default()
        })?;
        backend.write_patch(frame.patch())?;

        assert!(backend.apply_state(&TerminalState::default()).is_err());
        assert!(backend.restore().is_err());
        let retry_start = backend.writer.bytes.len();
        backend.restore()?;

        let output = &backend.writer.bytes[retry_start..];
        let enter = output
            .windows(8)
            .position(|window| window == b"\x1b[?1049h")
            .ok_or("missing alternate-screen re-entry")?;
        let leave = output
            .windows(8)
            .position(|window| window == b"\x1b[?1049l")
            .ok_or("missing alternate-screen leave")?;
        assert!(enter < leave);
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

    #[test]
    fn keyboard_stack_is_popped_before_leaving_the_alternate_screen() -> io::Result<()> {
        let mut backend = CrosstermBackend::new(Vec::new())?.with_capabilities(Capabilities {
            keyboard: KeyboardCapability::Enhanced,
            ..Capabilities::default()
        });
        backend.apply_state(&TerminalState {
            screen: ScreenMode::Alternate,
            keyboard: KeyboardMode::Enhanced,
            ..TerminalState::default()
        })?;

        backend.apply_state(&TerminalState::default())?;

        let pop = backend
            .writer
            .windows(3)
            .rposition(|window| window == b"\x1b[<")
            .expect("missing keyboard stack pop");
        let leave = backend
            .writer
            .windows(8)
            .rposition(|window| window == b"\x1b[?1049l")
            .expect("missing alternate-screen leave");
        assert!(pop < leave);
        Ok(())
    }

    #[test]
    fn enhanced_keyboard_ownership_moves_between_screen_buffers() -> io::Result<()> {
        let mut backend = CrosstermBackend::new(Vec::new())?.with_capabilities(Capabilities {
            keyboard: KeyboardCapability::Enhanced,
            ..Capabilities::default()
        });
        backend.apply_state(&TerminalState {
            keyboard: KeyboardMode::Enhanced,
            ..TerminalState::default()
        })?;
        let transition_start = backend.writer.len();

        backend.apply_state(&TerminalState {
            screen: ScreenMode::Alternate,
            keyboard: KeyboardMode::Enhanced,
            ..TerminalState::default()
        })?;

        let output = &backend.writer[transition_start..];
        let pop = output
            .windows(3)
            .position(|window| window == b"\x1b[<")
            .expect("missing main-screen keyboard stack pop");
        let enter = output
            .windows(8)
            .position(|window| window == b"\x1b[?1049h")
            .expect("missing alternate-screen entry");
        let push = output
            .windows(4)
            .position(|window| window.starts_with(b"\x1b[>"))
            .expect("missing alternate-screen keyboard stack push");
        assert!(pop < enter && enter < push);
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

    struct FailFlushes {
        bytes: Vec<u8>,
        flushes: usize,
        fail_on: [usize; 2],
    }

    impl Write for FailFlushes {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            if self.fail_on.contains(&self.flushes) {
                return Err(io::Error::other("injected flush failure"));
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailWriteOnceAfter {
        bytes: Vec<u8>,
        fail_after: Option<usize>,
        fail_flush: bool,
    }

    impl Write for FailWriteOnceAfter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let Some(remaining) = self.fail_after else {
                self.bytes.extend_from_slice(buffer);
                return Ok(buffer.len());
            };
            if remaining == 0 {
                self.fail_after = None;
                return Err(io::Error::other("injected partial write failure"));
            }
            let written = remaining.min(buffer.len());
            self.bytes.extend_from_slice(&buffer[..written]);
            self.fail_after = Some(remaining - written);
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            if std::mem::take(&mut self.fail_flush) {
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
            images: None,
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
            lifecycle_stream_uncertain: false,
            title_unconfirmed: false,
            kitty: kitty::KittyState::default(),
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
    fn keyboard_stack_commands_are_rebalanced_after_failed_screen_transition() -> io::Result<()> {
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
        assert_eq!(pushes, 2);
        assert_eq!(pops, 2);
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
            lifecycle_stream_uncertain: false,
            title_unconfirmed: false,
            kitty: kitty::KittyState::default(),
        };

        assert!(backend.effective_state(&TerminalState::default()).raw_mode);
    }

    mod coordinate;
}
