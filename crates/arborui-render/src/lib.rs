//! Grapheme-aware cell buffers, composition, and transactional frame diffing.

mod buffer;
mod canvas;
mod cell;
mod frame;
mod grapheme_store;
mod hit;
mod image;
mod surface;

pub use buffer::{Buffer, BufferError};
pub use canvas::{Canvas, DrawError, TextDraw};
pub use cell::{Cell, CellContent, HyperlinkId};
pub use frame::{
    CellRun, CommitError, FramePatch, FramePatchValidationError, FramePreparationTimings,
    PatchCell, PatchCellContent, PreparedFrame, RenderError, Renderer, RendererStateId,
};
pub use grapheme_store::{GraphemeId, GraphemeStore, GraphemeStoreError};
pub use hit::{HitId, HitMap};
pub use image::{
    ImageError, ImageId, ImagePlacement, ImageScene, ImageSceneError, MAX_IMAGE_BYTES,
    MAX_IMAGE_PLACEMENTS, MAX_IMAGE_SCENE_BYTES, PixelRect, RgbaImage,
};
pub use surface::{Compositor, Opacity, Surface};
