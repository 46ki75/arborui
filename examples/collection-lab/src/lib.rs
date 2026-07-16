//! Fixed and variable-height visible-range experiments using only `arborui`.

use std::{cell::Cell, num::NonZeroUsize};

use arborui::{
    Color, Element, EventPhase, KeyAction, Modifier, PointerButton, PointerEventKind, Size, Style,
    UiEvent, UiKey,
    layout::{Dimension, FlexDirection, LayoutStyle},
    prelude::{Application, Block, Command, Invalidation, Point, UpdateContext, list, scroll_view},
    widgets::text,
};

const OVERSCAN_ROWS: usize = 2;
const OVERSCAN_CELLS: usize = 3;

/// One half-open item range constructed for a viewport.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VisibleRange {
    start: usize,
    end: usize,
    local_offset: usize,
    content_height: usize,
}

impl VisibleRange {
    /// Returns the first constructed item index.
    #[must_use]
    pub const fn start(self) -> usize {
        self.start
    }

    /// Returns the index after the final constructed item.
    #[must_use]
    pub const fn end(self) -> usize {
        self.end
    }

    /// Returns the number of constructed items.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Returns whether no items are constructed.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Returns the viewport offset relative to the constructed window.
    #[must_use]
    pub const fn local_offset(self) -> usize {
        self.local_offset
    }

    /// Returns the measured height of the constructed window.
    #[must_use]
    pub const fn content_height(self) -> usize {
        self.content_height
    }
}

/// Visible-range calculator for uniformly sized rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedHeightProvider {
    item_count: usize,
    row_height: NonZeroUsize,
    overscan_rows: usize,
}

impl FixedHeightProvider {
    /// Creates a fixed-height provider.
    #[must_use]
    pub const fn new(item_count: usize, row_height: NonZeroUsize, overscan_rows: usize) -> Self {
        Self {
            item_count,
            row_height,
            overscan_rows,
        }
    }

    /// Returns the logical content height.
    #[must_use]
    pub const fn content_height(self) -> usize {
        self.item_count.saturating_mul(self.row_height.get())
    }

    /// Returns the greatest valid scroll offset for a viewport height.
    #[must_use]
    pub const fn max_scroll(self, viewport_height: usize) -> usize {
        self.content_height().saturating_sub(viewport_height)
    }

    /// Calculates the visible and overscanned item range.
    #[must_use]
    pub fn visible_range(self, scroll: usize, viewport_height: usize) -> VisibleRange {
        if self.item_count == 0 || viewport_height == 0 {
            return VisibleRange::default();
        }

        let row_height = self.row_height.get();
        let scroll = scroll.min(self.max_scroll(viewport_height));
        let first_visible = scroll / row_height;
        let visible_bottom = scroll.saturating_add(viewport_height);
        let end_visible = visible_bottom.saturating_add(row_height.saturating_sub(1)) / row_height;
        let start = first_visible.saturating_sub(self.overscan_rows);
        let end = end_visible
            .saturating_add(self.overscan_rows)
            .min(self.item_count);

        VisibleRange {
            start,
            end,
            local_offset: scroll.saturating_sub(start.saturating_mul(row_height)),
            content_height: end.saturating_sub(start).saturating_mul(row_height),
        }
    }
}

/// A measured location retained while variable row heights change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeightAnchor {
    item_key: u64,
    intra_item_offset: usize,
}

impl HeightAnchor {
    /// Returns the anchored stable item key.
    #[must_use]
    pub const fn item_key(self) -> u64 {
        self.item_key
    }

    /// Returns the offset within the anchored item.
    #[must_use]
    pub const fn intra_item_offset(self) -> usize {
        self.intra_item_offset
    }
}

/// Visible-range calculator backed by cached measured row heights.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VariableHeightProvider {
    keys: Vec<u64>,
    heights: Vec<NonZeroUsize>,
    prefix: Vec<usize>,
    overscan_cells: usize,
}

