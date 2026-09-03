//! Standard backend-independent widgets for `arborui` applications.
//!
//! Interactive widgets are controlled: application state is borrowed for one
//! view and updates are emitted as owned messages.

mod activation;

/// Bordered content containers.
pub mod block;
/// Focusable push buttons.
pub mod button;
/// Controlled boolean checkboxes.
pub mod checkbox;
/// Vertical flex composition.
pub mod column;
/// Modal focus scopes and pointer barriers.
pub mod dialog;
/// Native terminal images with cell fallback content.
pub mod image;
/// Controlled single-line text input.
pub mod input;
/// Keyed vertical composition.
pub mod list;
/// Horizontal flex composition.
pub mod row;
/// Controlled clipped scrolling.
pub mod scroll;
/// Fixed and flexible empty space.
pub mod spacer;
/// Absolutely overlaid composition.
pub mod stack;
/// Borrowed text elements.
pub mod text;

pub use block::{Block, BorderSet};
pub use button::{Button, button};
pub use checkbox::{Checkbox, checkbox};
pub use column::{column, column_with_gap};
pub use dialog::{Dialog, dialog};
pub use image::{Image, image};
pub use input::{TextInput, text_input};
pub use list::{list, list_with_gap};
pub use row::{row, row_with_gap};
pub use scroll::{ScrollView, scroll_view};
pub use spacer::{flexible_spacer, spacer, spacer_with_dimensions};
pub use stack::stack;
pub use text::text;

#[cfg(test)]
mod tests;
