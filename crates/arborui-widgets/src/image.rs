use std::hash::{DefaultHasher, Hash, Hasher};

use arborui_core::{Rect, Style};
use arborui_layout::{Dimension, LayoutStyle};
use arborui_render::RgbaImage;
use arborui_ui::Element;

/// Creates a native-image builder with an explicit terminal-cell size.
#[must_use]
pub fn image(source: &RgbaImage, width: u16, height: u16) -> Image<'_> {
    Image::new(source, width, height)
}

/// Builder for a Kitty-capable image with ordinary cell fallback content.
pub struct Image<'a> {
    source: &'a RgbaImage,
    fallback: &'a str,
    style: Style,
    layout: LayoutStyle,
}

impl<'a> Image<'a> {
    /// Creates an image with a `[image]` fallback and exact cell dimensions.
    #[must_use]
    pub fn new(source: &'a RgbaImage, width: u16, height: u16) -> Self {
        Self {
            source,
            fallback: "[image]",
            style: Style::default(),
            layout: LayoutStyle::new().size(Dimension::cells(width), Dimension::cells(height)),
        }
    }

    /// Sets the text painted when native images are unavailable.
    ///
    /// The text is clipped to the image element and is also retained beneath
    /// transparent native-image pixels.
    #[must_use]
    pub const fn fallback(mut self, fallback: &'a str) -> Self {
        self.fallback = fallback;
        self
    }

    /// Sets the fallback cell and background style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Overrides the image element's layout properties.
    #[must_use]
    pub const fn layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }

    /// Builds the declarative image element.
    #[must_use]
    pub fn build<Message>(self) -> Element<'a, Message> {
        let mut hasher = DefaultHasher::new();
        self.source.id().hash(&mut hasher);
        let fingerprint = hasher.finish();
        let source = self.source;
        let style = self.style;

        Element::custom("image", [Element::text(self.fallback).style(style)])
            .layout(self.layout)
            .style(style)
            .paint(fingerprint, move |size, canvas| {
                if size.is_empty() {
                    return Ok(());
                }
                canvas.draw_image(Rect::new(0, 0, size.width, size.height), source)?;
                Ok(())
            })
    }
}
