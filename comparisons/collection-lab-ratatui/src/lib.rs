//! Matched Ratatui implementation of ArborUI's Collection Lab experiment.

use std::{convert::Infallible, num::NonZeroUsize};

use arborui::{WidthPolicy, graphemes};
use arborui_example_collection_lab::{
    CollectionMode, FixedHeightProvider, LogAction, LogModel, OverlayAction, OverlayModel,
    TableAction, TableModel, UnicodeAction, UnicodeModel, VariableHeightProvider, VisibleRange,
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, ClearType, TestBackend, WindowSize},
    buffer::Buffer,
    layout::{Constraint, Position, Rect, Size},
    style::{Color, Modifier, Style},
    widgets::{Cell, Row, StatefulWidget, Table, TableState},
};

const OVERSCAN_ROWS: usize = 2;
const OVERSCAN_CELLS: usize = 3;
const HEADER: &str = "Arrows/Page/Home/End move | Enter selects | v mode | r reverse | q quit";
const TABLE_HEADER: &str = "Arrows/Page/Home/End move | Enter selects | u updates | q quit";
const LOG_HEADER: &str = "Up/Down/Page/Home/End scroll | a appends | q quit";
const UNICODE_HEADER: &str = "Arrows shift | r replace | q quit";

/// Deterministic eight-frame resize storm for collection, table, and log workloads.
pub const STANDARD_RESIZE_STORM: [(u16, u16); 8] = [
    (42, 10),
    (56, 16),
    (36, 9),
    (60, 15),
    (46, 11),
    (54, 14),
    (44, 13),
    (48, 12),
];

/// Deterministic eight-frame resize storm for the open overlay workload.
pub const OVERLAY_RESIZE_STORM: [(u16, u16); 8] = [
    (34, 10),
    (48, 16),
    (28, 9),
    (52, 15),
    (38, 11),
    (46, 14),
    (36, 13),
    (40, 12),
];

/// Deterministic eight-frame resize storm for the Unicode clipping workload.
pub const UNICODE_RESIZE_STORM: [(u16, u16); 8] = [
    (30, 10),
    (44, 14),
    (24, 10),
    (48, 13),
    (34, 11),
    (42, 12),
    (32, 10),
    (36, 10),
];

/// Nonzero Unicode cell offset retained throughout the resize-storm trace.
pub const UNICODE_RESIZE_STORM_OFFSET: usize = 8;

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
    /// Move one viewport above the active item's top, advancing at least one item.
    PageUp,
    /// Move one viewport below the active item's top, advancing at least one item.
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

/// Observable state of the matched table workload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TableSemanticState {
    /// Stable active row key.
    pub active_key: Option<u64>,
    /// Stable selected row key.
    pub selected_key: Option<u64>,
    /// Controlled row scroll offset.
    pub scroll_offset: usize,
    /// Application-owned data viewport height.
    pub viewport_height: usize,
    /// Row range constructed by the latest render.
    pub visible_range: VisibleRange,
    /// Number of rows constructed by the latest render.
    pub constructed_rows: usize,
    /// Latest accepted background-update generation.
    pub generation: u64,
}

/// Observable state of the matched scrolling-log workload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogSemanticState {
    /// Controlled row scroll offset.
    pub scroll_offset: usize,
    /// Whether appends keep the viewport at the tail.
    pub follows_tail: bool,
    /// Application-owned data viewport height.
    pub viewport_height: usize,
    /// Record range constructed by the latest render.
    pub visible_range: VisibleRange,
    /// Number of rows constructed by the latest render.
    pub constructed_rows: usize,
    /// Number of records currently retained.
    pub retained_records: usize,
    /// Latest accepted producer generation.
    pub generation: u64,
}

/// Focus target in the matched overlay workload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayFocus {
    /// Control that opens the dialog.
    Open,
    /// Covered background action.
    Background,
    /// Dialog confirmation action.
    Confirm,
    /// Dialog cancellation action.
    Cancel,
}

/// Observable state of the matched overlay workload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlaySemanticState {
    /// Whether the modal dialog is open.
    pub dialog_open: bool,
    /// Number of confirmed actions.
    pub confirmations: u64,
    /// Number of background control activations.
    pub background_activations: u64,
    /// Active focus target.
    pub focus: OverlayFocus,
}

