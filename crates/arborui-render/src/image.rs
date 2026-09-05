use std::{
    collections::HashSet,
    fmt,
    hash::{Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use arborui_core::{Rect, Size};

/// Maximum decoded byte length accepted for one image.
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

/// Maximum total decoded source bytes referenced by one image scene.
pub const MAX_IMAGE_SCENE_BYTES: usize = 256 * 1024 * 1024;

/// Maximum number of placements in one image scene.
pub const MAX_IMAGE_PLACEMENTS: usize = 4_096;

/// Stable process-local identity for immutable image pixels.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImageId(u64);

impl ImageId {
    /// Returns the numeric process-local identity.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Failure to construct decoded RGBA image data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImageError {
    /// At least one pixel dimension is zero.
    Empty,
    /// Pixel dimensions cannot be represented as an RGBA byte length.
    SizeOverflow,
    /// The supplied byte length does not equal `width * height * 4`.
    LengthMismatch {
        /// Required byte length.
        expected: usize,
        /// Supplied byte length.
        actual: usize,
    },
    /// The decoded image exceeds the renderer's per-image safety limit.
    TooLarge {
        /// Supplied byte length.
        bytes: usize,
        /// Maximum accepted byte length.
        maximum: usize,
    },
}

impl fmt::Display for ImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("image dimensions must be nonzero"),
            Self::SizeOverflow => formatter.write_str("image dimensions overflow its byte length"),
            Self::LengthMismatch { expected, actual } => write!(
                formatter,
                "image requires {expected} RGBA bytes but received {actual}"
            ),
            Self::TooLarge { bytes, maximum } => write!(
                formatter,
                "image contains {bytes} bytes, exceeding the {maximum}-byte limit"
            ),
        }
    }
}

impl std::error::Error for ImageError {}

/// Failure to add a placement to a renderer-created image scene.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImageSceneError {
    /// The scene contains too many placements.
    TooManyPlacements {
        /// Maximum accepted placement count.
        maximum: usize,
    },
    /// Unique decoded sources in the scene exceed the safety limit.
    TooLarge {
        /// Total decoded source bytes.
        bytes: usize,
        /// Maximum accepted byte length.
        maximum: usize,
    },
}

impl fmt::Display for ImageSceneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyPlacements { maximum } => {
                write!(
                    formatter,
                    "image scene exceeds the {maximum}-placement limit"
                )
            }
            Self::TooLarge { bytes, maximum } => write!(
                formatter,
                "image scene references {bytes} decoded bytes, exceeding the {maximum}-byte limit"
            ),
        }
    }
}

impl std::error::Error for ImageSceneError {}

/// Immutable decoded 8-bit sRGB RGBA pixels.
///
/// Clones share the pixel allocation and retain the same [`ImageId`]. Creating
/// a new value assigns a new identity even when its pixels are identical.
#[derive(Clone)]
pub struct RgbaImage {
    id: ImageId,
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

impl RgbaImage {
    /// Creates validated row-major RGBA image data.
    pub fn new(width: u32, height: u32, pixels: impl Into<Arc<[u8]>>) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty);
        }
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(ImageError::SizeOverflow)?;
        if expected > MAX_IMAGE_BYTES {
            return Err(ImageError::TooLarge {
                bytes: expected,
                maximum: MAX_IMAGE_BYTES,
            });
        }
        let pixels = pixels.into();
        if pixels.len() != expected {
            return Err(ImageError::LengthMismatch {
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            id: next_image_id(),
            width,
            height,
            pixels,
        })
    }

    /// Returns this immutable source's process-local identity.
    #[must_use]
    pub const fn id(&self) -> ImageId {
        self.id
    }

    /// Returns the pixel width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the pixel height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the row-major RGBA bytes.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

impl fmt::Debug for RgbaImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RgbaImage")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.pixels.len())
            .finish()
    }
}

impl PartialEq for RgbaImage {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for RgbaImage {}

impl Hash for RgbaImage {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// A rectangular source region measured in pixels.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PixelRect {
    /// Horizontal pixel offset.
    pub x: u32,
    /// Vertical pixel offset.
    pub y: u32,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
}

impl PixelRect {
    /// Creates a pixel rectangle.
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// One image source mapped into a terminal-cell rectangle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImagePlacement {
    image: RgbaImage,
    destination: Rect,
    source: PixelRect,
}

impl ImagePlacement {
    /// Maps a complete image into `destination`.
    #[must_use]
    pub const fn new(image: RgbaImage, destination: Rect) -> Self {
        let source = PixelRect::new(0, 0, image.width, image.height);
        Self {
            image,
            destination,
            source,
        }
    }