impl VariableHeightProvider {
    /// Creates a provider from stable keys and non-zero cached item heights.
    #[must_use]
    pub fn new(
        items: impl IntoIterator<Item = (u64, NonZeroUsize)>,
        overscan_cells: usize,
    ) -> Self {
        let (keys, heights): (Vec<_>, Vec<_>) = items.into_iter().unzip();
        let mut prefix = Vec::with_capacity(heights.len().saturating_add(1));
        prefix.push(0usize);
        for height in &heights {
            let next = prefix
                .last()
                .copied()
                .unwrap_or_default()
                .saturating_add(height.get());
            prefix.push(next);
        }
        Self {
            keys,
            heights,
            prefix,
            overscan_cells,
        }
    }

    /// Returns the number of measured items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.heights.len()
    }

    /// Returns whether no measured items are cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// Returns the logical content height.
    #[must_use]
    pub fn content_height(&self) -> usize {
        self.prefix.last().copied().unwrap_or_default()
    }

    /// Returns the greatest valid scroll offset for a viewport height.
    #[must_use]
    pub fn max_scroll(&self, viewport_height: usize) -> usize {
        self.content_height().saturating_sub(viewport_height)
    }

    /// Returns the cached height of one item.
    #[must_use]
    pub fn height(&self, index: usize) -> Option<NonZeroUsize> {
        self.heights.get(index).copied()
    }

    /// Returns one item's logical top edge.
    #[must_use]
    pub fn item_offset(&self, index: usize) -> Option<usize> {
        self.prefix
            .get(index)
            .copied()
            .filter(|_| index < self.len())
    }

    /// Returns the item containing one logical content offset.
    #[must_use]
    pub fn item_index_at_offset(&self, offset: usize) -> Option<usize> {
        self.item_at_offset(offset)
    }

    /// Captures the item and intra-item offset at a scroll position.
    #[must_use]
    pub fn anchor(&self, scroll: usize) -> Option<HeightAnchor> {
        let index = self.item_at_offset(scroll)?;
        Some(HeightAnchor {
            item_key: self.keys[index],
            intra_item_offset: scroll.saturating_sub(self.prefix[index]),
        })
    }

    /// Updates one cached measurement and returns scroll preserving an anchor.
    pub fn update_height(
        &mut self,
        index: usize,
        height: NonZeroUsize,
        anchor: HeightAnchor,
        viewport_height: usize,
    ) -> Option<usize> {
        let stored = self.heights.get_mut(index)?;
        *stored = height;
        for suffix_index in index.saturating_add(1)..self.prefix.len() {
            self.prefix[suffix_index] =
                self.prefix[suffix_index - 1].saturating_add(self.heights[suffix_index - 1].get());
        }
        let anchor_index = self
            .keys
            .iter()
            .position(|item_key| *item_key == anchor.item_key)?;
        let anchor_height = self.heights.get(anchor_index)?.get();
        let intra = anchor
            .intra_item_offset
            .min(anchor_height.saturating_sub(1));
        Some(
            self.prefix[anchor_index]
                .saturating_add(intra)
                .min(self.max_scroll(viewport_height)),
        )
    }

    /// Calculates the visible range with cell-based overscan.
    #[must_use]
    pub fn visible_range(&self, scroll: usize, viewport_height: usize) -> VisibleRange {
        if self.is_empty() || viewport_height == 0 {
            return VisibleRange::default();
        }

        let scroll = scroll.min(self.max_scroll(viewport_height));
        let overscan_top = scroll.saturating_sub(self.overscan_cells);
        let start = self.item_at_offset(overscan_top).unwrap_or_default();
        let overscan_bottom = scroll
            .saturating_add(viewport_height)
            .saturating_add(self.overscan_cells)
            .min(self.content_height());
        let end = self
            .prefix
            .partition_point(|offset| *offset < overscan_bottom)
            .min(self.len());

        VisibleRange {
            start,
            end,
            local_offset: scroll.saturating_sub(self.prefix[start]),
            content_height: self.prefix[end].saturating_sub(self.prefix[start]),
        }
    }

    fn item_at_offset(&self, offset: usize) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let offset = offset.min(self.content_height().saturating_sub(1));
        Some(
            self.prefix
                .partition_point(|item_offset| *item_offset <= offset)
                .saturating_sub(1)
                .min(self.len().saturating_sub(1)),
        )
    }
}

