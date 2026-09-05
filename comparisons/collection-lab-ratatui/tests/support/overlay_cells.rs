use arborui::{Color as ArboruiColor, Modifier as ArboruiModifier};
use arborui_test::{TestCell, TestCellContent};
use ratatui::{
    buffer::Cell,
    style::{Color, Modifier},
};

#[derive(Debug, Eq, PartialEq)]
pub struct StyledCell<'a> {
    content: &'a str,
    foreground: Color,
    background: Color,
    underline_color: Color,
    modifiers: Modifier,
}

pub fn arborui_cell(cell: &TestCell) -> StyledCell<'_> {
    let content = match &cell.content {
        TestCellContent::Empty => " ",
        TestCellContent::Grapheme { text, width: 1 } => text,
        content => panic!("overlay oracle expects single-column content: {content:?}"),
    };
    let mut modifiers = Modifier::empty();
    for (arborui, ratatui) in [
        (ArboruiModifier::BOLD, Modifier::BOLD),
        (ArboruiModifier::DIM, Modifier::DIM),
        (ArboruiModifier::ITALIC, Modifier::ITALIC),
        (ArboruiModifier::UNDERLINED, Modifier::UNDERLINED),
        (ArboruiModifier::SLOW_BLINK, Modifier::SLOW_BLINK),
        (ArboruiModifier::RAPID_BLINK, Modifier::RAPID_BLINK),
        (ArboruiModifier::REVERSED, Modifier::REVERSED),
        (ArboruiModifier::HIDDEN, Modifier::HIDDEN),
        (ArboruiModifier::CROSSED_OUT, Modifier::CROSSED_OUT),
    ] {
        if cell.style.modifiers.contains(arborui) {
            modifiers.insert(ratatui);
        }
    }
    StyledCell {
        content,
        foreground: color(cell.style.foreground),
        background: color(cell.style.background),
        underline_color: color(cell.style.underline_color),
        modifiers,
    }
}

pub fn ratatui_cell(cell: &Cell) -> StyledCell<'_> {
    StyledCell {
        content: cell.symbol(),
        foreground: cell.fg,
        background: cell.bg,
        underline_color: cell.underline_color,
        modifiers: cell.modifier,
    }
}

// Committed None colors serialize as Reset. Map only terminal color aliases;
// never guess RGB palette values or discard styles on blank/reversed cells.
fn color(color: Option<ArboruiColor>) -> Color {
    match color {
        None | Some(ArboruiColor::Reset) => Color::Reset,
        Some(ArboruiColor::Black) => Color::Black,
        Some(ArboruiColor::Red) => Color::Red,
        Some(ArboruiColor::Green) => Color::Green,
        Some(ArboruiColor::Yellow) => Color::Yellow,
        Some(ArboruiColor::Blue) => Color::Blue,
        Some(ArboruiColor::Magenta) => Color::Magenta,
        Some(ArboruiColor::Cyan) => Color::Cyan,
        Some(ArboruiColor::White) => Color::Gray,
        Some(ArboruiColor::BrightBlack) => Color::DarkGray,
        Some(ArboruiColor::BrightRed) => Color::LightRed,
        Some(ArboruiColor::BrightGreen) => Color::LightGreen,
        Some(ArboruiColor::BrightYellow) => Color::LightYellow,
        Some(ArboruiColor::BrightBlue) => Color::LightBlue,
        Some(ArboruiColor::BrightMagenta) => Color::LightMagenta,
        Some(ArboruiColor::BrightCyan) => Color::LightCyan,
        Some(ArboruiColor::BrightWhite) => Color::White,
        Some(ArboruiColor::Indexed(index)) => Color::Indexed(index),
        Some(ArboruiColor::Rgb { red, green, blue }) => Color::Rgb(red, green, blue),
    }
}

#[test]
fn overlay_oracle_preserves_blank_cell_styles_and_color_distinctions() {
    let mut arborui = TestCell::default();
    let mut ratatui = Cell::default();
    assert_eq!(arborui_cell(&arborui), ratatui_cell(&ratatui));
    arborui.style = arborui::Style::new()
        .foreground(ArboruiColor::Reset)
        .background(ArboruiColor::Reset)
        .underline_color(ArboruiColor::Reset);
    assert_eq!(arborui_cell(&arborui), ratatui_cell(&ratatui));

    for (source, target) in [
        (ArboruiColor::White, Color::Gray),
        (ArboruiColor::BrightBlack, Color::DarkGray),
        (ArboruiColor::BrightCyan, Color::LightCyan),
        (ArboruiColor::BrightWhite, Color::White),
        (ArboruiColor::Indexed(42), Color::Indexed(42)),
        (ArboruiColor::rgb(1, 2, 3), Color::Rgb(1, 2, 3)),
    ] {
        arborui.style.foreground = Some(source);
        assert_ne!(arborui_cell(&arborui), ratatui_cell(&ratatui));
        ratatui.fg = target;
        assert_eq!(arborui_cell(&arborui), ratatui_cell(&ratatui));
    }
    arborui.style.background = Some(ArboruiColor::Black);
    assert_ne!(arborui_cell(&arborui), ratatui_cell(&ratatui));
    ratatui.bg = Color::Black;
    assert_eq!(arborui_cell(&arborui), ratatui_cell(&ratatui));
    arborui.style.underline_color = Some(ArboruiColor::Red);
    assert_ne!(arborui_cell(&arborui), ratatui_cell(&ratatui));
    ratatui.underline_color = Color::Red;
    assert_eq!(arborui_cell(&arborui), ratatui_cell(&ratatui));
    arborui.style.modifiers = ArboruiModifier::REVERSED;
    assert_ne!(arborui_cell(&arborui), ratatui_cell(&ratatui));
    ratatui.modifier = Modifier::REVERSED;
    assert_eq!(arborui_cell(&arborui), ratatui_cell(&ratatui));
    ratatui.bg = Color::Reset;
    assert_ne!(arborui_cell(&arborui), ratatui_cell(&ratatui));
}
