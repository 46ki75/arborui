use std::io::{self, Write};

use arborui_core::{Color, CursorShape, CursorState, CursorVisibility, Modifier, Style};
use arborui_render::{FramePatch, PatchCellContent};
use arborui_terminal::{Capabilities, ColorCapability};
use crossterm::{
    QueueableCommand,
    cursor::{Hide, MoveTo, SetCursorStyle, Show},
    style::{
        Attribute, Color as CrosstermColor, Print, SetAttribute, SetBackgroundColor,
        SetForegroundColor, SetUnderlineColor,
    },
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate},
};

pub(crate) fn write_patch<W: Write>(
    writer: &mut W,
    patch: &FramePatch,
    capabilities: &Capabilities,
) -> io::Result<()> {
    patch
        .validate_for_width_policy(capabilities.width_policy)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if patch.is_empty() {
        return Ok(());
    }

    if !capabilities.synchronized_updates {
        write_patch_body(writer, patch, capabilities.color)?;
        return writer.flush();
    }

    writer.queue(BeginSynchronizedUpdate)?;
    let body_result = write_patch_body(writer, patch, capabilities.color);
    let end_result = writer.queue(EndSynchronizedUpdate).map(|_| ());
    let flush_result = writer.flush();
    body_result.and(end_result).and(flush_result)
}

fn write_patch_body<W: Write>(
    writer: &mut W,
    patch: &FramePatch,
    color_capability: ColorCapability,
) -> io::Result<()> {
    let mut active_style = None;
    for run in &patch.runs {
        for (offset, cell) in run.cells.iter().enumerate() {
            let x = coordinate(run.position.x, offset)?;
            let y = coordinate(run.position.y, 0)?;
            match &cell.content {
                PatchCellContent::Continuation { .. } => continue,
                PatchCellContent::Empty => {
                    writer.queue(MoveTo(x, y))?;
                    apply_style(writer, cell.style, color_capability, &mut active_style)?;
                    writer.queue(Print(' '))?;
                }
                PatchCellContent::Grapheme { text, .. } => {
                    writer.queue(MoveTo(x, y))?;
                    apply_style(writer, cell.style, color_capability, &mut active_style)?;
                    writer.queue(Print(text.as_ref()))?;
                }
            }
        }
    }

    if !patch.runs.is_empty() || patch.cursor_changed {
        apply_cursor(writer, patch.cursor)?;
    }
    Ok(())
}

pub(crate) fn apply_cursor<W: Write>(writer: &mut W, cursor: CursorState) -> io::Result<()> {
    match cursor.visibility {
        CursorVisibility::Hidden => {
            writer.queue(Hide)?;
        }
        CursorVisibility::Visible => {
            let x = u16::try_from(cursor.position.x)
                .map_err(|_| invalid_coordinate(cursor.position.x))?;
            let y = u16::try_from(cursor.position.y)
                .map_err(|_| invalid_coordinate(cursor.position.y))?;
            writer.queue(MoveTo(x, y))?;
            writer.queue(Show)?;
            writer.queue(cursor_style(cursor))?;
        }
    }
    Ok(())
}

fn apply_style<W: Write>(
    writer: &mut W,
    style: Style,
    color_capability: ColorCapability,
    active: &mut Option<Style>,
) -> io::Result<()> {
    if *active == Some(style) {
        return Ok(());
    }

    writer.queue(SetAttribute(Attribute::Reset))?;
    writer.queue(SetForegroundColor(
        style.foreground.map_or(CrosstermColor::Reset, |value| {
            color(value, color_capability)
        }),
    ))?;
    writer.queue(SetBackgroundColor(
        style.background.map_or(CrosstermColor::Reset, |value| {
            color(value, color_capability)
        }),
    ))?;
    writer.queue(SetUnderlineColor(
        style
            .underline_color
            .map_or(CrosstermColor::Reset, |value| {
                color(value, color_capability)
            }),
    ))?;

    for (modifier, attribute) in [
        (Modifier::BOLD, Attribute::Bold),
        (Modifier::DIM, Attribute::Dim),
        (Modifier::ITALIC, Attribute::Italic),
        (Modifier::UNDERLINED, Attribute::Underlined),
        (Modifier::SLOW_BLINK, Attribute::SlowBlink),
        (Modifier::RAPID_BLINK, Attribute::RapidBlink),
        (Modifier::REVERSED, Attribute::Reverse),
        (Modifier::HIDDEN, Attribute::Hidden),
        (Modifier::CROSSED_OUT, Attribute::CrossedOut),
    ] {
        if style.modifiers.contains(modifier) {
            writer.queue(SetAttribute(attribute))?;
        }
    }
    *active = Some(style);
    Ok(())
}

