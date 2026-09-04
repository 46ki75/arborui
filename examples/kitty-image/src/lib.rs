//! Interactive native-image exercise using only the `arborui` facade.

use arborui::{
    EventPhase, KeyAction, Modifier, PointerButton, PointerEventKind, TerminalViewport, UiEvent,
    UiKey, prelude::*,
};

const PIXEL_WIDTH: u32 = 400;
const PIXEL_HEIGHT: u32 = 200;
const CELL_HEIGHT_TO_WIDTH: u32 = 2;
const ROOT_RESERVED_HEIGHT: u16 = 6;
const SIDEBAR_MIN_WIDTH: u16 = 20;
const SIDEBAR_MAX_WIDTH: u16 = 36;
const MIN_PREVIEW_WIDTH: u16 = 12;
const MOVE_OFFSET: u16 = 8;

/// Messages accepted by [`KittyImageDemo`].
pub enum Message {
    /// Select the previous immutable image source.
    PreviousImage,
    /// Select the next immutable image source.
    NextImage,
    /// Select an immutable image source by its list index.
    SelectImage(usize),
    /// Add or remove the transparent overlay from the displayed source.
    ToggleOverlay,
    /// Move the image placement horizontally.
    TogglePosition,
    /// Add or remove all image placements.
    ToggleVisible,
    /// Refit the viewer to a changed terminal viewport.
    Resize(Size),
    /// Request orderly application shutdown.
    Quit,
}

/// Manual exercise for native image replacement, movement, and cleanup.
pub struct KittyImageDemo {
    sources: Vec<ImageSource>,
    source_index: usize,
    show_overlay: bool,
    shifted: bool,
    visible: bool,
    graphics_status: String,
    viewport: Size,
    cell_aspect: CellAspect,
}

#[derive(Clone, Copy)]
struct CellAspect {
    width: u128,
    height: u128,
}

impl CellAspect {
    const FALLBACK: Self = Self {
        width: 1,
        height: CELL_HEIGHT_TO_WIDTH as u128,
    };

    fn from_viewport(viewport: TerminalViewport) -> Self {
        let Some(pixels) = viewport.pixels else {
            return Self::FALLBACK;
        };
        if viewport.cells.is_empty() || pixels.width == 0 || pixels.height == 0 {
            return Self::FALLBACK;
        }
        Self {
            width: u128::from(pixels.width) * u128::from(viewport.cells.height),
            height: u128::from(pixels.height) * u128::from(viewport.cells.width),
        }
    }
}

struct ImageSource {
    label: String,
    base: RgbaImage,
    composited: RgbaImage,
}

impl KittyImageDemo {
    /// Creates the demo and its generated RGBA sources.
    pub fn new(
        graphics_status: impl Into<String>,
        viewport: impl Into<TerminalViewport>,
    ) -> Result<Self, arborui::ImageError> {
        let viewport = viewport.into();
        let sources = prepare_sources(vec![
            ("generated aurora".to_owned(), base_image(false)?),
            ("generated sunset".to_owned(), base_image(true)?),
        ])?;
        Ok(Self {
            sources,
            source_index: 0,
            show_overlay: true,
            shifted: false,
            visible: true,
            graphics_status: graphics_status.into(),
            viewport: viewport.cells,
            cell_aspect: CellAspect::from_viewport(viewport),
        })
    }

    /// Creates the demo with a non-empty set of named decoded images.
    pub fn with_images(
        graphics_status: impl Into<String>,
        viewport: impl Into<TerminalViewport>,
        first: (String, RgbaImage),
        additional: impl IntoIterator<Item = (String, RgbaImage)>,
    ) -> Result<Self, arborui::ImageError> {
        let viewport = viewport.into();
        let mut sources = vec![first];
        sources.extend(additional);
        let sources = prepare_sources(sources)?;
        Ok(Self {
            sources,
            source_index: 0,
            show_overlay: true,
            shifted: false,
            visible: true,
            graphics_status: graphics_status.into(),
            viewport: viewport.cells,
            cell_aspect: CellAspect::from_viewport(viewport),
        })
    }