/// Observable state of the matched Unicode clipping workload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodeSemanticState {
    /// Controlled horizontal cell offset.
    pub offset: usize,
    /// Whether the replacement row contains its width-two glyph.
    pub replacement_is_wide: bool,
    /// Complete terminal dimensions.
    pub terminal_size: (u16, u16),
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

/// Ratatui adapter around the framework-neutral table model.
pub struct RatatuiTableLab {
    model: TableModel,
    constructed_rows: usize,
}

/// Ratatui adapter around the framework-neutral scrolling-log model.
pub struct RatatuiLogLab {
    model: LogModel,
    constructed_rows: usize,
}

/// Ratatui adapter around the framework-neutral overlay model.
pub struct RatatuiOverlayLab {
    model: OverlayModel,
    focus: OverlayFocus,
}

/// Ratatui adapter around the framework-neutral Unicode clipping model.
pub struct RatatuiUnicodeLab {
    model: UnicodeModel,
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
            ComparisonAction::PageUp | ComparisonAction::PageDown => {
                let top = self.item_bounds(current).map_or(0, |(top, _)| top);
                // Page from the active position, not the independently controlled viewport.
                if action == ComparisonAction::PageUp {
                    self.index_for_offset(top.saturating_sub(self.viewport_height))
                        .min(current.saturating_sub(1))
                } else {
                    self.index_for_offset(top.saturating_add(self.viewport_height))
                        .max(current.saturating_add(1))
                        .min(last)
                }
            }
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
            // Top-align oversized rows so repeated reveals cannot oscillate.
            self.scroll = bottom.saturating_sub(self.viewport_height).min(top);
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

impl RatatuiTableLab {
    /// Creates a generated table with explicit terminal dimensions.
    #[must_use]
    pub fn new(item_count: usize, width: u16, height: u16) -> Self {
        Self {
            model: TableModel::new(item_count, width, height),
            constructed_rows: 0,
        }
    }

    /// Applies one deterministic application action without drawing.
    pub fn apply(&mut self, action: TableAction) {
        self.model.apply(action);
    }

    /// Returns the dimensions expected by the next draw.
    #[must_use]
    pub const fn terminal_size(&self) -> (u16, u16) {
        self.model.terminal_size()
    }

    /// Returns the shared application model.
    #[must_use]
    pub const fn model(&self) -> &TableModel {
        &self.model
    }

    /// Returns current observable application state.
    #[must_use]
    pub fn semantic_state(&self) -> TableSemanticState {
        TableSemanticState {
            active_key: self.model.active_key(),
            selected_key: self.model.selected_key(),
            scroll_offset: self.model.scroll_offset(),
            viewport_height: self.model.viewport_height(),
            visible_range: self.model.visible_range(),
            constructed_rows: self.constructed_rows,
            generation: self.model.generation(),
        }
    }

    /// Paints one complete desired table frame with Ratatui's stateful table widget.
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let range = self.model.visible_range();
        self.constructed_rows = range.len();
        let area = frame.area();
        let buffer = frame.buffer_mut();
        paint_clipped(buffer, area.x, area.y, TABLE_HEADER, Style::default());
        if area.width == 0 || area.height < 3 {
            return;
        }