fn color(color: Color, capability: ColorCapability) -> CrosstermColor {
    match (color, capability) {
        (Color::Rgb { red, green, blue }, ColorCapability::TrueColor) => CrosstermColor::Rgb {
            r: red,
            g: green,
            b: blue,
        },
        (Color::Rgb { red, green, blue }, ColorCapability::Ansi256) => {
            CrosstermColor::AnsiValue(rgb_to_ansi256(red, green, blue))
        }
        (Color::Rgb { red, green, blue }, ColorCapability::Ansi16) => {
            nearest_ansi16(red, green, blue)
        }
        (Color::Indexed(value), ColorCapability::TrueColor | ColorCapability::Ansi256) => {
            CrosstermColor::AnsiValue(value)
        }
        (Color::Indexed(value), ColorCapability::Ansi16) if value < 16 => ansi16(value),
        (Color::Indexed(value), ColorCapability::Ansi16) => {
            let (red, green, blue) = ansi256_rgb(value);
            nearest_ansi16(red, green, blue)
        }
        (named, _) => named_color(named),
    }
}

const fn named_color(color: Color) -> CrosstermColor {
    match color {
        Color::Reset => CrosstermColor::Reset,
        Color::Black => CrosstermColor::Black,
        Color::Red => CrosstermColor::DarkRed,
        Color::Green => CrosstermColor::DarkGreen,
        Color::Yellow => CrosstermColor::DarkYellow,
        Color::Blue => CrosstermColor::DarkBlue,
        Color::Magenta => CrosstermColor::DarkMagenta,
        Color::Cyan => CrosstermColor::DarkCyan,
        Color::White => CrosstermColor::Grey,
        Color::BrightBlack => CrosstermColor::DarkGrey,
        Color::BrightRed => CrosstermColor::Red,
        Color::BrightGreen => CrosstermColor::Green,
        Color::BrightYellow => CrosstermColor::Yellow,
        Color::BrightBlue => CrosstermColor::Blue,
        Color::BrightMagenta => CrosstermColor::Magenta,
        Color::BrightCyan => CrosstermColor::Cyan,
        Color::BrightWhite => CrosstermColor::White,
        Color::Indexed(_) | Color::Rgb { .. } => CrosstermColor::Reset,
    }
}

const fn ansi16(value: u8) -> CrosstermColor {
    match value {
        0 => CrosstermColor::Black,
        1 => CrosstermColor::DarkRed,
        2 => CrosstermColor::DarkGreen,
        3 => CrosstermColor::DarkYellow,
        4 => CrosstermColor::DarkBlue,
        5 => CrosstermColor::DarkMagenta,
        6 => CrosstermColor::DarkCyan,
        7 => CrosstermColor::Grey,
        8 => CrosstermColor::DarkGrey,
        9 => CrosstermColor::Red,
        10 => CrosstermColor::Green,
        11 => CrosstermColor::Yellow,
        12 => CrosstermColor::Blue,
        13 => CrosstermColor::Magenta,
        14 => CrosstermColor::Cyan,
        _ => CrosstermColor::White,
    }
}

fn rgb_to_ansi256(red: u8, green: u8, blue: u8) -> u8 {
    let red = ((u16::from(red) * 5 + 127) / 255) as u8;
    let green = ((u16::from(green) * 5 + 127) / 255) as u8;
    let blue = ((u16::from(blue) * 5 + 127) / 255) as u8;
    16 + 36 * red + 6 * green + blue
}

