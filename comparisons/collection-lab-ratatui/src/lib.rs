//! Matched Ratatui implementation of ArborUI's Collection Lab experiment.

use std::{convert::Infallible, num::NonZeroUsize};

use arborui_example_collection_lab::{
    CollectionMode, FixedHeightProvider, VariableHeightProvider, VisibleRange,
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, ClearType, TestBackend, WindowSize},
    buffer::Buffer,
    layout::{Position, Rect, Size},
    style::{Color, Modifier, Style},
};

const OVERSCAN_ROWS: usize = 2;
const OVERSCAN_CELLS: usize = 3;
const HEADER: &str = "Arrows/Page/Home/End move | Enter selects | v mode | r reverse | q quit";

/// One framework-neutral action in the matched scenario.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComparisonAction {
    /// Move the active item upward.
    Up,
    /// Move the active item downward.
    Down,
    /// Move to the first item.
    Home,
    /// Move to the final item.
    End,
    /// Move upward by one viewport.
    PageUp,
    /// Move downward by one viewport.
    PageDown,
    /// Select the active item.
    SelectActive,
    /// Switch between fixed and variable-height rows.
    ToggleMode,
    /// Reverse item order while retaining stable identity.
    Reverse,
    /// Change the complete terminal dimensions.
    Resize {
        /// Complete terminal width.
        width: u16,
        /// Complete terminal height.
        height: u16,
    },
}

/// Observable state compared independently from character output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticState {
    /// Stable active item key.
    pub active_key: Option<u64>,
    /// Stable selected item key.
    pub selected_key: Option<u64>,
    /// Controlled logical scroll offset.
    pub scroll_offset: usize,
    /// Application-owned viewport height.
    pub viewport_height: usize,
    /// Item range constructed by the latest render.
    pub visible_range: VisibleRange,
    /// Number of rows constructed by the latest render.
    pub constructed_rows: usize,
}

#[derive(Clone, Debug)]
struct Item {
    key: u64,
    fixed_label: String,
    variable_label: String,
    height: NonZeroUsize,
}

/// Application-owned state and rendering policy for the matched Ratatui side.
pub struct RatatuiCollectionLab {
    items: Vec<Item>,
    mode: CollectionMode,
    fixed: FixedHeightProvider,
    variable: VariableHeightProvider,
    terminal_width: u16,
    terminal_height: u16,
    viewport_height: usize,
    scroll: usize,
    active: Option<u64>,
    selected: Option<u64>,
    constructed_rows: usize,
}

/// Logical test backend that records Ratatui's diff output work.
pub struct CountingBackend {
    inner: TestBackend,
    changed_cells: usize,
    draws: usize,
    flushes: usize,
}

impl CountingBackend {
    /// Creates a counting backend at fixed dimensions.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            inner: TestBackend::new(width, height),
            changed_cells: 0,
            draws: 0,
            flushes: 0,
        }
    }

    /// Clears counters without changing logical terminal contents.
    pub fn reset_counts(&mut self) {
        self.changed_cells = 0;
        self.draws = 0;
        self.flushes = 0;
    }

    /// Returns cells submitted by Ratatui's latest measured diffs.
    #[must_use]
    pub const fn changed_cells(&self) -> usize {
        self.changed_cells
    }

    /// Returns backend draw calls.
    #[must_use]
    pub const fn draws(&self) -> usize {
        self.draws
    }

    /// Returns backend flush calls.
    #[must_use]
    pub const fn flushes(&self) -> usize {
        self.flushes
    }
}

impl Backend for CountingBackend {
    type Error = Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        self.draws = self.draws.saturating_add(1);
        let changed_cells = &mut self.changed_cells;
        self.inner.draw(content.inspect(|_| {
            *changed_cells = changed_cells.saturating_add(1);
        }))
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.flushes = self.flushes.saturating_add(1);
        self.inner.flush()
    }
}

impl RatatuiCollectionLab {
    /// Creates a generated collection with explicit terminal dimensions.
    #[must_use]
    pub fn new(mode: CollectionMode, item_count: usize, width: u16, height: u16) -> Self {
        let items: Vec<_> = (0..item_count)
            .map(|index| {
                let key = u64::try_from(index).unwrap_or(u64::MAX);
                let height = NonZeroUsize::new(index % 3 + 1).unwrap_or(NonZeroUsize::MIN);
                Item {
                    key,
                    fixed_label: format!("Item {key:06}"),
                    variable_label: (0..height.get())
                        .map(|line| format!("Item {key:06} / line {}", line + 1))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    height,
                }
            })
            .collect();
        let fixed = FixedHeightProvider::new(items.len(), NonZeroUsize::MIN, OVERSCAN_ROWS);
        let variable = VariableHeightProvider::new(
            items.iter().map(|item| (item.key, item.height)),
            OVERSCAN_CELLS,
        );
        Self {
            active: items.first().map(|item| item.key),
            items,
            mode,
            fixed,
            variable,
            terminal_width: width,
            terminal_height: height,
            viewport_height: usize::from(height.saturating_sub(4).max(1)),
            scroll: 0,
            selected: None,
            constructed_rows: 0,
        }
    }

