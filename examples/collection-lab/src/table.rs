use std::{cell::Cell, num::NonZeroUsize};

use arborui::{
    Color, Element, EventPhase, KeyAction, Modifier, PointerButton, PointerEventKind, Style,
    UiEvent, UiKey,
    layout::{Dimension, FlexDirection, LayoutStyle},
    prelude::{
        Application, Block, Command, Invalidation, Point, UpdateContext, column, list,
        row_with_gap, scroll_view,
    },
    widgets::text,
};

use crate::{FixedHeightProvider, VisibleRange};

const TABLE_OVERSCAN_ROWS: usize = 2;
const HELP: &str = "Arrows/Page/Home/End move | Enter selects | u updates | q quit";
const REGIONS: [&str; 8] = [
    "Tokyo",
    "München",
    "São Paulo",
    "Zürich",
    "Québec",
    "Kraków",
    "Bogotá",
    "Δelta",
];

/// One generated row in the application-level table experiment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRecord {
    key: u64,
    key_display: String,
    name: String,
    region: &'static str,
    status: &'static str,
    value: String,
    revision: u64,
}

impl TableRecord {
    /// Returns the stable row key.
    #[must_use]
    pub const fn key(&self) -> u64 {
        self.key
    }

    /// Returns the stable key formatted for display.
    #[must_use]
    pub fn key_display(&self) -> &str {
        &self.key_display
    }

    /// Returns the generated service name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the Unicode region label.
    #[must_use]
    pub const fn region(&self) -> &'static str {
        self.region
    }

    /// Returns the current status label.
    #[must_use]
    pub const fn status(&self) -> &'static str {
        self.status
    }

    /// Returns the current display value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns the last applied row revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

/// Explicit table column widths shared by both framework renderers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TableColumns {
    key: u16,
    name: u16,
    region: u16,
    status: u16,
    value: u16,
}

impl TableColumns {
    /// Resolves responsive columns for the panel's inner width.
    #[must_use]
    pub fn for_inner_width(inner_width: u16) -> Self {
        let spacing = 4u16.min(inner_width);
        let available = inner_width.saturating_sub(spacing);
        let mut widths = [8u16, 16, 10, 8, 8];
        let minimums = [4u16, 6, 5, 6, 4];
        let shrink_order = [1usize, 2, 4, 0, 3];
        while widths.iter().copied().sum::<u16>() > available {
            let Some(index) = shrink_order
                .iter()
                .copied()
                .find(|index| widths[*index] > minimums[*index])
            else {
                break;
            };
            widths[index] = widths[index].saturating_sub(1);
        }
        Self {
            key: widths[0],
            name: widths[1],
            region: widths[2],
            status: widths[3],
            value: widths[4],
        }
    }

    /// Returns the key-column width.
    #[must_use]
    pub const fn key(self) -> u16 {
        self.key
    }

    /// Returns the name-column width.
    #[must_use]
    pub const fn name(self) -> u16 {
        self.name
    }

    /// Returns the region-column width.
    #[must_use]
    pub const fn region(self) -> u16 {
        self.region
    }

    /// Returns the status-column width.
    #[must_use]
    pub const fn status(self) -> u16 {
        self.status
    }

    /// Returns the value-column width.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.value
    }

    /// Returns all widths in display order.
    #[must_use]
    pub const fn widths(self) -> [u16; 5] {
        [self.key, self.name, self.region, self.status, self.value]
    }
}

/// One deterministic action accepted by the shared table model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableAction {
    /// Move the active row upward.
    Up,
    /// Move the active row downward.
    Down,
    /// Move to the first row.
    Home,
    /// Move to the final row.
    End,
    /// Move upward by one viewport from the active row.
    PageUp,
    /// Move downward by one viewport from the active row.
    PageDown,
    /// Select the active row.
    SelectActive,
    /// Activate and select one stable row key.
    Select(u64),
    /// Apply a signed wheel delta.
    Scrolled(Point),
    /// Change the complete terminal dimensions.
    Resize {
        /// Complete terminal width.
        width: u16,
        /// Complete terminal height.
        height: u16,
    },
    /// Apply a deterministic update from a background producer.
    BackgroundUpdate {
        /// Stable row key to update.
        key: u64,
        /// Monotonic update revision.
        revision: u64,
    },
    /// Request orderly shutdown from the interactive application.
    Quit,
}

/// Framework-neutral state and transition policy for the table workload.
pub struct TableModel {
    rows: Vec<TableRecord>,
    provider: FixedHeightProvider,
    terminal_width: u16,
    terminal_height: u16,
    viewport_height: usize,
    scroll: usize,
    active: Option<u64>,
    selected: Option<u64>,
    generation: u64,
}

