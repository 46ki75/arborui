//! Grapheme-aware cell buffers, composition, and transactional frame diffing.

mod buffer;
mod canvas;
mod cell;
mod frame;
mod grapheme_store;
mod hit;
mod surface;

pub use buffer::{Buffer, BufferError};
pub use canvas::{Canvas, DrawError, TextDraw};
pub use cell::{Cell, CellContent, HyperlinkId};
pub use frame::{
    CellRun, CommitError, FramePatch, PatchCell, PatchCellContent, PreparedFrame, RenderError,
    Renderer, RendererStateId,
};
pub use grapheme_store::{GraphemeId, GraphemeStore, GraphemeStoreError};
pub use hit::{HitId, HitMap};
pub use surface::{Compositor, Opacity, Surface};