/// Height strategy exercised by [`CollectionLab`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CollectionMode {
    /// Every row occupies one cell.
    #[default]
    Fixed,
    /// Rows use cached explicit heights from one to three cells.
    Variable,
}

#[derive(Clone, Debug)]
struct Item {
    key: u64,
    fixed_label: String,
    variable_label: String,
    height: NonZeroUsize,
}

/// Messages accepted by [`CollectionLab`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Message {
    /// Move the active item one row upward.
    Up,
    /// Move the active item one row downward.
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
    /// Activate and select one stable item key.
    Select(u64),
    /// Apply a signed wheel delta.
    Scrolled(Point),
    /// Replace the application-owned viewport height after resize.
    Resized(Size),
    /// Switch between the fixed and variable providers.
    ToggleMode,
    /// Reverse item order while preserving active and selected keys.
    Reverse,
    /// Request orderly shutdown.
    Quit,
}

/// Facade-only application proving bounded visible-range construction.
pub struct CollectionLab {
    items: Vec<Item>,
    mode: CollectionMode,
    fixed: FixedHeightProvider,
    variable: VariableHeightProvider,
    viewport_height: usize,
    scroll: usize,
    active: Option<u64>,
    selected: Option<u64>,
    constructed_rows: Cell<usize>,
}