    pub(crate) fn clipped(image: RgbaImage, destination: Rect, visible: Rect) -> Option<Self> {
        // A clipped destination needs a matching source-pixel crop; retain the
        // cell fallback until that mapping is represented explicitly.
        (destination == visible).then(|| Self::new(image, destination))
    }

    /// Returns the immutable image source.
    #[must_use]
    pub const fn image(&self) -> &RgbaImage {
        &self.image
    }

    /// Returns the visible destination in terminal cells.
    #[must_use]
    pub const fn destination(&self) -> Rect {
        self.destination
    }

    /// Returns the visible source region in pixels.
    #[must_use]
    pub const fn source(&self) -> PixelRect {
        self.source
    }

    pub(crate) fn is_valid_for(&self, frame: Size) -> bool {
        let bounds = Rect::new(0, 0, frame.width, frame.height);
        let source_right = self.source.x.checked_add(self.source.width);
        let source_bottom = self.source.y.checked_add(self.source.height);
        !self.destination.is_empty()
            && self.destination.intersection(bounds) == Some(self.destination)
            && self.source.width != 0
            && self.source.height != 0
            && source_right.is_some_and(|right| right <= self.image.width)
            && source_bottom.is_some_and(|bottom| bottom <= self.image.height)
    }
}

/// Complete backend-neutral native-image scene for one logical frame.
#[derive(Clone, Default)]
pub struct ImageScene {
    placements: Vec<ImagePlacement>,
    stale: Vec<bool>,
}

impl ImageScene {
    /// Creates an empty image scene.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            placements: Vec::new(),
            stale: Vec::new(),
        }
    }

    /// Creates a scene from placements in their supplied order.
    #[must_use]
    pub fn from_placements(placements: impl IntoIterator<Item = ImagePlacement>) -> Self {
        let placements = placements.into_iter().collect::<Vec<_>>();
        let stale = vec![false; placements.len()];
        Self { placements, stale }
    }

    /// Returns image placements in their retained scene order.
    ///
    /// Relative order determines stacking for overlapping placements. Order
    /// between disjoint placements has no rendering significance and can differ
    /// after selective repaint.
    #[must_use]
    pub fn placements(&self) -> &[ImagePlacement] {
        &self.placements
    }

    /// Returns whether the scene contains no placements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }

    pub(crate) fn push(&mut self, placement: ImagePlacement) -> Result<(), ImageSceneError> {
        self.push_with_limits(placement, MAX_IMAGE_PLACEMENTS, MAX_IMAGE_SCENE_BYTES)
    }

    fn push_with_limits(
        &mut self,
        placement: ImagePlacement,
        maximum_placements: usize,
        maximum_bytes: usize,
    ) -> Result<(), ImageSceneError> {
        let desired_placements = self.stale.iter().filter(|stale| !**stale).count();
        if desired_placements >= maximum_placements {
            return Err(ImageSceneError::TooManyPlacements {
                maximum: maximum_placements,
            });
        }
        let source_bytes = self.source_bytes_with(&placement);
        if source_bytes > maximum_bytes {
            return Err(ImageSceneError::TooLarge {
                bytes: source_bytes,
                maximum: maximum_bytes,
            });
        }

        if let Some(index) = self
            .placements
            .iter()
            .zip(&self.stale)
            .position(|(existing, stale)| *stale && existing == &placement)
        {
            let next = self.stale.iter().position(|stale| *stale).unwrap_or(index);
            self.placements.swap(next, index);
            self.stale.swap(next, index);
            self.stale[next] = false;
            return Ok(());
        }

        let replacement = self.stale.iter().position(|stale| *stale);
        if let Some(index) = replacement {
            self.placements[index] = placement;
            self.stale[index] = false;
        } else {
            self.placements.push(placement);
            self.stale.push(false);
        }
        Ok(())
    }

    pub(crate) fn mark_damaged(&mut self, rows: &[bool], clip: Rect) {
        for (placement, stale) in self.placements.iter().zip(&mut self.stale) {
            let destination = placement
                .destination
                .intersection(clip)
                .unwrap_or(Rect::ZERO);
            let start = usize::try_from(destination.y).unwrap_or_default();
            let end = usize::try_from(destination.bottom()).unwrap_or_default();
            let damaged = !destination.is_empty()
                && rows
                    .get(start..end.min(rows.len()))
                    .is_some_and(|rows| rows.iter().any(|selected| *selected));
            *stale |= damaged;
        }
    }

    pub(crate) fn finish_repaint(&mut self) {
        let mut index = 0_usize;
        self.placements.retain(|_| {
            let retain = !self.stale[index];
            index += 1;
            retain
        });
        self.stale.clear();
        self.stale.resize(self.placements.len(), false);
    }

    fn source_bytes_with(&self, placement: &ImagePlacement) -> usize {
        let mut sources = HashSet::new();
        let mut bytes = 0_usize;
        for (existing, stale) in self.placements.iter().zip(&self.stale) {
            if *stale || !sources.insert(existing.image.id()) {
                continue;
            }
            bytes = bytes.saturating_add(existing.image.pixels.len());
        }
        if sources.insert(placement.image.id()) {
            bytes = bytes.saturating_add(placement.image.pixels.len());
        }
        bytes
    }
}

