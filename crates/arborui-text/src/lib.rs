//! Unicode grapheme segmentation and terminal width measurement.
//!
//! The crate keeps UTF-8 byte offsets, user-visible grapheme clusters, and
//! terminal display columns as distinct concepts.

mod edit;
mod measure;

pub use edit::{ByteOffset, Selection, TextBuffer, TextEdit, TextMovement};
pub use measure::{
    Grapheme, Graphemes, TextMetrics, WidthPolicy, grapheme_width, graphemes, measure,
};