fn ansi256_rgb(value: u8) -> (u8, u8, u8) {
    if value < 16 {
        return ANSI16_RGB[usize::from(value)];
    }
    if value >= 232 {
        let gray = 8 + (value - 232) * 10;
        return (gray, gray, gray);
    }

    let value = value - 16;
    let levels = [0, 95, 135, 175, 215, 255];
    (
        levels[usize::from(value / 36)],
        levels[usize::from((value % 36) / 6)],
        levels[usize::from(value % 6)],
    )
}

fn nearest_ansi16(red: u8, green: u8, blue: u8) -> CrosstermColor {
    let mut best = 0;
    let mut best_distance = u32::MAX;
    for (index, &(candidate_red, candidate_green, candidate_blue)) in ANSI16_RGB.iter().enumerate()
    {
        let red_distance = i32::from(red) - i32::from(candidate_red);
        let green_distance = i32::from(green) - i32::from(candidate_green);
        let blue_distance = i32::from(blue) - i32::from(candidate_blue);
        let distance = (red_distance * red_distance
            + green_distance * green_distance
            + blue_distance * blue_distance) as u32;
        if distance < best_distance {
            best = index as u8;
            best_distance = distance;
        }
    }
    ansi16(best)
}

const ANSI16_RGB: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (128, 0, 0),
    (0, 128, 0),
    (128, 128, 0),
    (0, 0, 128),
    (128, 0, 128),
    (0, 128, 128),
    (192, 192, 192),
    (128, 128, 128),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (0, 0, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

const fn cursor_style(cursor: CursorState) -> SetCursorStyle {
    match (cursor.shape, cursor.blinking) {
        (CursorShape::Block, true) => SetCursorStyle::BlinkingBlock,
        (CursorShape::Block, false) => SetCursorStyle::SteadyBlock,
        (CursorShape::Underline, true) => SetCursorStyle::BlinkingUnderScore,
        (CursorShape::Underline, false) => SetCursorStyle::SteadyUnderScore,
        (CursorShape::Bar, true) => SetCursorStyle::BlinkingBar,
        (CursorShape::Bar, false) => SetCursorStyle::SteadyBar,
    }
}

fn coordinate(base: i32, offset: usize) -> io::Result<u16> {
    let offset = i32::try_from(offset).map_err(|_| invalid_coordinate(i32::MAX))?;
    let coordinate = base
        .checked_add(offset)
        .ok_or_else(|| invalid_coordinate(base))?;
    u16::try_from(coordinate).map_err(|_| invalid_coordinate(coordinate))
}

fn invalid_coordinate(value: i32) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("terminal coordinate {value} is outside the u16 range"),
    )
}

#[cfg(test)]
mod tests {
    use std::{io, sync::Arc};

    use arborui_core::{Point, Size};
    use arborui_render::Renderer;
    use arborui_text::WidthPolicy;

    use super::*;

    #[derive(Default)]
    struct FailOnceOnText {
        bytes: Vec<u8>,
        failed: bool,
    }

    impl Write for FailOnceOnText {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if !self.failed && buffer.contains(&b'x') {
                self.failed = true;
                return Err(io::Error::other("injected write failure"));
            }
            self.bytes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn serializes_changed_graphemes_and_cursor() -> Result<(), Box<dyn std::error::Error>> {
        let mut renderer = Renderer::new(Size::new(2, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(
            Size::new(2, 1),
            CursorState::visible(Point::new(1, 0)),
            |canvas| {
                canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
                Ok(())
            },
        )?;
        let mut output = Vec::new();

        write_patch(
            &mut output,
            frame.patch(),
            &Capabilities {
                synchronized_updates: true,
                ..Capabilities::default()
            },
        )?;

        assert!(output.windows(1).any(|window| window == b"x"));
        assert!(output.starts_with(b"\x1b[?2026h"));
        assert!(output.ends_with(b"\x1b[?2026l"));
        Ok(())
    }

    #[test]
    fn no_op_patch_emits_no_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let first = renderer.prepare(Size::new(1, 1), CursorState::default(), |_| Ok(()))?;
        assert_eq!(renderer.commit(first), Ok(()));
        let second = renderer.prepare(Size::new(1, 1), CursorState::default(), |_| Ok(()))?;
        let mut output = Vec::new();

        write_patch(&mut output, second.patch(), &Capabilities::default())?;

        assert!(output.is_empty());
        Ok(())
    }

    #[test]
    fn wide_grapheme_is_emitted_once_at_the_leading_cell() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut renderer = Renderer::new(Size::new(3, 1), WidthPolicy::Unicode);
        let initial = renderer.prepare(Size::new(3, 1), CursorState::HIDDEN, |_| Ok(()))?;
        assert_eq!(renderer.commit(initial), Ok(()));
        let frame = renderer.prepare(Size::new(3, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::new(1, 0), "界", Style::default(), None)?;
            Ok(())
        })?;
        let mut output = Vec::new();