        let panel_y = area.y.saturating_add(1);
        let panel_height = u16::try_from(self.model.viewport_height().saturating_add(3))
            .unwrap_or(u16::MAX)
            .min(area.height.saturating_sub(1));
        if panel_height < 3 {
            return;
        }
        let border_style = Style::new().fg(Color::LightCyan);
        paint_horizontal_border(buffer, area.x, panel_y, area.width, '┌', '┐', border_style);
        paint_clipped(
            buffer,
            area.x.saturating_add(1),
            panel_y,
            " Virtualized service table ",
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

        let columns = self.model.columns();
        let rows = self.model.rows()[range.start()..range.end()]
            .iter()
            .map(|record| {
                let style = if self.model.selected_key() == Some(record.key()) {
                    Style::new().bg(Color::Blue).fg(Color::White)
                } else {
                    Style::new()
                };
                Row::new([
                    Cell::from(record.key_display()),
                    Cell::from(record.name()),
                    Cell::from(record.region()),
                    Cell::from(record.status()),
                    Cell::from(record.value()),
                ])
                .style(style)
            });
        let widths = columns.widths().map(Constraint::Length);
        let table = Table::new(rows, widths)
            .header(
                Row::new(["ID", "SERVICE", "REGION", "STATUS", "LATENCY"])
                    .style(Style::new().add_modifier(Modifier::BOLD)),
            )
            .column_spacing(0)
            .row_highlight_style(
                Style::new()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            );
        // Ratatui auto-reveals selected rows. Only highlight one-row records inside the
        // clipped body, excluding both panel borders, the header, and overscan.
        let visible_start = range.start().saturating_add(range.local_offset());
        let visible_end = visible_start
            .saturating_add(usize::from(panel_height.saturating_sub(3)))
            .min(range.end());
        let selected = self
            .model
            .active_key()
            .and_then(|key| usize::try_from(key).ok())
            .filter(|index| area.width > 2 && *index >= visible_start && *index < visible_end)
            .map(|index| index.saturating_sub(range.start()));
        let mut state = TableState::new()
            .with_offset(range.local_offset())
            .with_selected(selected);
        StatefulWidget::render(
            table,
            Rect::new(
                area.x.saturating_add(1),
                panel_y.saturating_add(1),
                area.width.saturating_sub(2),
                panel_height.saturating_sub(2),
            ),
            buffer,
            &mut state,
        );
    }
}

impl RatatuiLogLab {
    /// Creates generated bounded history with explicit terminal dimensions.
    #[must_use]
    pub fn new(initial_count: usize, history_limit: usize, width: u16, height: u16) -> Self {
        Self {
            model: LogModel::new(initial_count, history_limit, width, height),
            constructed_rows: 0,
        }
    }

    /// Applies one deterministic application action without drawing.
    pub fn apply(&mut self, action: LogAction) {
        self.model.apply(action);
    }

    /// Returns the dimensions expected by the next draw.
    #[must_use]
    pub const fn terminal_size(&self) -> (u16, u16) {
        self.model.terminal_size()
    }

    /// Returns the shared application model.
    #[must_use]
    pub const fn model(&self) -> &LogModel {
        &self.model
    }

    /// Returns current observable application state.
    #[must_use]
    pub fn semantic_state(&self) -> LogSemanticState {
        LogSemanticState {
            scroll_offset: self.model.scroll_offset(),
            follows_tail: self.model.follows_tail(),
            viewport_height: self.model.viewport_height(),
            visible_range: self.model.visible_range(),
            constructed_rows: self.constructed_rows,
            retained_records: self.model.records().len(),
            generation: self.model.generation(),
        }
    }

    /// Paints one complete desired scrolling-log frame.
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let range = self.model.visible_range();
        self.constructed_rows = range.len();
        let area = frame.area();
        let buffer = frame.buffer_mut();
        paint_clipped(buffer, area.x, area.y, LOG_HEADER, Style::default());
        if area.width == 0 || area.height < 2 {
            return;
        }

        let panel_y = area.y.saturating_add(1);
        let panel_height = u16::try_from(self.model.viewport_height().saturating_add(2))
            .unwrap_or(u16::MAX)
            .min(area.height.saturating_sub(1));
        if panel_height < 2 {
            return;
        }
        let border_style = Style::new().fg(Color::LightCyan);
        paint_horizontal_border(buffer, area.x, panel_y, area.width, '┌', '┐', border_style);
        paint_clipped(
            buffer,
            area.x.saturating_add(1),
            panel_y,
            " Bounded scrolling log ",
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
        for record in self
            .model
            .records()
            .iter()
            .skip(range.start())
            .take(range.len())
        {
            if logical_y >= content_top && logical_y < content_bottom {
                paint_clipped_width(
                    buffer,
                    content_x,
                    u16::try_from(logical_y).unwrap_or(u16::MAX),
                    record.text(),
                    content_width,
                    Style::new(),
                );
            }
            logical_y = logical_y.saturating_add(1);
        }
    }
}

impl RatatuiOverlayLab {
    /// Creates a closed overlay workload at explicit terminal dimensions.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Self {
        Self {
            model: OverlayModel::new(width, height),
            focus: OverlayFocus::Open,
        }
    }

    /// Applies one shared action and updates focus for modal transitions.
    pub fn apply(&mut self, action: OverlayAction) -> bool {
        match action {
            OverlayAction::Open => self.focus = OverlayFocus::Confirm,
            OverlayAction::Confirm | OverlayAction::Cancel => self.focus = OverlayFocus::Open,
            OverlayAction::ActivateBackground
            | OverlayAction::Resize { .. }
            | OverlayAction::Quit => {}
        }
        self.model.apply(action)
    }