impl TableModel {
    /// Creates a generated table at explicit terminal dimensions.
    #[must_use]
    pub fn new(item_count: usize, width: u16, height: u16) -> Self {
        let rows: Vec<_> = (0..item_count)
            .map(|index| {
                let key = u64::try_from(index).unwrap_or(u64::MAX);
                TableRecord {
                    key,
                    key_display: format!("{key:06}"),
                    name: format!("Service {key:06}"),
                    region: REGIONS[index % REGIONS.len()],
                    status: "steady",
                    value: format!("{} ms", 10 + index % 990),
                    revision: 0,
                }
            })
            .collect();
        Self {
            active: rows.first().map(TableRecord::key),
            provider: FixedHeightProvider::new(rows.len(), NonZeroUsize::MIN, TABLE_OVERSCAN_ROWS),
            rows,
            terminal_width: width,
            terminal_height: height,
            viewport_height: table_viewport_height(height),
            scroll: 0,
            selected: None,
            generation: 0,
        }
    }

    /// Applies one action and returns whether it requests shutdown.
    pub fn apply(&mut self, action: TableAction) -> bool {
        match action {
            TableAction::Up
            | TableAction::Down
            | TableAction::Home
            | TableAction::End
            | TableAction::PageUp
            | TableAction::PageDown => self.move_active(action),
            TableAction::SelectActive => self.selected = self.active,
            TableAction::Select(key) => {
                if let Some(index) = self.index_for_key(key) {
                    self.active = Some(key);
                    self.selected = Some(key);
                    self.reveal(index);
                }
            }
            TableAction::Scrolled(delta) => {
                self.scroll = if delta.y.is_negative() {
                    self.scroll.saturating_sub(delta.y.unsigned_abs() as usize)
                } else {
                    self.scroll.saturating_add(delta.y as usize)
                }
                .min(self.max_scroll());
            }
            TableAction::Resize { width, height } => self.resize(width, height),
            TableAction::BackgroundUpdate { key, revision } => {
                if let Some(index) = self.index_for_key(key) {
                    let row = &mut self.rows[index];
                    row.revision = revision;
                    row.status = if revision % 2 == 0 {
                        "steady"
                    } else {
                        "updating"
                    };
                    row.value = format!("{} ms", 10 + (index + revision as usize) % 990);
                    self.generation = self.generation.max(revision);
                }
            }
            TableAction::Quit => return true,
        }
        false
    }

    /// Returns all generated rows.
    #[must_use]
    pub fn rows(&self) -> &[TableRecord] {
        &self.rows
    }

    /// Returns the active stable key.
    #[must_use]
    pub const fn active_key(&self) -> Option<u64> {
        self.active
    }

    /// Returns the selected stable key.
    #[must_use]
    pub const fn selected_key(&self) -> Option<u64> {
        self.selected
    }

    /// Returns the controlled row scroll offset.
    #[must_use]
    pub const fn scroll_offset(&self) -> usize {
        self.scroll
    }

    /// Returns the body viewport height.
    #[must_use]
    pub const fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    /// Returns the latest accepted background-update generation.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the complete terminal dimensions.
    #[must_use]
    pub const fn terminal_size(&self) -> (u16, u16) {
        (self.terminal_width, self.terminal_height)
    }

    /// Returns the responsive table columns for the current terminal width.
    #[must_use]
    pub fn columns(&self) -> TableColumns {
        TableColumns::for_inner_width(self.terminal_width.saturating_sub(2))
    }

    /// Returns the current visible and overscanned row range.
    #[must_use]
    pub fn visible_range(&self) -> VisibleRange {
        self.provider
            .visible_range(self.scroll, self.viewport_height)
    }

    fn index_for_key(&self, key: u64) -> Option<usize> {
        let index = usize::try_from(key).ok()?;
        self.rows.get(index).filter(|row| row.key == key)?;
        Some(index)
    }

    fn active_index(&self) -> Option<usize> {
        self.active.and_then(|key| self.index_for_key(key))
    }

    fn max_scroll(&self) -> usize {
        self.provider.max_scroll(self.viewport_height)
    }

    fn move_active(&mut self, action: TableAction) {
        if self.rows.is_empty() {
            return;
        }
        let current = self.active_index().unwrap_or_default();
        let last = self.rows.len().saturating_sub(1);
        let target = match action {
            TableAction::Up => current.saturating_sub(1),
            TableAction::Down => current.saturating_add(1).min(last),
            TableAction::Home => 0,
            TableAction::End => last,
            TableAction::PageUp => current.saturating_sub(self.viewport_height),
            TableAction::PageDown => current.saturating_add(self.viewport_height).min(last),
            _ => current,
        };
        self.active = Some(self.rows[target].key);
        self.reveal(target);
    }

    fn reveal(&mut self, index: usize) {
        if index < self.scroll {
            self.scroll = index;
        } else if index.saturating_add(1) > self.scroll.saturating_add(self.viewport_height) {
            self.scroll = index.saturating_add(1).saturating_sub(self.viewport_height);
        }
        self.scroll = self.scroll.min(self.max_scroll());
    }