        write_patch(&mut output, frame.patch(), &Capabilities::default())?;

        let leading_move: &[u8] = b"\x1b[1;2H";
        let continuation_move: &[u8] = b"\x1b[1;3H";
        assert!(
            output
                .windows(leading_move.len())
                .any(|window| window == leading_move)
        );
        assert!(
            !output
                .windows(continuation_move.len())
                .any(|window| window == continuation_move)
        );
        assert_eq!(
            output
                .windows("界".len())
                .filter(|window| *window == "界".as_bytes())
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn malformed_continuation_is_rejected_before_output() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut renderer = Renderer::new(Size::new(2, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(2, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "界", Style::default(), None)?;
            Ok(())
        })?;
        let mut malformed = frame.patch().clone();
        malformed.runs[0].cells.remove(0);
        let mut output = Vec::new();

        let error = write_patch(
            &mut output,
            &malformed,
            &Capabilities {
                synchronized_updates: true,
                ..Capabilities::default()
            },
        )
        .expect_err("an isolated continuation must be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(output.is_empty());
        Ok(())
    }

    #[test]
    fn configured_width_policy_rejects_mismatched_text_before_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "·", Style::default(), None)?;
            Ok(())
        })?;
        let mut output = Vec::new();

        let error = write_patch(
            &mut output,
            frame.patch(),
            &Capabilities {
                synchronized_updates: true,
                width_policy: WidthPolicy::Cjk,
                ..Capabilities::default()
            },
        )
        .expect_err("the CJK width must differ from the patch's Unicode width");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(output.is_empty());
        Ok(())
    }

    #[test]
    fn malformed_grapheme_text_is_rejected_before_output() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;

        for malformed_text in ["界", "ab", "\n"] {
            let mut malformed = frame.patch().clone();
            let PatchCellContent::Grapheme { text, .. } = &mut malformed.runs[0].cells[0].content
            else {
                panic!("test patch must contain a grapheme");
            };
            *text = Arc::from(malformed_text);
            let mut output = Vec::new();

            let error = write_patch(
                &mut output,
                &malformed,
                &Capabilities {
                    synchronized_updates: true,
                    ..Capabilities::default()
                },
            )
            .expect_err("malformed grapheme text must be rejected");

            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(output.is_empty());
        }
        Ok(())
    }

    #[test]
    fn synchronized_update_is_closed_after_body_failure() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut renderer = Renderer::new(Size::new(1, 1), WidthPolicy::Unicode);
        let frame = renderer.prepare(Size::new(1, 1), CursorState::HIDDEN, |canvas| {
            canvas.draw_text(Point::ORIGIN, "x", Style::default(), None)?;
            Ok(())
        })?;
        let mut output = FailOnceOnText::default();

        assert!(
            write_patch(
                &mut output,
                frame.patch(),
                &Capabilities {
                    synchronized_updates: true,
                    ..Capabilities::default()
                },
            )
            .is_err()
        );
        assert!(output.bytes.ends_with(b"\x1b[?2026l"));
        Ok(())
    }

    #[test]
    fn colors_are_downgraded_to_terminal_capability() {
        assert_eq!(
            color(
                Color::Rgb {
                    red: 255,
                    green: 0,
                    blue: 0,
                },
                ColorCapability::Ansi16,
            ),
            CrosstermColor::Red
        );
        assert!(matches!(
            color(
                Color::Rgb {
                    red: 20,
                    green: 100,
                    blue: 200,
                },
                ColorCapability::Ansi256,
            ),
            CrosstermColor::AnsiValue(_)
        ));
    }
}