    /// Moves focus forward, wrapping within the active focus scope.
    pub const fn focus_next(&mut self) {
        self.focus = if self.model.dialog_open() {
            match self.focus {
                OverlayFocus::Confirm => OverlayFocus::Cancel,
                _ => OverlayFocus::Confirm,
            }
        } else {
            match self.focus {
                OverlayFocus::Open => OverlayFocus::Background,
                _ => OverlayFocus::Open,
            }
        };
    }

    /// Moves focus backward, wrapping within the active focus scope.
    pub const fn focus_previous(&mut self) {
        self.focus = if self.model.dialog_open() {
            match self.focus {
                OverlayFocus::Confirm => OverlayFocus::Cancel,
                _ => OverlayFocus::Confirm,
            }
        } else {
            match self.focus {
                OverlayFocus::Open => OverlayFocus::Background,
                _ => OverlayFocus::Open,
            }
        };
    }

    /// Returns the dimensions expected by the next draw.
    #[must_use]
    pub const fn terminal_size(&self) -> (u16, u16) {
        self.model.terminal_size()
    }

    /// Returns the shared application model.
    #[must_use]
    pub const fn model(&self) -> &OverlayModel {
        &self.model
    }

    /// Returns current observable application state.
    #[must_use]
    pub const fn semantic_state(&self) -> OverlaySemanticState {
        OverlaySemanticState {
            dialog_open: self.model.dialog_open(),
            confirmations: self.model.confirmations(),
            background_activations: self.model.background_activations(),
            focus: self.focus,
        }
    }

    /// Paints one complete desired overlay frame.
    pub fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let buffer = frame.buffer_mut();
        paint_block(
            buffer,
            area,
            "Overlay composition",
            Style::new().fg(Color::LightCyan),
        );
        paint_clipped(
            buffer,
            area.x.saturating_add(2),
            area.y.saturating_add(2),
            "Persistent workspace",
            Style::new(),
        );
        for (row, label, focus) in [
            (4, "[ Open dialog ]", OverlayFocus::Open),
            (5, "[ Background action ]", OverlayFocus::Background),
        ] {
            let control = Rect::new(
                area.x.saturating_add(2),
                area.y.saturating_add(row),
                area.width.saturating_sub(4),
                1,
            )
            .intersection(area);
            let style = if self.focus == focus {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new()
            };
            // Column children fill their content row, including trailing spaces.
            buffer.set_style(control, style);
            paint_clipped_width(buffer, control.x, control.y, label, control.width, style);
        }
        paint_clipped_width(
            buffer,
            area.x.saturating_add(2),
            area.y.saturating_add(7),
            "Confirmation and background counts are tracked semantically.",
            area.width.saturating_sub(4),
            Style::new(),
        );

        if self.model.dialog_open() {
            // The opaque scrim replaces lower cells, not just their backgrounds.
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    buffer[(x, y)].reset();
                    buffer[(x, y)].set_bg(Color::Black);
                }
            }
            let dialog = Rect::new(
                area.x.saturating_add(area.width.saturating_sub(26) / 2),
                area.y
                    .saturating_add(area.height.saturating_sub(7).saturating_add(1) / 2),
                26.min(area.width),
                7.min(area.height),
            );
            let panel_style = Style::new().fg(Color::White).bg(Color::Black);
            buffer.set_style(dialog, panel_style);
            paint_block(
                buffer,
                dialog,
                "Confirm action",
                // ArborUI's custom border paint does not inherit the panel style.
                Style::reset().fg(Color::LightCyan),
            );
            paint_clipped(
                buffer,
                dialog.x.saturating_add(2),
                dialog.y.saturating_add(2),
                "Delete selected item?",
                Style::new(),
            );
            for (offset, label, focus) in [
                (0, "[ Confirm ]", OverlayFocus::Confirm),
                (13, "[ Cancel ]", OverlayFocus::Cancel),
            ] {
                let style = if self.focus == focus {
                    panel_style.add_modifier(Modifier::REVERSED)
                } else {
                    panel_style
                };
                // The 22-cell content clip leaves 11 Confirm cells and 9 Cancel
                // cells visible; the two-cell gap never receives focus styling.
                paint_clipped_width(
                    buffer,
                    dialog.x.saturating_add(2).saturating_add(offset),
                    dialog.y.saturating_add(4),
                    label,
                    dialog.width.saturating_sub(4).saturating_sub(offset),
                    style,
                );
            }
        }
    }
}