    fn resize(&mut self, width: u16, height: u16) {
        self.terminal_width = width;
        self.terminal_height = height;
        self.viewport_height = table_viewport_height(height);
        self.scroll = self.scroll.min(self.max_scroll());
        if let Some(index) = self.active_index() {
            self.reveal(index);
        }
    }
}

/// Returns the data-row viewport for a complete terminal height.
#[must_use]
pub const fn table_viewport_height(terminal_height: u16) -> usize {
    let height = terminal_height.saturating_sub(5);
    if height == 0 { 1 } else { height as usize }
}

/// Facade-only ArborUI adapter for the shared table model.
pub struct TableLab {
    model: TableModel,
    constructed_rows: Cell<usize>,
}

impl TableLab {
    /// Creates a table application at explicit terminal dimensions.
    #[must_use]
    pub fn new(item_count: usize, width: u16, height: u16) -> Self {
        Self {
            model: TableModel::new(item_count, width, height),
            constructed_rows: Cell::new(0),
        }
    }

    /// Returns the shared application model.
    #[must_use]
    pub const fn model(&self) -> &TableModel {
        &self.model
    }

    /// Returns the number of row elements built by the latest view.
    #[must_use]
    pub fn constructed_rows(&self) -> usize {
        self.constructed_rows.get()
    }
}

