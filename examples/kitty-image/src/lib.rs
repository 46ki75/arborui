//! Interactive native-image exercise using only the `arborui` facade.

use arborui::{EventPhase, KeyAction, UiEvent, UiKey, prelude::*};

const PIXEL_WIDTH: u32 = 400;
const PIXEL_HEIGHT: u32 = 200;
const IMAGE_WIDTH: u16 = 40;
const IMAGE_HEIGHT: u16 = 10;
const STAGE_WIDTH: u16 = 48;

/// Messages accepted by [`KittyImageDemo`].
pub enum Message {
    /// Switch between the two immutable image sources.
    TogglePalette,
    /// Add or remove the transparent image layer.
    ToggleOverlay,
    /// Move the image placement horizontally.
    TogglePosition,
    /// Add or remove all image placements.
    ToggleVisible,
    /// Request orderly application shutdown.
    Quit,
}

/// Manual exercise for native image replacement, layering, movement, and cleanup.
pub struct KittyImageDemo {
    aurora: RgbaImage,
    sunset: RgbaImage,
    overlay: RgbaImage,
    warm_palette: bool,
    show_overlay: bool,
    shifted: bool,
    visible: bool,
    graphics_status: String,
}

impl KittyImageDemo {
    /// Creates the demo and its generated RGBA sources.
    pub fn new(graphics_status: impl Into<String>) -> Result<Self, arborui::ImageError> {
        Ok(Self {
            aurora: base_image(false)?,
            sunset: base_image(true)?,
            overlay: overlay_image()?,
            warm_palette: false,
            show_overlay: true,
            shifted: false,
            visible: true,
            graphics_status: graphics_status.into(),
        })
    }

    fn image_stage(&self) -> Element<'_, Message> {
        let stage = if self.visible {
            let source = if self.warm_palette {
                &self.sunset
            } else {
                &self.aurora
            };
            let base = image(source, IMAGE_WIDTH, IMAGE_HEIGHT)
                .fallback("TEXT FALLBACK - native image unavailable")
                .style(
                    Style::new()
                        .foreground(Color::BrightYellow)
                        .background(Color::Blue),
                )
                .layout(image_layout())
                .build()
                .key("base-image");
            let mut layers = vec![base];
            if self.show_overlay {
                layers.push(
                    image(&self.overlay, IMAGE_WIDTH, IMAGE_HEIGHT)
                        .fallback("TEXT FALLBACK - native image unavailable")
                        .style(
                            Style::new()
                                .foreground(Color::BrightYellow)
                                .background(Color::Blue),
                        )
                        .layout(image_layout())
                        .build()
                        .key("overlay-image"),
                );
            }
            stack(layers).key("image-stack")
        } else {
            text("IMAGE HIDDEN - prior placement should be gone")
                .style(Style::new().foreground(Color::BrightYellow))
                .layout(image_layout())
                .key("hidden-image")
        };
        let offset = if self.shifted { 8 } else { 0 };

        row([
            spacer(offset, IMAGE_HEIGHT),
            stage,
            spacer(STAGE_WIDTH - IMAGE_WIDTH - offset, IMAGE_HEIGHT),
        ])
        .layout(
            LayoutStyle::new()
                .size(
                    Dimension::cells(STAGE_WIDTH),
                    Dimension::cells(IMAGE_HEIGHT),
                )
                .flex(0, 0),
        )
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
            Message::TogglePalette => {
                self.warm_palette = !self.warm_palette;
                context.invalidate(Invalidation::Paint);
            }
            Message::ToggleOverlay => {
                self.show_overlay = !self.show_overlay;
                context.invalidate(Invalidation::Recompose);
            }
            Message::TogglePosition => {
                self.shifted = !self.shifted;
                context.invalidate(Invalidation::Layout);
            }
            Message::ToggleVisible => {
                self.visible = !self.visible;
                context.invalidate(Invalidation::Recompose);
            }
            Message::Quit => return Command::quit(),
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let palette = if self.warm_palette {
            "palette: sunset"
        } else {
            "palette: aurora"
        };
        let overlay = if self.show_overlay {
            "overlay: on"
        } else {
            "overlay: off"
        };
        let position = if self.shifted {
            "position: right"
        } else {
            "position: left"
        };
        let visibility = if self.visible {
            "image: shown"
        } else {
            "image: hidden"
        };
        let panel = Block::new(self.image_stage())
            .title("400x200 RGBA -> 40x10 cells")
            .padding(Insets::all(1))
            .border_style(Style::new().foreground(Color::BrightCyan))
            .layout(
                LayoutStyle::new()
                    .size(Dimension::cells(52), Dimension::cells(14))
                    .flex(0, 0),
            )
            .build();