    /// Applies one application action without drawing.
    pub fn apply(&mut self, action: ComparisonAction) {
        match action {
            ComparisonAction::Up
            | ComparisonAction::Down
            | ComparisonAction::Home
            | ComparisonAction::End
            | ComparisonAction::PageUp
            | ComparisonAction::PageDown => self.move_active(action),
            ComparisonAction::SelectActive => self.selected = self.active,
            ComparisonAction::ToggleMode => {
                self.mode = match self.mode {
                    CollectionMode::Fixed => CollectionMode::Variable,
                    CollectionMode::Variable => CollectionMode::Fixed,
                };
                self.scroll = 0;
                if let Some(index) = self.active_index() {
                    self.reveal(index);
                }
            }
            ComparisonAction::Reverse => {
                self.items.reverse();
                self.rebuild_providers();
            }
            ComparisonAction::Resize { width, height } => {
                self.terminal_width = width;
                self.terminal_height = height;
                self.viewport_height = usize::from(height.saturating_sub(4).max(1));
                self.scroll = self.scroll.min(self.max_scroll());
                if let Some(index) = self.active_index() {
                    self.reveal(index);
                }
            }
        }
    }

    /// Returns the dimensions expected by the next draw.
    #[must_use]
    pub const fn terminal_size(&self) -> (u16, u16) {
        (self.terminal_width, self.terminal_height)
    }

    /// Returns current observable application state.
    #[must_use]
    pub fn semantic_state(&self) -> SemanticState {
        let visible_range = self.visible_range();
        SemanticState {
            active_key: self.active,
            selected_key: self.selected,
            scroll_offset: self.scroll,
            viewport_height: self.viewport_height,
            visible_range,
            constructed_rows: self.constructed_rows,
        }
    }

    /// Paints one complete desired frame.
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let range = self.visible_range();
        self.constructed_rows = range.len();
        let area = frame.area();
        let buffer = frame.buffer_mut();
        paint_clipped(buffer, area.x, area.y, HEADER, Style::default());
        self.paint_panel(buffer, area, range);
    }

    fn paint_panel(&self, buffer: &mut Buffer, area: Rect, range: VisibleRange) {
        if area.width == 0 || area.height < 2 {
            return;
        }
        let panel_y = area.y.saturating_add(1);
        let panel_height = u16::try_from(self.viewport_height.saturating_add(2))
            .unwrap_or(u16::MAX)
            .min(area.height.saturating_sub(1));
        if panel_height < 2 {
            return;
        }
        let border_style = Style::new().fg(Color::LightCyan);
        paint_horizontal_border(buffer, area.x, panel_y, area.width, '┌', '┐', border_style);
        let title = match self.mode {
            CollectionMode::Fixed => " Fixed-height visible range ",
            CollectionMode::Variable => " Variable-height visible range ",
        };
        paint_clipped(
            buffer,
            area.x.saturating_add(1),
            panel_y,
            title,
            border_style,
        );
        let bottom = panel_y.saturating_add(panel_height.saturating_sub(1));
        paint_horizontal_border(buffer, area.x, bottom, area.width, '└', '┘', border_style);
        for y in panel_y.saturating_add(1)..bottom {
            buffer[(area.x, y)].set_symbol("│").set_style(border_style);
            if area.width > 1 {
                buffer[(area.x.saturating_add(area.width - 1), y)]
                    .set_symbol("│")
                    .set_style(border_style);
            }
        }

        let content_x = area.x.saturating_add(1);
        let content_width = area.width.saturating_sub(2);
        let content_top = i32::from(panel_y.saturating_add(1));
        let content_bottom = i32::from(bottom);
        let mut logical_y =
            content_top.saturating_sub(i32::try_from(range.local_offset()).unwrap_or(i32::MAX));
        for item in &self.items[range.start()..range.end()] {
            let label = match self.mode {
                CollectionMode::Fixed => &item.fixed_label,
                CollectionMode::Variable => &item.variable_label,
            };
            let mut style = Style::new();
            if self.selected == Some(item.key) {
                style = style.bg(Color::Blue).fg(Color::White);
            }
            if self.active == Some(item.key) {
                style = style.fg(Color::LightYellow).add_modifier(Modifier::BOLD);
            }
            for line in label.lines() {
                if logical_y >= content_top && logical_y < content_bottom {
                    let y = u16::try_from(logical_y).unwrap_or(u16::MAX);
                    buffer.set_style(Rect::new(content_x, y, content_width, 1), style);
                    paint_clipped(buffer, content_x, y, line, style);
                }
                logical_y = logical_y.saturating_add(1);
            }
        }
    }

    fn visible_range(&self) -> VisibleRange {
        match self.mode {
            CollectionMode::Fixed => self.fixed.visible_range(self.scroll, self.viewport_height),
            CollectionMode::Variable => self
                .variable
                .visible_range(self.scroll, self.viewport_height),
        }
    }

    fn max_scroll(&self) -> usize {
        match self.mode {
            CollectionMode::Fixed => self.fixed.max_scroll(self.viewport_height),
            CollectionMode::Variable => self.variable.max_scroll(self.viewport_height),
        }
    }

    fn active_index(&self) -> Option<usize> {
        let active = self.active?;
        self.items.iter().position(|item| item.key == active)
    }

    fn index_for_offset(&self, offset: usize) -> usize {
        match self.mode {
            CollectionMode::Fixed => offset.min(self.items.len().saturating_sub(1)),
            CollectionMode::Variable => self
                .variable
                .item_index_at_offset(offset)
                .unwrap_or_default(),
        }
    }

    fn item_bounds(&self, index: usize) -> Option<(usize, usize)> {
        match self.mode {
            CollectionMode::Fixed => Some((index, index.saturating_add(1))),
            CollectionMode::Variable => {
                let top = self.variable.item_offset(index)?;
                let height = self.variable.height(index)?.get();
                Some((top, top.saturating_add(height)))
            }
        }
    }

    fn move_active(&mut self, action: ComparisonAction) {
        if self.items.is_empty() {
            return;
        }
        let current = self.active_index().unwrap_or_default();
        let last = self.items.len().saturating_sub(1);
        let target = match action {
            ComparisonAction::Up => current.saturating_sub(1),
            ComparisonAction::Down => current.saturating_add(1).min(last),
            ComparisonAction::Home => 0,
            ComparisonAction::End => last,
            ComparisonAction::PageUp => {
                self.index_for_offset(self.scroll.saturating_sub(self.viewport_height))
            }
            ComparisonAction::PageDown => self.index_for_offset(
                self.scroll
                    .saturating_add(self.viewport_height)
                    .min(self.max_scroll()),
            ),
            _ => current,
        };
        self.active = Some(self.items[target].key);
        self.reveal(target);
    }

    fn reveal(&mut self, index: usize) {
        let Some((top, bottom)) = self.item_bounds(index) else {
            return;
        };
        if top < self.scroll {
            self.scroll = top;
        } else if bottom > self.scroll.saturating_add(self.viewport_height) {
            self.scroll = bottom.saturating_sub(self.viewport_height);
        }
        self.scroll = self.scroll.min(self.max_scroll());
    }

    fn rebuild_providers(&mut self) {
        self.fixed = FixedHeightProvider::new(self.items.len(), NonZeroUsize::MIN, OVERSCAN_ROWS);
        self.variable = VariableHeightProvider::new(
            self.items.iter().map(|item| (item.key, item.height)),
            OVERSCAN_CELLS,
        );
        self.scroll = self.scroll.min(self.max_scroll());
        if let Some(index) = self.active_index() {
            self.reveal(index);
        }
    }
}