impl fmt::Debug for ImageScene {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageScene")
            .field("placements", &self.placements)
            .finish()
    }
}

impl PartialEq for ImageScene {
    fn eq(&self, other: &Self) -> bool {
        self.placements == other.placements
    }
}

impl Eq for ImageScene {}

fn next_image_id() -> ImageId {
    static NEXT_IMAGE_ID: AtomicU64 = AtomicU64::new(1);
    loop {
        let id = NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return ImageId(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_rgba_dimensions_byte_length_and_limit() {
        assert_eq!(RgbaImage::new(0, 1, Vec::new()), Err(ImageError::Empty));
        assert_eq!(
            RgbaImage::new(2, 1, vec![0; 7]),
            Err(ImageError::LengthMismatch {
                expected: 8,
                actual: 7,
            })
        );
        assert_eq!(
            RgbaImage::new(u32::MAX, u32::MAX, Vec::new()),
            Err(ImageError::SizeOverflow)
        );
        assert_eq!(
            RgbaImage::new(4_097, 4_096, Vec::new()),
            Err(ImageError::TooLarge {
                bytes: 4_097 * 4_096 * 4,
                maximum: MAX_IMAGE_BYTES,
            })
        );
        assert!(RgbaImage::new(2, 1, vec![0; 8]).is_ok());
    }

    #[test]
    fn clones_preserve_identity() -> Result<(), ImageError> {
        let image = RgbaImage::new(1, 1, vec![1, 2, 3, 4])?;

        assert_eq!(image.id(), image.clone().id());
        assert_ne!(image.id(), RgbaImage::new(1, 1, vec![1, 2, 3, 4])?.id());
        Ok(())
    }

    #[test]
    fn repeated_placements_preserve_paint_order() -> Result<(), Box<dyn std::error::Error>> {
        let first = RgbaImage::new(1, 1, vec![0; 4])?;
        let second = RgbaImage::new(1, 1, vec![1; 4])?;
        let mut scene = ImageScene::new();

        scene.push(ImagePlacement::new(first.clone(), Rect::new(0, 0, 1, 1)))?;
        scene.push(ImagePlacement::new(second.clone(), Rect::new(0, 0, 1, 1)))?;
        scene.push(ImagePlacement::new(first.clone(), Rect::new(0, 0, 1, 1)))?;

        assert_eq!(
            scene
                .placements()
                .iter()
                .map(|placement| placement.image().id())
                .collect::<Vec<_>>(),
            [first.id(), second.id(), first.id()]
        );
        Ok(())
    }

    #[test]
    fn selective_repaint_rechecks_reactivated_source_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let first = RgbaImage::new(1, 1, vec![0; 4])?;
        let second = RgbaImage::new(1, 1, vec![1; 4])?;
        let replacement = RgbaImage::new(1, 1, vec![2; 4])?;
        let destination = Rect::new(0, 0, 1, 1);
        let mut scene = ImageScene::new();
        scene.push_with_limits(ImagePlacement::new(first.clone(), destination), 3, 8)?;
        scene.push_with_limits(ImagePlacement::new(second.clone(), destination), 3, 8)?;
        scene.mark_damaged(&[true], destination);
        scene.push_with_limits(ImagePlacement::new(replacement, destination), 3, 8)?;
        scene.push_with_limits(ImagePlacement::new(first, destination), 3, 8)?;

        assert_eq!(
            scene.push_with_limits(ImagePlacement::new(second, destination), 3, 8),
            Err(ImageSceneError::TooLarge {
                bytes: 12,
                maximum: 8,
            })
        );
        Ok(())
    }

    #[test]
    fn partial_cell_clipping_defers_to_the_fallback() -> Result<(), ImageError> {
        let image = RgbaImage::new(100, 100, vec![0; 100 * 100 * 4])?;
        let placement =
            ImagePlacement::clipped(image, Rect::new(10, 5, 10, 5), Rect::new(12, 6, 5, 3));

        assert_eq!(placement, None);
        Ok(())
    }
}