impl CollectionLab {
    /// Creates a generated collection with an explicit application-owned viewport.
    #[must_use]
    pub fn new(mode: CollectionMode, item_count: usize, viewport_height: usize) -> Self {
        let items: Vec<_> = (0..item_count)
            .map(|index| {
                let key = u64::try_from(index).unwrap_or(u64::MAX);
                let height = NonZeroUsize::new(index % 3 + 1).unwrap_or(NonZeroUsize::MIN);
                let fixed_label = format!("Item {key:06}");
                let variable_label = (0..height.get())
                    .map(|line| format!("Item {key:06} / line {}", line + 1))
                    .collect::<Vec<_>>()
                    .join("\n");
                Item {
                    key,
                    fixed_label,
                    variable_label,
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
            viewport_height: viewport_height.max(1),
            scroll: 0,
            selected: None,
            constructed_rows: Cell::new(0),
        }
    }

    /// Returns the active stable item key.
    #[must_use]
    pub const fn active_key(&self) -> Option<u64> {
        self.active
    }

    /// Returns the selected stable item key.
    #[must_use]
    pub const fn selected_key(&self) -> Option<u64> {
        self.selected
    }

    /// Returns the controlled logical scroll offset.
    #[must_use]
    pub const fn scroll_offset(&self) -> usize {
        self.scroll
    }

    /// Returns the current application-owned viewport height.
    #[must_use]
    pub const fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    /// Returns the number of row elements built by the most recent view.
    #[must_use]
    pub fn constructed_rows(&self) -> usize {
        self.constructed_rows.get()
    }

    /// Returns the range represented by the current model state.
    #[must_use]
    pub fn visible_range(&self) -> VisibleRange {
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

    fn active_index(&self) -> Option<usize> {
        let active = self.active?;
        self.items.iter().position(|item| item.key == active)
    }

    fn move_active(&mut self, message: Message) {
        if self.items.is_empty() {
            return;
        }
        let current = self.active_index().unwrap_or_default();
        let last = self.items.len().saturating_sub(1);
        let target = match message {
            Message::Up => current.saturating_sub(1),
            Message::Down => current.saturating_add(1).min(last),
            Message::Home => 0,
            Message::End => last,
            Message::PageUp => {
                self.index_for_offset(self.scroll.saturating_sub(self.viewport_height))
            }
            Message::PageDown => self.index_for_offset(
                self.scroll
                    .saturating_add(self.viewport_height)
                    .min(self.max_scroll()),
            ),
            _ => current,
        };
        self.active = Some(self.items[target].key);
        self.reveal(target);
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

    fn resize(&mut self, size: Size) {
        self.viewport_height = usize::from(size.height.saturating_sub(4).max(1));
        self.scroll = self.scroll.min(self.max_scroll());
        if let Some(index) = self.active_index() {
            self.reveal(index);
        }
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

impl Default for CollectionLab {
    fn default() -> Self {
        Self::new(CollectionMode::Fixed, 100_000, 16)
    }
}

impl Application for CollectionLab {
    type Message = Message;

    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        match message {
            Message::Up
            | Message::Down
            | Message::Home
            | Message::End
            | Message::PageUp
            | Message::PageDown => self.move_active(message),
            Message::SelectActive => self.selected = self.active,
            Message::Select(key) => {
                if let Some(index) = self.items.iter().position(|item| item.key == key) {
                    self.active = Some(key);
                    self.selected = Some(key);
                    self.reveal(index);
                }
            }
            Message::Scrolled(delta) => {
                self.scroll = if delta.y.is_negative() {
                    self.scroll.saturating_sub(delta.y.unsigned_abs() as usize)
                } else {
                    self.scroll.saturating_add(delta.y as usize)
                }
                .min(self.max_scroll());
            }
            Message::Resized(size) => self.resize(size),
            Message::ToggleMode => {
                self.mode = match self.mode {
                    CollectionMode::Fixed => CollectionMode::Variable,
                    CollectionMode::Variable => CollectionMode::Fixed,
                };
                self.scroll = 0;
                if let Some(index) = self.active_index() {
                    self.reveal(index);
                }
            }
            Message::Reverse => {
                self.items.reverse();
                self.rebuild_providers();
            }
            Message::Quit => return Command::quit(),
        }
        context.invalidate(Invalidation::Recompose);
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let range = self.visible_range();
        self.constructed_rows.set(range.len());
        let full_width = Dimension::percent(100);
        let rows = self.items[range.start..range.end]
            .iter()
            .enumerate()
            .map(|(window_index, item)| {
                let index = range.start.saturating_add(window_index);
                let height = match self.mode {
                    CollectionMode::Fixed => 1,
                    CollectionMode::Variable => item.height.get(),
                };
                let label = match self.mode {
                    CollectionMode::Fixed => &item.fixed_label,
                    CollectionMode::Variable => &item.variable_label,
                };
                let mut style = Style::new();
                if self.selected == Some(item.key) {
                    style = style.background(Color::Blue).foreground(Color::BrightWhite);
                }
                if self.active == Some(item.key) {
                    style = style
                        .foreground(Color::BrightYellow)
                        .add_modifiers(Modifier::BOLD);
                }
                let key = item.key;
                let row = text(label)
                    .style(style)
                    .layout(LayoutStyle {
                        width: full_width,
                        height: Dimension::cells(u16::try_from(height).unwrap_or(u16::MAX)),
                        flex_shrink: 0,
                        ..LayoutStyle::default()
                    })
                    .interactive(true)
                    .on_event(EventPhase::Target, move |event, context| {
                        if matches!(
                            event,
                            UiEvent::Pointer(pointer)
                                if pointer.kind == PointerEventKind::Down(PointerButton::Primary)
                        ) {
                            context.emit(Message::Select(key));
                            context.mark_handled();
                        }
                    });
                (item.key, row, index)
            })
            .map(|(key, row, _index)| (key, row));
        let content = list(rows).layout(LayoutStyle {
            width: full_width,
            height: Dimension::cells(u16::try_from(range.content_height()).unwrap_or(u16::MAX)),
            direction: FlexDirection::Column,
            flex_shrink: 0,
            ..LayoutStyle::default()
        });
        let local_offset = i32::try_from(range.local_offset()).unwrap_or(i32::MAX);
        let collection = scroll_view(Point::new(0, local_offset), content)
            .on_scroll(Message::Scrolled)
            .layout(LayoutStyle::new().size(
                full_width,
                Dimension::cells(u16::try_from(self.viewport_height).unwrap_or(u16::MAX)),
            ))
            .build()
            .key("collection")
            .focusable(true)
            .focus_style(Style::new().add_modifiers(Modifier::REVERSED))
            .on_event(EventPhase::Target, |event, context| {
                let message = match event {
                    UiEvent::Key(key)
                        if matches!(key.action, KeyAction::Press | KeyAction::Repeat) =>
                    {
                        match key.key {
                            UiKey::Up => Some(Message::Up),
                            UiKey::Down => Some(Message::Down),
                            UiKey::Home => Some(Message::Home),
                            UiKey::End => Some(Message::End),
                            UiKey::PageUp => Some(Message::PageUp),
                            UiKey::PageDown => Some(Message::PageDown),
                            UiKey::Enter | UiKey::Character(' ') => Some(Message::SelectActive),
                            UiKey::Character('v') => Some(Message::ToggleMode),
                            UiKey::Character('r') => Some(Message::Reverse),
                            UiKey::Character('q') | UiKey::Escape => Some(Message::Quit),
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
        let panel = Block::new(collection)
            .title(match self.mode {
                CollectionMode::Fixed => "Fixed-height visible range",
                CollectionMode::Variable => "Variable-height visible range",
            })
            .border_style(Style::new().foreground(Color::BrightCyan))
            .layout(LayoutStyle::new().size(
                full_width,
                Dimension::cells(
                    u16::try_from(self.viewport_height.saturating_add(2)).unwrap_or(u16::MAX),
                ),
            ))
            .build();

        arborui::prelude::column([
            text("Arrows/Page/Home/End move | Enter selects | v mode | r reverse | q quit"),
            panel,
        ])
        .layout(LayoutStyle {
            width: full_width,
            height: Dimension::percent(100),
            direction: FlexDirection::Column,
            ..LayoutStyle::default()
        })
        .on_event(EventPhase::Capture, |event, context| {
            if let UiEvent::Resize(size) = event {
                context.emit(Message::Resized(*size));
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_range_is_bounded_by_visible_rows_and_overscan() {
        let provider = FixedHeightProvider::new(
            1_000_000,
            NonZeroUsize::new(2).unwrap_or(NonZeroUsize::MIN),
            3,
        );

        let range = provider.visible_range(500_001, 20);

        assert_eq!(
            range,
            VisibleRange {
                start: 249_997,
                end: 250_014,
                local_offset: 7,
                content_height: 34,
            }
        );
        assert_eq!(provider.max_scroll(20), 1_999_980);
    }

    #[test]
    fn variable_measurement_update_preserves_the_anchor() {
        let items = [1, 2, 3, 1]
            .into_iter()
            .enumerate()
            .filter_map(|(key, height)| {
                Some((u64::try_from(key).ok()?, NonZeroUsize::new(height)?))
            });
        let mut provider = VariableHeightProvider::new(items, 2);
        let anchor = provider
            .anchor(4)
            .expect("non-empty provider has an anchor");
        assert_eq!(
            anchor,
            HeightAnchor {
                item_key: 2,
                intra_item_offset: 1,
            }
        );

        let scroll = provider
            .update_height(
                0,
                NonZeroUsize::new(3).unwrap_or(NonZeroUsize::MIN),
                anchor,
                2,
            )
            .expect("anchor remains present");

        assert_eq!(scroll, 6);
        assert_eq!(provider.anchor(scroll), Some(anchor));
    }
}