        column_with_gap(
            [
                text("ArborUI Kitty graphics lab")
                    .style(Style::new().foreground(Color::BrightCyan)),
                text(&self.graphics_status),
                text(
                    "Expected: gradient with a translucent X; fallback text means no native image",
                ),
                row_with_gap(
                    [
                        text(palette),
                        text(overlay),
                        text(position),
                        text(visibility),
                    ],
                    2,
                ),
                panel,
                row_with_gap(
                    [
                        button("Palette [p]", || Message::TogglePalette).build(),
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
        .on_event(EventPhase::Capture, |event, context| {
            let message = match event {
                UiEvent::Key(key) if matches!(key.action, KeyAction::Press | KeyAction::Repeat) => {
                    match key.key {
                        UiKey::Character('p' | 'P') => Some(Message::TogglePalette),
                        UiKey::Character('o' | 'O') => Some(Message::ToggleOverlay),
                        UiKey::Character('m' | 'M') => Some(Message::TogglePosition),
                        UiKey::Character('h' | 'H') => Some(Message::ToggleVisible),
                        UiKey::Character('q' | 'Q') | UiKey::Escape => Some(Message::Quit),
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
        })
    }
}

fn image_layout() -> LayoutStyle {
    LayoutStyle::new()
        .size(
            Dimension::cells(IMAGE_WIDTH),
            Dimension::cells(IMAGE_HEIGHT),
        )
        .flex(0, 0)
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

fn overlay_image() -> Result<RgbaImage, arborui::ImageError> {
    let mut pixels = Vec::with_capacity((PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize);
    for y in 0..PIXEL_HEIGHT {
        for x in 0..PIXEL_WIDTH {
            let rising = y.abs_diff(x * PIXEL_HEIGHT / PIXEL_WIDTH) <= 3;
            let falling = y.abs_diff((PIXEL_WIDTH - 1 - x) * PIXEL_HEIGHT / PIXEL_WIDTH) <= 3;
            let frame = ((x == 100 || x == 299) && (50..=149).contains(&y))
                || ((y == 50 || y == 149) && (100..=299).contains(&x));
            if rising || falling || frame {
                pixels.extend_from_slice(&[255, 230, 40, 210]);
            } else {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    RgbaImage::new(PIXEL_WIDTH, PIXEL_HEIGHT, pixels)
}

#[cfg(test)]
mod tests {
    use arborui_test::{Size, TestApp};

    use super::*;

    #[test]
    fn controls_replace_move_and_clear_the_image_scene() -> Result<(), arborui::ImageError> {
        let mut app = TestApp::new(KittyImageDemo::new("headless test")?, Size::new(80, 30));
        assert_eq!(app.frame().images().placements().len(), 2);
        assert!(app.frame().characters().contains("TEXT FALLBACK"));
        let initial_id = app.frame().images().placements()[0].image().id();
        let initial_x = app.frame().images().placements()[0].destination().x;

        app.send(Message::TogglePalette);
        assert_ne!(
            app.frame().images().placements()[0].image().id(),
            initial_id
        );

        app.send(Message::TogglePosition);
        assert_eq!(
            app.frame().images().placements()[0].destination().x,
            initial_x + 8
        );

        app.send(Message::ToggleOverlay);
        assert_eq!(app.frame().images().placements().len(), 1);

        app.resize(Size::new(30, 30));
        assert!(app.frame().images().is_empty());
        assert!(app.frame().characters().contains("TEXT FALLBACK"));

        app.resize(Size::new(80, 30));
        assert_eq!(app.frame().images().placements().len(), 1);

        app.send(Message::ToggleVisible);
        assert!(app.frame().images().is_empty());
        Ok(())
    }
}