    fn current_source(&self) -> &ImageSource {
        &self.sources[self.source_index]
    }

    fn image_stage(&self) -> Element<'_, Message> {
        let available = self.preview_content_size();
        let image_size = self.image_cell_size();
        let stage = if self.visible {
            let current = self.current_source();
            let source = if self.show_overlay {
                &current.composited
            } else {
                &current.base
            };
            image(source, image_size.width, image_size.height)
                .fallback("TEXT FALLBACK - native image unavailable")
                .style(
                    Style::new()
                        .foreground(Color::BrightYellow)
                        .background(Color::Blue),
                )
                .layout(image_layout(image_size))
                .build()
                .key("image")
        } else {
            text("IMAGE HIDDEN - prior placement should be gone")
                .style(Style::new().foreground(Color::BrightYellow))
                .layout(image_layout(image_size))
                .key("hidden-image")
        };
        let horizontal_space = available.width.saturating_sub(image_size.width);
        let centered_left = horizontal_space / 2;
        let left = if self.shifted {
            centered_left
                .saturating_add(MOVE_OFFSET)
                .min(horizontal_space)
        } else {
            centered_left
        };
        let right = horizontal_space.saturating_sub(left);
        let top = available.height.saturating_sub(image_size.height) / 2;
        let bottom = available
            .height
            .saturating_sub(image_size.height)
            .saturating_sub(top);
        let image_row = row([
            spacer(left, image_size.height),
            stage,
            spacer(right, image_size.height),
        ])
        .layout(image_layout(Size::new(available.width, image_size.height)));

        column([
            spacer(available.width, top),
            image_row,
            spacer(available.width, bottom),
        ])
        .layout(image_layout(available))
    }

    fn image_cell_size(&self) -> Size {
        let available = self.preview_content_size();
        let source = &self.current_source().base;
        let width_at_full_height =
            u128::from(source.width()) * u128::from(available.height) * self.cell_aspect.height
                / (u128::from(source.height()) * self.cell_aspect.width);
        if width_at_full_height <= u128::from(available.width) {
            return Size::new(
                u16::try_from(width_at_full_height)
                    .unwrap_or(available.width)
                    .max(1),
                available.height,
            );
        }

        let height_at_full_width =
            u128::from(available.width) * u128::from(source.height()) * self.cell_aspect.width
                / (u128::from(source.width()) * self.cell_aspect.height);
        Size::new(
            available.width,
            u16::try_from(height_at_full_width)
                .unwrap_or(available.height)
                .max(1),
        )
    }

    fn workspace_height(&self) -> u16 {
        self.viewport
            .height
            .saturating_sub(ROOT_RESERVED_HEIGHT)
            .max(1)
    }

    fn sidebar_width(&self) -> u16 {
        let preferred = (self.viewport.width / 3).clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        preferred
            .min(self.viewport.width.saturating_sub(MIN_PREVIEW_WIDTH + 1))
            .max(1)
    }

    fn preview_content_size(&self) -> Size {
        Size::new(
            self.viewport
                .width
                .saturating_sub(self.sidebar_width())
                .saturating_sub(3)
                .max(1),
            self.workspace_height().saturating_sub(2).max(1),
        )
    }

    fn selection_window(&self) -> (usize, usize) {
        let visible =
            usize::from(self.workspace_height().saturating_sub(2).max(1)).min(self.sources.len());
        let maximum_start = self.sources.len().saturating_sub(visible);
        let start = self
            .source_index
            .saturating_sub(visible / 2)
            .min(maximum_start);
        (start, start + visible)
    }

    fn selection_panel(&self) -> Element<'_, Message> {
        let full_width = Dimension::percent(100);
        let (start, end) = self.selection_window();
        let rows = self.sources[start..end]
            .iter()
            .enumerate()
            .map(|(local_index, source)| {
                let index = start + local_index;
                let selected = index == self.source_index;
                let style = if selected {
                    Style::new()
                        .foreground(Color::BrightWhite)
                        .background(Color::Blue)
                        .add_modifiers(Modifier::BOLD)
                } else {
                    Style::new().foreground(Color::White)
                };
                let marker = if selected { ">" } else { " " };
                let row = row([
                    text(marker).style(style).layout(
                        LayoutStyle::new()
                            .size(Dimension::cells(2), Dimension::cells(1))
                            .flex(0, 0),
                    ),
                    text(&source.label)
                        .style(style)
                        .layout(LayoutStyle::new().flex(1, 1)),
                ])
                .layout(LayoutStyle {
                    width: full_width,
                    height: Dimension::cells(1),
                    direction: FlexDirection::Row,
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
                        context.emit(Message::SelectImage(index));
                        context.mark_handled();
                    }
                });
                (index, row)
            });
        let content = list(rows).layout(LayoutStyle {
            width: full_width,
            height: Dimension::cells(u16::try_from(end - start).unwrap_or(u16::MAX)),
            direction: FlexDirection::Column,
            flex_shrink: 0,
            ..LayoutStyle::default()
        });

        Block::new(content)
            .title("Images")
            .border_style(Style::new().foreground(Color::BrightBlack))
            .layout(
                LayoutStyle::new()
                    .size(
                        Dimension::cells(self.sidebar_width()),
                        Dimension::percent(100),
                    )
                    .flex(0, 1),
            )
            .build()
    }
}