impl RatatuiUnicodeLab {
    /// Creates the deterministic Unicode workload at explicit terminal dimensions.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            model: UnicodeModel::new(width, height),
        }
    }

    /// Applies one deterministic application action without drawing.
    pub fn apply(&mut self, action: UnicodeAction) -> bool {
        self.model.apply(action)
    }

    /// Returns the dimensions expected by the next draw.
    #[must_use]
    pub const fn terminal_size(&self) -> (u16, u16) {
        self.model.terminal_size()
    }

    /// Returns the shared application model.
    #[must_use]
    pub const fn model(&self) -> &UnicodeModel {
        &self.model
    }

    /// Returns current observable application state.
    #[must_use]
    pub const fn semantic_state(&self) -> UnicodeSemanticState {
        UnicodeSemanticState {
            offset: self.model.offset(),
            replacement_is_wide: self.model.replacement_is_wide(),
            terminal_size: self.model.terminal_size(),
        }
    }

    /// Paints one complete desired Unicode clipping frame.
    pub fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let buffer = frame.buffer_mut();
        paint_clipped(buffer, area.x, area.y, UNICODE_HEADER, Style::default());
        if area.width == 0 || area.height < 2 {
            return;
        }

        let panel = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            9.min(area.height - 1),
        );
        paint_block(
            buffer,
            panel,
            "Unicode cell clipping",
            Style::new().fg(Color::LightCyan),
        );
        let content_width = panel.width.saturating_sub(2);
        let content_x = panel.x.saturating_add(1);
        for (index, row) in self.model.rows().iter().enumerate() {
            let y = panel
                .y
                .saturating_add(1)
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
            if y >= panel.bottom().saturating_sub(1) {
                break;
            }
            for x in content_x..content_x.saturating_add(content_width) {
                buffer[(x, y)].set_symbol(" ");
            }
            paint_unicode_clipped(
                buffer,
                content_x,
                y,
                row,
                self.model.offset(),
                content_width,
            );
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

/// Draws one table frame into Ratatui's logical test terminal.
pub fn draw_test_table_frame(
    terminal: &mut Terminal<TestBackend>,
    application: &mut RatatuiTableLab,
) -> Result<String, Infallible> {
    draw_test_table_terminal(terminal, application)?;
    Ok(buffer_characters(terminal.backend().buffer()))
}

/// Draws a table without materializing a character snapshot.
pub fn draw_test_table_terminal(
    terminal: &mut Terminal<TestBackend>,
    application: &mut RatatuiTableLab,
) -> Result<(), Infallible> {
    draw_table_terminal(terminal, application)
}

/// Draws one complete table frame through any Ratatui backend.
pub fn draw_table_terminal<B: Backend>(
    terminal: &mut Terminal<B>,
    application: &mut RatatuiTableLab,
) -> Result<(), B::Error> {
    terminal.draw(|frame| application.render(frame))?;
    Ok(())
}

/// Draws one scrolling-log frame into Ratatui's logical test terminal.
pub fn draw_test_log_frame(
    terminal: &mut Terminal<TestBackend>,
    application: &mut RatatuiLogLab,
) -> Result<String, Infallible> {
    draw_test_log_terminal(terminal, application)?;
    Ok(buffer_characters(terminal.backend().buffer()))
}

/// Draws a scrolling log without materializing a character snapshot.
pub fn draw_test_log_terminal(
    terminal: &mut Terminal<TestBackend>,
    application: &mut RatatuiLogLab,
) -> Result<(), Infallible> {
    draw_log_terminal(terminal, application)
}

/// Draws one complete scrolling-log frame through any Ratatui backend.
pub fn draw_log_terminal<B: Backend>(
    terminal: &mut Terminal<B>,
    application: &mut RatatuiLogLab,
) -> Result<(), B::Error> {
    terminal.draw(|frame| application.render(frame))?;
    Ok(())
}

/// Draws one overlay frame into Ratatui's logical test terminal.
pub fn draw_test_overlay_frame(
    terminal: &mut Terminal<TestBackend>,
    application: &RatatuiOverlayLab,
) -> Result<String, Infallible> {
    draw_test_overlay_terminal(terminal, application)?;
    Ok(buffer_characters(terminal.backend().buffer()))
}

/// Draws an overlay without materializing a character snapshot.
pub fn draw_test_overlay_terminal(
    terminal: &mut Terminal<TestBackend>,
    application: &RatatuiOverlayLab,
) -> Result<(), Infallible> {
    draw_overlay_terminal(terminal, application)
}

/// Draws one complete overlay frame through any Ratatui backend.
pub fn draw_overlay_terminal<B: Backend>(
    terminal: &mut Terminal<B>,
    application: &RatatuiOverlayLab,
) -> Result<(), B::Error> {
    terminal.draw(|frame| application.render(frame))?;
    Ok(())
}

/// Draws one Unicode frame into Ratatui's logical test terminal.
pub fn draw_test_unicode_frame(
    terminal: &mut Terminal<TestBackend>,
    application: &RatatuiUnicodeLab,
) -> Result<String, Infallible> {
    let completed = terminal.draw(|frame| application.render(frame))?;
    Ok(buffer_unicode_characters(completed.buffer))
}

/// Draws the Unicode workload without materializing a character snapshot.
pub fn draw_test_unicode_terminal(
    terminal: &mut Terminal<TestBackend>,
    application: &RatatuiUnicodeLab,
) -> Result<(), Infallible> {
    draw_unicode_terminal(terminal, application)
}

/// Draws one complete Unicode frame through any Ratatui backend.
pub fn draw_unicode_terminal<B: Backend>(
    terminal: &mut Terminal<B>,
    application: &RatatuiUnicodeLab,
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

fn buffer_unicode_characters(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut output = String::new();
    for y in area.top()..area.bottom() {
        if y != area.top() {
            output.push('\n');
        }
        let mut x = area.left();
        while x < area.right() {
            let symbol = buffer[(x, y)].symbol();
            output.push_str(symbol);
            let width = graphemes(symbol, WidthPolicy::Unicode)
                .next()
                .map_or(1, |grapheme| grapheme.width.max(1));
            x = x.saturating_add(u16::try_from(width).unwrap_or(u16::MAX));
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

fn paint_block(buffer: &mut Buffer, area: Rect, title: &str, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    paint_horizontal_border(buffer, area.x, area.y, area.width, '┌', '┐', style);
    paint_clipped_width(
        buffer,
        area.x.saturating_add(1),
        area.y,
        &format!(" {title} "),
        area.width.saturating_sub(2),
        style,
    );
    if area.height == 1 {
        return;
    }
    let bottom = area.y.saturating_add(area.height - 1);
    paint_horizontal_border(buffer, area.x, bottom, area.width, '└', '┘', style);
    for y in area.y.saturating_add(1)..bottom {
        buffer[(area.x, y)].set_symbol("│").set_style(style);
        if area.width > 1 {
            buffer[(area.x.saturating_add(area.width - 1), y)]
                .set_symbol("│")
                .set_style(style);
        }
    }
}

fn paint_clipped(buffer: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if y < buffer.area.top() || y >= buffer.area.bottom() || x >= buffer.area.right() {
        return;
    }
    let width = buffer.area.right().saturating_sub(x);
    buffer.set_stringn(x, y, text, usize::from(width), style);
}

fn paint_clipped_width(buffer: &mut Buffer, x: u16, y: u16, text: &str, width: u16, style: Style) {
    if y < buffer.area.top() || y >= buffer.area.bottom() || x >= buffer.area.right() {
        return;
    }
    let width = width.min(buffer.area.right().saturating_sub(x));
    buffer.set_stringn(x, y, text, usize::from(width), style);
}

fn paint_unicode_clipped(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    value: &str,
    offset: usize,
    width: u16,
) {
    let mut logical_x = 0usize;
    for grapheme in graphemes(value, WidthPolicy::Unicode) {
        let width_of_grapheme = grapheme.width;
        let end = logical_x.saturating_add(width_of_grapheme);
        if logical_x >= offset {
            let screen_x = logical_x.saturating_sub(offset);
            if screen_x.saturating_add(width_of_grapheme) > usize::from(width) {
                break;
            }
            buffer.set_stringn(
                x.saturating_add(u16::try_from(screen_x).unwrap_or(u16::MAX)),
                y,
                grapheme.text,
                width_of_grapheme,
                Style::new(),
            );
        }
        logical_x = end;
        if logical_x.saturating_sub(offset) >= usize::from(width) {
            break;
        }
    }
}
