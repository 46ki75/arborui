use arborui_core::{Point, Style};
use arborui_layout::LayoutStyle;
use arborui_ui::{Element, EventPhase, PointerEventKind, UiEvent};

/// Creates a controlled scroll viewport builder.
#[must_use]
pub fn scroll_view<'a, Message>(
    offset: Point,
    child: Element<'a, Message>,
) -> ScrollView<'a, Message>
where
    Message: 'a,
{
    ScrollView::new(offset, child)
}

/// Builder for a clipped controlled scroll viewport.
///
/// Positive application offsets move content left and up. Wheel handlers emit
/// signed deltas and never mutate the supplied offset.
pub struct ScrollView<'a, Message> {
    offset: Point,
    child: Element<'a, Message>,
    on_scroll: Option<Box<dyn Fn(Point) -> Message + 'a>>,
    style: Style,
    layout: LayoutStyle,
}

impl<'a, Message: 'a> ScrollView<'a, Message> {
    /// Creates a viewport borrowing no scroll state beyond this frame.
    #[must_use]
    pub fn new(offset: Point, child: Element<'a, Message>) -> Self {
        Self {
            offset,
            child,
            on_scroll: None,
            style: Style::default(),
            layout: LayoutStyle::default(),
        }
    }

    /// Maps vertical and horizontal wheel deltas into application messages.
    #[must_use]
    pub fn on_scroll(mut self, mapper: impl Fn(Point) -> Message + 'a) -> Self {
        self.on_scroll = Some(Box::new(mapper));
        self
    }

    /// Sets the viewport style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the viewport layout properties.
    #[must_use]
    pub const fn layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }

    /// Builds the declarative scroll viewport.
    #[must_use]
    pub fn build(self) -> Element<'a, Message> {
        let translated = Point::new(
            self.offset.x.saturating_neg(),
            self.offset.y.saturating_neg(),
        );
        let mut element = Element::custom("scroll-view", [self.child])
            .layout(self.layout)
            .style(self.style)
            .child_offset(translated);
        if let Some(mapper) = self.on_scroll {
            element = element.on_event(EventPhase::Bubble, move |event, context| {
                let delta = match event {
                    UiEvent::Pointer(pointer) => match pointer.kind {
                        PointerEventKind::Scroll(delta) => Some(Point::new(0, i32::from(delta))),
                        PointerEventKind::ScrollHorizontal(delta) => {
                            Some(Point::new(i32::from(delta), 0))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(delta) = delta {
                    context.emit(mapper(delta));
                    context.mark_handled();
                    context.stop_propagation();
                }
            });
        }
        element
    }
}