impl Application for TableLab {
    type Message = TableAction;

    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        if self.model.apply(message) {
            return Command::quit();
        }
        context.invalidate(Invalidation::Recompose);
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let range = self.model.visible_range();
        self.constructed_rows.set(range.len());
        let columns = self.model.columns();
        let full_width = Dimension::percent(100);
        let rows = self.model.rows()[range.start()..range.end()]
            .iter()
            .map(|record| {
                let mut style = Style::new();
                if self.model.selected_key() == Some(record.key) {
                    style = style.background(Color::Blue).foreground(Color::BrightWhite);
                }
                if self.model.active_key() == Some(record.key) {
                    style = style
                        .foreground(Color::BrightYellow)
                        .add_modifiers(Modifier::BOLD);
                }
                let key = record.key;
                let row = table_row(
                    [
                        record.key_display.as_str(),
                        record.name.as_str(),
                        record.region,
                        record.status,
                        record.value.as_str(),
                    ],
                    columns,
                    style,
                )
                .interactive(true)
                .on_event(EventPhase::Target, move |event, context| {
                    if matches!(
                        event,
                        UiEvent::Pointer(pointer)
                            if pointer.kind == PointerEventKind::Down(PointerButton::Primary)
                    ) {
                        context.emit(TableAction::Select(key));
                        context.mark_handled();
                    }
                });
                (key, row)
            });
        let content = list(rows).layout(LayoutStyle {
            width: full_width,
            height: Dimension::cells(u16::try_from(range.content_height()).unwrap_or(u16::MAX)),
            direction: FlexDirection::Column,
            flex_shrink: 0,
            ..LayoutStyle::default()
        });
        let local_offset = i32::try_from(range.local_offset()).unwrap_or(i32::MAX);
        let body = scroll_view(Point::new(0, local_offset), content)
            .on_scroll(TableAction::Scrolled)
            .layout(LayoutStyle::new().size(
                full_width,
                Dimension::cells(u16::try_from(self.model.viewport_height()).unwrap_or(u16::MAX)),
            ))
            .build();
        let update_key = self.model.active_key().unwrap_or_default();
        let update_revision = self.model.generation().saturating_add(1);
        let table = column([
            table_row(
                ["ID", "SERVICE", "REGION", "STATUS", "LATENCY"],
                columns,
                Style::new().add_modifiers(Modifier::BOLD),
            ),
            body,
        ])
        .key("table")
        .focusable(true)
        .focus_style(Style::new().add_modifiers(Modifier::REVERSED))
        .on_event(EventPhase::Target, move |event, context| {
            let message = match event {
                UiEvent::Key(key) if matches!(key.action, KeyAction::Press | KeyAction::Repeat) => {
                    match key.key {
                        UiKey::Up => Some(TableAction::Up),
                        UiKey::Down => Some(TableAction::Down),
                        UiKey::Home => Some(TableAction::Home),
                        UiKey::End => Some(TableAction::End),
                        UiKey::PageUp => Some(TableAction::PageUp),
                        UiKey::PageDown => Some(TableAction::PageDown),
                        UiKey::Enter | UiKey::Character(' ') => Some(TableAction::SelectActive),
                        UiKey::Character('u') => Some(TableAction::BackgroundUpdate {
                            key: update_key,
                            revision: update_revision,
                        }),
                        UiKey::Character('q') | UiKey::Escape => Some(TableAction::Quit),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(message) = message {
                context.emit(message);
                context.mark_handled();
                context.prevent_default();
            }
        });
        let panel = Block::new(table)
            .title("Virtualized service table")
            .border_style(Style::new().foreground(Color::BrightCyan))
            .layout(
                LayoutStyle::new().size(
                    full_width,
                    Dimension::cells(
                        u16::try_from(self.model.viewport_height().saturating_add(3))
                            .unwrap_or(u16::MAX),
                    ),
                ),
            )
            .build();

        column([text(HELP), panel])
            .layout(LayoutStyle {
                width: full_width,
                height: Dimension::percent(100),
                direction: FlexDirection::Column,
                ..LayoutStyle::default()
            })
            .on_event(EventPhase::Capture, |event, context| {
                if let UiEvent::Resize(size) = event {
                    context.emit(TableAction::Resize {
                        width: size.width,
                        height: size.height,
                    });
                }
            })
    }
}

fn table_row(values: [&str; 5], columns: TableColumns, style: Style) -> Element<'_, TableAction> {
    let cells = values
        .into_iter()
        .zip(columns.widths())
        .map(|(value, width)| {
            text(value).style(style).layout(LayoutStyle {
                width: Dimension::cells(width),
                height: Dimension::cells(1),
                flex_shrink: 0,
                ..LayoutStyle::default()
            })
        });
    row_with_gap(cells, 1).layout(LayoutStyle {
        width: Dimension::percent(100),
        height: Dimension::cells(1),
        flex_shrink: 0,
        ..LayoutStyle::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging_table_repeated_down_reaches_last_row() {
        let mut model = TableModel::new(100, 48, 12);
        assert_eq!(model.viewport_height(), 7);
        for expected in (7..=98).step_by(7).chain([99, 99]) {
            model.apply(TableAction::PageDown);
            assert_eq!(model.active_key(), Some(expected));
            assert_eq!(model.scroll_offset(), expected as usize - 6);
        }
    }

    #[test]
    fn paging_table_up_from_end_reaches_first_row() {
        let mut model = TableModel::new(100, 48, 12);
        model.apply(TableAction::End);
        for expected in (0..=13).rev().map(|page| 1 + page * 7).chain([0, 0]) {
            model.apply(TableAction::PageUp);
            assert_eq!(model.active_key(), Some(expected));
            assert_eq!(model.scroll_offset(), expected as usize);
        }
    }

    #[test]
    fn paging_table_ignores_wheel_detachment_and_preserves_selection() {
        let mut model = TableModel::new(100, 48, 12);
        model.apply(TableAction::SelectActive);
        model.apply(TableAction::Scrolled(Point::new(0, i32::MAX)));
        assert_eq!(model.scroll_offset(), 93);
        model.apply(TableAction::PageDown);
        assert_eq!(model.active_key(), Some(7));
        assert_eq!(model.scroll_offset(), 7);
        model.apply(TableAction::End);
        model.apply(TableAction::Scrolled(Point::new(0, i32::MIN)));
        assert_eq!(model.scroll_offset(), 0);
        model.apply(TableAction::PageUp);
        assert_eq!(model.active_key(), Some(92));
        assert_eq!(model.scroll_offset(), 86);
        assert_eq!(model.selected_key(), Some(0));
    }

    #[test]
    fn paging_table_empty_single_underfilled_and_saturated_viewports() {
        for viewport in [1, 7, usize::MAX] {
            for count in [0usize, 1, 3] {
                let mut model = TableModel::new(count, 48, 12);
                model.viewport_height = viewport;
                for step in 1usize..=4 {
                    model.apply(TableAction::PageDown);
                    let expected = count
                        .checked_sub(1)
                        .map(|last| step.saturating_mul(viewport).min(last) as u64);
                    assert_eq!(model.active_key(), expected);
                    assert!(model.scroll_offset() <= model.max_scroll());
                }
                for _ in 0..4 {
                    model.apply(TableAction::PageUp);
                }
                assert_eq!(model.active_key(), (count > 0).then_some(0));
                assert_eq!(model.scroll_offset(), 0);
            }
        }
    }

    #[test]
    fn table_model_preserves_selection_across_updates_and_resize() {
        let mut model = TableModel::new(100, 48, 12);
        model.apply(TableAction::SelectActive);
        model.apply(TableAction::End);
        model.apply(TableAction::BackgroundUpdate {
            key: 0,
            revision: 3,
        });
        model.apply(TableAction::Resize {
            width: 34,
            height: 9,
        });

        assert_eq!(model.active_key(), Some(99));
        assert_eq!(model.selected_key(), Some(0));
        assert_eq!(model.generation(), 3);
        assert_eq!(model.rows()[0].status(), "updating");
        assert_eq!(model.viewport_height(), 4);
        assert!(model.columns().name() < 16);
    }
}