impl Application for KittyImageDemo {
    type Message = Message;

    fn update(
        &mut self,
        message: Self::Message,
        context: &mut UpdateContext<Self::Message>,
    ) -> Command<Self::Message> {
        match message {
            Message::PreviousImage => {
                if self.sources.len() > 1 {
                    self.source_index = if self.source_index == 0 {
                        self.sources.len() - 1
                    } else {
                        self.source_index - 1
                    };
                    context.invalidate(Invalidation::Layout);
                }
            }
            Message::NextImage => {
                if self.sources.len() > 1 {
                    self.source_index = (self.source_index + 1) % self.sources.len();
                    context.invalidate(Invalidation::Layout);
                }
            }
            Message::SelectImage(index) => {
                if index < self.sources.len() && index != self.source_index {
                    self.source_index = index;
                    context.invalidate(Invalidation::Layout);
                }
            }
            Message::ToggleOverlay => {
                self.show_overlay = !self.show_overlay;
                context.invalidate(Invalidation::Paint);
            }
            Message::TogglePosition => {
                self.shifted = !self.shifted;
                context.invalidate(Invalidation::Layout);
            }
            Message::ToggleVisible => {
                self.visible = !self.visible;
                context.invalidate(Invalidation::Recompose);
            }
            Message::Resize(size) => {
                if size != self.viewport {
                    self.viewport = size;
                    context.invalidate(Invalidation::Layout);
                }
            }
            Message::Quit => return Command::quit(),
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let full_width = Dimension::percent(100);
        let full_height = Dimension::percent(100);
        let source_count = self.sources.len();
        let panel = Block::new(self.image_stage())
            .title(&self.current_source().label)
            .border_style(Style::new().foreground(Color::BrightCyan))
            .layout(LayoutStyle::new().size(full_width, full_height).flex(1, 1))
            .build();
        let workspace = row_with_gap([self.selection_panel(), panel], 1).layout(LayoutStyle {
            width: full_width,
            height: full_height,
            direction: FlexDirection::Row,
            flex_grow: 1,
            flex_shrink: 1,
            gap: 1,
            ..LayoutStyle::default()
        });

        column_with_gap(
            [
                text("ArborUI image viewer").style(Style::new().foreground(Color::BrightCyan)),
                text(&self.graphics_status),
                workspace,
                row_with_gap(
                    [
                        button("Prev [p]", || Message::PreviousImage).build(),
                        button("Next [n]", || Message::NextImage).build(),
                        button("Overlay [o]", || Message::ToggleOverlay).build(),
                        button("Move [m]", || Message::TogglePosition).build(),
                        button("Show/hide [h]", || Message::ToggleVisible).build(),
                        button("Quit [q]", || Message::Quit).build(),
                    ],
                    1,
                ),
            ],
            1,
        )
        .layout(LayoutStyle {
            width: full_width,
            height: full_height,
            direction: FlexDirection::Column,
            gap: 1,
            ..LayoutStyle::default()
        })
        .on_event(EventPhase::Capture, move |event, context| {
            let message = match event {
                UiEvent::Key(key) if matches!(key.action, KeyAction::Press | KeyAction::Repeat) => {
                    match key.key {
                        UiKey::Up | UiKey::Left | UiKey::Character('p' | 'P') => {
                            Some(Message::PreviousImage)
                        }
                        UiKey::Down | UiKey::Right | UiKey::Character('n' | 'N') => {
                            Some(Message::NextImage)
                        }
                        UiKey::Home => Some(Message::SelectImage(0)),
                        UiKey::End => Some(Message::SelectImage(source_count - 1)),
                        UiKey::Character('o' | 'O') => Some(Message::ToggleOverlay),
                        UiKey::Character('m' | 'M') => Some(Message::TogglePosition),
                        UiKey::Character('h' | 'H') => Some(Message::ToggleVisible),
                        UiKey::Character('q' | 'Q') | UiKey::Escape => Some(Message::Quit),
                        _ => None,
                    }
                }
                UiEvent::Resize(size) => Some(Message::Resize(*size)),
                _ => None,
            };
            if let Some(message) = message {
                context.emit(message);
                context.mark_handled();
                context.prevent_default();
            }
        })
    }
}

fn image_layout(size: Size) -> LayoutStyle {
    LayoutStyle::new()
        .size(Dimension::cells(size.width), Dimension::cells(size.height))
        .flex(0, 0)
}

fn prepare_sources(
    sources: Vec<(String, RgbaImage)>,
) -> Result<Vec<ImageSource>, arborui::ImageError> {
    let count = sources.len();
    sources
        .into_iter()
        .enumerate()
        .map(|(index, (name, base))| {
            let label = format!(
                "image {}/{}: {} ({}x{})",
                index + 1,
                count,
                name,
                base.width(),
                base.height()
            );
            let composited = composite_overlay(&base)?;
            Ok(ImageSource {
                label,
                base,
                composited,
            })
        })
        .collect()
}

fn base_image(warm: bool) -> Result<RgbaImage, arborui::ImageError> {
    let mut pixels = Vec::with_capacity((PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize);
    for y in 0..PIXEL_HEIGHT {
        for x in 0..PIXEL_WIDTH {
            let horizontal = (x * 255 / (PIXEL_WIDTH - 1)) as u8;
            let vertical = (y * 255 / (PIXEL_HEIGHT - 1)) as u8;
            let checker = ((x / 40) + (y / 40)) % 2 == 0;
            let (mut red, mut green, mut blue) = if warm {
                (
                    80_u8.saturating_add(horizontal.saturating_mul(2) / 3),
                    20_u8.saturating_add(vertical / 2),
                    35_u8.saturating_add((u8::MAX - horizontal) / 4),
                )
            } else {
                (
                    15_u8.saturating_add(vertical / 3),
                    55_u8.saturating_add(horizontal / 2),
                    90_u8.saturating_add((u8::MAX - vertical) / 2),
                )
            };
            if checker {
                red = red.saturating_add(22);
                green = green.saturating_add(22);
                blue = blue.saturating_add(22);
            }
            if x < 4 || y < 4 || x >= PIXEL_WIDTH - 4 || y >= PIXEL_HEIGHT - 4 {
                (red, green, blue) = (245, 245, 245);
            }
            pixels.extend_from_slice(&[red, green, blue, u8::MAX]);
        }
    }
    RgbaImage::new(PIXEL_WIDTH, PIXEL_HEIGHT, pixels)
}

fn composite_overlay(source: &RgbaImage) -> Result<RgbaImage, arborui::ImageError> {
    let width = source.width();
    let height = source.height();
    let line_radius = width.min(height).div_ceil(64).max(1);
    let frame_left = (width - 1) / 4;
    let frame_right = (width - 1) * 3 / 4;
    let frame_top = (height - 1) / 4;
    let frame_bottom = (height - 1) * 3 / 4;
    let mut pixels = source.pixels().to_vec();

    for y in 0..height {
        for x in 0..width {
            let rising = y.abs_diff(x * height / width) <= line_radius;
            let falling = y.abs_diff((width - 1 - x) * height / width) <= line_radius;
            let frame = ((x == frame_left || x == frame_right)
                && (frame_top..=frame_bottom).contains(&y))
                || ((y == frame_top || y == frame_bottom)
                    && (frame_left..=frame_right).contains(&x));
            if rising || falling || frame {
                let offset = ((y * width + x) * 4) as usize;
                alpha_blend(&mut pixels[offset..offset + 4], [255, 230, 40, 210]);
            }
        }
    }
    RgbaImage::new(width, height, pixels)
}

fn alpha_blend(base: &mut [u8], overlay: [u8; 4]) {
    let overlay_alpha = u32::from(overlay[3]);
    let base_alpha = u32::from(base[3]);
    let inverse_alpha = u32::from(u8::MAX) - overlay_alpha;
    let output_alpha = overlay_alpha + (base_alpha * inverse_alpha + 127) / 255;

    for channel in 0..3 {
        let foreground = u32::from(overlay[channel]) * overlay_alpha;
        let background = (u32::from(base[channel]) * base_alpha * inverse_alpha + 127) / 255;
        base[channel] = ((foreground + background + output_alpha / 2) / output_alpha) as u8;
    }
    base[3] = output_alpha as u8;
}

#[cfg(test)]
mod tests {
    use arborui_test::{KeyCode, Size, TestApp};

    use super::*;

    #[test]
    fn controls_replace_move_and_clear_the_image_scene() -> Result<(), arborui::ImageError> {
        let viewport = Size::new(80, 30);
        let mut app = TestApp::new(KittyImageDemo::new("headless test", viewport)?, viewport);
        assert_eq!(app.frame().images().placements().len(), 1);
        assert!(app.frame().characters().contains("TEXT FALLBACK"));
        assert!(app.frame().characters().contains("generated aurora"));
        assert!(app.frame().characters().contains("image 2/2"));
        let initial_id = app.frame().images().placements()[0].image().id();
        assert_eq!(app.frame().images().placements()[0].destination().width, 51);

        app.send(Message::NextImage);
        assert_ne!(
            app.frame().images().placements()[0].image().id(),
            initial_id
        );

        let composited_id = app.frame().images().placements()[0].image().id();
        app.send(Message::ToggleOverlay);
        assert_eq!(app.frame().images().placements().len(), 1);
        assert_ne!(
            app.frame().images().placements()[0].image().id(),
            composited_id
        );

        app.resize(Size::new(120, 40));
        assert_eq!(app.frame().images().placements()[0].destination().width, 81);

        app.resize(Size::new(50, 20));
        assert_eq!(app.frame().images().placements().len(), 1);
        assert_eq!(app.frame().images().placements()[0].destination().width, 27);

        app.send(Message::ToggleVisible);
        assert!(app.frame().images().is_empty());
        Ok(())
    }

    #[test]
    fn cycles_loaded_images() -> Result<(), arborui::ImageError> {
        let first = RgbaImage::new(3, 2, vec![255; 3 * 2 * 4])?;
        let second = RgbaImage::new(1, 2, vec![0; 8])?;
        let application = KittyImageDemo::with_images(
            "headless test",
            Size::new(80, 30),
            ("first.png".to_owned(), first),
            [("second.webp".to_owned(), second)],
        )?;
        let first_id = application.sources[0].composited.id();
        let second_id = application.sources[1].composited.id();
        let mut app = TestApp::new(application, Size::new(80, 30));

        assert_eq!(app.frame().images().placements()[0].image().id(), first_id);
        assert!(
            app.frame()
                .characters()
                .contains("image 1/2: first.png (3x2)")
        );

        app.click(Point::new(2, 6));
        assert_eq!(app.frame().images().placements()[0].image().id(), second_id);
        assert!(
            app.frame()
                .characters()
                .contains("image 2/2: second.webp (1x2)")
        );

        app.send(Message::PreviousImage);
        assert_eq!(app.frame().images().placements()[0].image().id(), first_id);
        Ok(())
    }

    #[test]
    fn selecting_the_active_image_from_input_does_not_commit_another_frame()
    -> Result<(), arborui::ImageError> {
        let viewport = Size::new(80, 30);
        let mut app = TestApp::new(KittyImageDemo::new("headless test", viewport)?, viewport);

        let report = app.key(KeyCode::Home);

        assert_eq!(report.updates, 1);
        assert_eq!(report.committed_frames, 0);
        Ok(())
    }

    #[test]
    fn selection_window_follows_keyboard_navigation() -> Result<(), arborui::ImageError> {
        let first = (
            "photo-01.png".to_owned(),
            RgbaImage::new(1, 1, vec![255; 4])?,
        );
        let mut additional = Vec::new();
        for index in 2..=30 {
            additional.push((
                format!("photo-{index:02}.png"),
                RgbaImage::new(1, 1, vec![255; 4])?,
            ));
        }
        let viewport = Size::new(80, 12);
        let mut app = TestApp::new(
            KittyImageDemo::with_images("headless test", viewport, first, additional)?,
            viewport,
        );

        assert!(app.frame().characters().contains("image 1/30"));
        app.key(KeyCode::End);
        assert!(app.frame().characters().contains("image 30/30"));
        assert!(!app.frame().characters().contains("image 1/30"));

        app.key(KeyCode::Up);
        assert!(app.frame().characters().contains("image 29/30"));
        Ok(())
    }

    #[test]
    fn portrait_image_uses_aspect_aware_cell_width() -> Result<(), arborui::ImageError> {
        let source = RgbaImage::new(150, 200, vec![255; 150 * 200 * 4])?;
        let mut app = TestApp::new(
            KittyImageDemo::with_images(
                "headless test",
                Size::new(80, 30),
                ("portrait.png".to_owned(), source),
                [],
            )?,
            Size::new(80, 30),
        );

        assert_eq!(app.frame().images().placements()[0].destination().width, 33);
        assert_eq!(
            app.frame().images().placements()[0].destination().height,
            22
        );
        assert_eq!(app.frame().images().placements().len(), 1);

        let centered_x = app.frame().images().placements()[0].destination().x;
        app.send(Message::TogglePosition);
        assert_eq!(
            app.frame().images().placements()[0].destination().x,
            centered_x + i32::from(MOVE_OFFSET)
        );
        Ok(())
    }

    #[test]
    fn portrait_image_uses_measured_cell_aspect_ratio() -> Result<(), arborui::ImageError> {
        let cells = Size::new(80, 30);
        let viewport =
            TerminalViewport::with_pixels(cells, arborui::TerminalPixelSize::new(800, 750));
        let source = RgbaImage::new(150, 200, vec![255; 150 * 200 * 4])?;
        let app = TestApp::new(
            KittyImageDemo::with_images(
                "headless test",
                viewport,
                ("portrait.png".to_owned(), source),
                [],
            )?,
            cells,
        );

        assert_eq!(app.frame().images().placements()[0].destination().width, 41);
        assert_eq!(
            app.frame().images().placements()[0].destination().height,
            22
        );
        Ok(())
    }
}