/// Draws one application frame into Ratatui's logical test terminal.
pub fn draw_test_frame(
    terminal: &mut Terminal<TestBackend>,
    application: &mut RatatuiCollectionLab,
) -> Result<String, Infallible> {
    draw_test_terminal(terminal, application)?;
    Ok(buffer_characters(terminal.backend().buffer()))
}

/// Draws without materializing a character snapshot.
pub fn draw_test_terminal(
    terminal: &mut Terminal<TestBackend>,
    application: &mut RatatuiCollectionLab,
) -> Result<(), Infallible> {
    draw_terminal(terminal, application)
}

/// Draws one complete desired frame through any Ratatui backend.
pub fn draw_terminal<B: Backend>(
    terminal: &mut Terminal<B>,
    application: &mut RatatuiCollectionLab,
) -> Result<(), B::Error> {
    terminal.draw(|frame| application.render(frame))?;
    Ok(())
}

/// Converts a Ratatui buffer to ArborUI's full-width character snapshot format.
#[must_use]
pub fn buffer_characters(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut output = String::new();
    for y in area.top()..area.bottom() {
        if y != area.top() {
            output.push('\n');
        }
        for x in area.left()..area.right() {
            output.push_str(buffer[(x, y)].symbol());
        }
    }
    output
}

fn paint_horizontal_border(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    left: char,
    right: char,
    style: Style,
) {
    if width == 0 {
        return;
    }
    buffer[(x, y)].set_char(left).set_style(style);
    for offset in 1..width.saturating_sub(1) {
        buffer[(x.saturating_add(offset), y)]
            .set_symbol("─")
            .set_style(style);
    }
    if width > 1 {
        buffer[(x.saturating_add(width - 1), y)]
            .set_char(right)
            .set_style(style);
    }
}

fn paint_clipped(buffer: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if y < buffer.area.top() || y >= buffer.area.bottom() || x >= buffer.area.right() {
        return;
    }
    let width = buffer.area.right().saturating_sub(x);
    buffer.set_stringn(x, y, text, usize::from(width), style);
}
