use arborui_core::Style;
use arborui_layout::{Align, Dimension, FlexDirection, Justify, LayoutStyle};
use arborui_ui::{Element, EventPhase, KeyAction, PointerEventKind, UiEvent, UiKey};

/// Creates a modal dialog builder around caller-supplied content.
#[must_use]
pub fn dialog<'a, Message>(
    child: Element<'a, Message>,
    on_dismiss: impl Fn() -> Message + 'a,
) -> Dialog<'a, Message>
where
    Message: 'a,
{
    Dialog::new(child, on_dismiss)
}

/// Builder for a modal focus scope that blocks interaction with lower layers.
///
/// The dialog fills its containing layout region. Place it as the final layer
/// of a viewport-sized stack to create a viewport modal. The child controls the
/// dialog panel's size and appearance. Escape emits the dismissal message.
pub struct Dialog<'a, Message> {
    child: Element<'a, Message>,
    on_dismiss: Box<dyn Fn() -> Message + 'a>,
    scrim_style: Style,
}

impl<'a, Message: 'a> Dialog<'a, Message> {
    /// Creates a modal dialog around `child`.
    #[must_use]
    pub fn new(child: Element<'a, Message>, on_dismiss: impl Fn() -> Message + 'a) -> Self {
        Self {
            child,
            on_dismiss: Box::new(on_dismiss),
            scrim_style: Style::default(),
        }
    }

    /// Sets the style used to fill the containing region behind the dialog panel.
    #[must_use]
    pub const fn scrim_style(mut self, style: Style) -> Self {
        self.scrim_style = style;
        self
    }

    /// Builds the modal dialog element for its containing layout region.
    #[must_use]
    pub fn build(self) -> Element<'a, Message> {
        let dismiss_from_key = self.on_dismiss;
        let panel = self
            .child
            .interactive(true)
            .on_event(EventPhase::Target, |event, context| {
                if matches!(event, UiEvent::Pointer(_)) {
                    context.mark_handled();
                }
            });

        Element::custom("dialog", [panel])
            .layout(LayoutStyle {
                width: Dimension::percent(100),
                height: Dimension::percent(100),
                direction: FlexDirection::Column,
                align: Align::Center,
                justify: Justify::Center,
                ..LayoutStyle::default()
            })
            .style(self.scrim_style)
            .focus_scope(true)
            .pointer_modal(true)
            .interactive(true)
            .on_event(EventPhase::Capture, move |event, context| {
                if matches!(
                    event,
                    UiEvent::Key(key)
                        if key.key == UiKey::Escape && key.action == KeyAction::Press
                ) {
                    context.emit(dismiss_from_key());
                    context.mark_handled();
                    context.prevent_default();
                    context.stop_propagation();
                }
            })
            .on_event(EventPhase::Target, move |event, context| {
                if let UiEvent::Pointer(pointer) = event {
                    match pointer.kind {
                        PointerEventKind::Down(_) => context.capture_pointer(),
                        PointerEventKind::Up(_) => context.release_pointer(),
                        PointerEventKind::Drag(_)
                        | PointerEventKind::Moved
                        | PointerEventKind::Scroll(_)
                        | PointerEventKind::ScrollHorizontal(_) => {}
                    }
                    context.mark_handled();
                    context.prevent_default();
                }
            })
    }
}
