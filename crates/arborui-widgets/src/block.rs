use std::hash::{DefaultHasher, Hash, Hasher};

use arborui_core::{Insets, Point, Style};
use arborui_layout::LayoutStyle;
use arborui_text::graphemes;
use arborui_ui::Element;

/// Glyph set used to paint a block border.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum BorderSet {
    /// Unicode box-drawing glyphs.
    #[default]
    Unicode,
    /// Portable ASCII border glyphs.
    Ascii,
}

#[derive(Clone, Copy)]
struct BorderGlyphs {
    horizontal: &'static str,
    vertical: &'static str,
    top_left: &'static str,
    top_right: &'static str,
    bottom_left: &'static str,
    bottom_right: &'static str,
}

impl BorderSet {
    const fn glyphs(self) -> BorderGlyphs {
        match self {
            Self::Unicode => BorderGlyphs {
                horizontal: "─",
                vertical: "│",
                top_left: "┌",
                top_right: "┐",
                bottom_left: "└",
                bottom_right: "┘",
            },
            Self::Ascii => BorderGlyphs {
                horizontal: "-",
                vertical: "|",
                top_left: "+",
                top_right: "+",
                bottom_left: "+",
                bottom_right: "+",
            },
        }
    }
}

/// Builder for a bordered content container.
pub struct Block<'a, Message> {
    child: Element<'a, Message>,
    title: Option<&'a str>,
    border: BorderSet,
    padding: Insets,
    style: Style,
    border_style: Style,
    layout: LayoutStyle,
}

impl<'a, Message> Block<'a, Message> {
    /// Creates a Unicode-bordered block around `child`.
    #[must_use]
    pub fn new(child: Element<'a, Message>) -> Self {
        Self {
            child,
            title: None,
            border: BorderSet::Unicode,
            padding: Insets::default(),
            style: Style::default(),
            border_style: Style::default(),
            layout: LayoutStyle::default(),
        }
    }

    /// Sets the optional title painted into the top border.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Selects the border glyph set.
    #[must_use]
    pub const fn border(mut self, border: BorderSet) -> Self {
        self.border = border;
        self
    }

    /// Sets spacing between the border and content.
    #[must_use]
    pub const fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// Sets the block's background and inherited cell style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style used for border and title glyphs.
    #[must_use]
    pub const fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Sets the block's layout properties.
    ///
    /// Building the block applies a one-cell border and the configured padding
    /// to this style.
    #[must_use]
    pub const fn layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }

    /// Builds the declarative block element.
    #[must_use]
    pub fn build(self) -> Element<'a, Message> {
        let Self {
            child,
            title,
            border,
            padding,
            style,
            border_style,
            mut layout,
        } = self;
        layout.border = Insets::all(1);
        layout.padding = padding;

        let mut hasher = DefaultHasher::new();
        title.hash(&mut hasher);
        border.hash(&mut hasher);
        border_style.hash(&mut hasher);
        let fingerprint = hasher.finish();
        let glyphs = border.glyphs();

        Element::custom("block", [child])
            .layout(layout)
            .style(style)
            .paint(fingerprint, move |size, canvas| {
                if size.width == 0 || size.height == 0 {
                    return Ok(());
                }

                let right = i32::from(size.width) - 1;
                let bottom = i32::from(size.height) - 1;
                let _ = canvas.draw_grapheme(Point::ORIGIN, glyphs.top_left, border_style, None)?;
                if size.width > 1 {
                    let _ = canvas.draw_grapheme(
                        Point::new(right, 0),
                        glyphs.top_right,
                        border_style,
                        None,
                    )?;
                }
                if size.height > 1 {
                    let _ = canvas.draw_grapheme(
                        Point::new(0, bottom),
                        glyphs.bottom_left,
                        border_style,
                        None,
                    )?;
                    if size.width > 1 {
                        let _ = canvas.draw_grapheme(
                            Point::new(right, bottom),
                            glyphs.bottom_right,
                            border_style,
                            None,
                        )?;
                    }
                }

                for x in 1..right {
                    let _ = canvas.draw_grapheme(
                        Point::new(x, 0),
                        glyphs.horizontal,
                        border_style,
                        None,
                    )?;
                    if size.height > 1 {
                        let _ = canvas.draw_grapheme(
                            Point::new(x, bottom),
                            glyphs.horizontal,
                            border_style,
                            None,
                        )?;
                    }
                }
                for y in 1..bottom {
                    let _ = canvas.draw_grapheme(
                        Point::new(0, y),
                        glyphs.vertical,
                        border_style,
                        None,
                    )?;
                    if size.width > 1 {
                        let _ = canvas.draw_grapheme(
                            Point::new(right, y),
                            glyphs.vertical,
                            border_style,
                            None,
                        )?;
                    }
                }

                if let Some(title) = title {
                    let available = usize::from(size.width.saturating_sub(2));
                    let decorated = format!(" {title} ");
                    let mut fitted = String::new();
                    let mut width = 0_usize;
                    for grapheme in graphemes(&decorated, canvas.width_policy()) {
                        if width.saturating_add(grapheme.width) > available {
                            break;
                        }
                        fitted.push_str(grapheme.text);
                        width = width.saturating_add(grapheme.width);
                    }
                    let _ = canvas.draw_text(Point::new(1, 0), &fitted, border_style, None)?;
                }
                Ok(())
            })
    }
}
