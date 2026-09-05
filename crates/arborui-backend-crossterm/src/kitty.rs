use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet},
    io::{self, Write},
    sync::{Arc, OnceLock},
};

use arborui_render::{ImageId, ImagePlacement, ImageScene, PixelRect, RgbaImage};
use arborui_terminal::TerminalViewport;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use crossterm::{QueueableCommand, cursor::MoveTo, terminal::EndSynchronizedUpdate};
use flate2::{Compression, write::ZlibEncoder};

use crate::output::coordinate;

const MAX_IMAGE_DIMENSION: u32 = 10_000;
const MAX_ENCODED_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENCODED_CACHE_ENTRIES: usize = 256;
const MAX_PREPARED_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const MIN_COMPRESSION_BYTES: usize = 1_024;
const MAX_CHUNK_BYTES: usize = 4_096;
const TRANSFER_BUCKET_PIXELS: u32 = 64;

#[derive(Debug)]
pub(crate) struct KittyState {
    single_command: bool,
    mappings: HashMap<ImageId, u32>,
    possibly_owned: BTreeSet<u32>,
    next_id: u32,
    stream_uncertain: bool,
    cache: EncodingCache,
}

impl Default for KittyState {
    fn default() -> Self {
        Self {
            single_command: false,
            mappings: HashMap::new(),
            possibly_owned: BTreeSet::new(),
            next_id: 1,
            stream_uncertain: false,
            cache: EncodingCache::new(MAX_ENCODED_CACHE_BYTES),
        }
    }
}

pub(crate) struct PreparedUpdate<'a> {
    single_command: bool,
    recover_stream: bool,
    delete_ids: Vec<u32>,
    placements: Vec<PreparedPlacement<'a>>,
    desired_image_ids: HashSet<ImageId>,
    desired_wire_ids: BTreeSet<u32>,
}

struct PreparedPlacement<'a> {
    id: u32,
    z_index: i32,
    placement: &'a ImagePlacement,
    encoded: Arc<EncodedImage>,
    upload: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TransferSize {
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct EncodedImage {
    format: u8,
    compressed: bool,
    size: TransferSize,
    payload: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct EncodingKey {
    image: ImageId,
    size: TransferSize,
}

#[derive(Debug)]
struct CacheEntry {
    encoded: Arc<EncodedImage>,
    last_used: u64,
}

#[derive(Debug)]
struct EncodingCache {
    entries: HashMap<EncodingKey, CacheEntry>,
    bytes: usize,
    limit: usize,
    entry_limit: usize,
    clock: u64,
}

impl EncodingCache {
    fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            limit,
            entry_limit: MAX_ENCODED_CACHE_ENTRIES,
            clock: 0,
        }
    }

    fn candidate(&self, image: &RgbaImage, size: TransferSize) -> io::Result<Arc<EncodedImage>> {
        let key = EncodingKey {
            image: image.id(),
            size,
        };
        if let Some(entry) = self.entries.get(&key) {
            return Ok(Arc::clone(&entry.encoded));
        }
        encode_pixels(image, size).map(Arc::new)
    }

    fn admit(&mut self, key: EncodingKey, encoded: &Arc<EncodedImage>) {
        let encoded_bytes = encoded.payload.len();
        if encoded_bytes > self.limit {
            return;
        }
        self.clock = self.clock.saturating_add(1);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = self.clock;
            return;
        }
        while self.entries.len() >= self.entry_limit
            || self.bytes.saturating_add(encoded_bytes) > self.limit
        {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(removed.encoded.payload.len());
            }
        }
        self.bytes += encoded_bytes;
        self.entries.insert(
            key,
            CacheEntry {
                encoded: Arc::clone(encoded),
                last_used: self.clock,
            },
        );
    }

    #[cfg(test)]
    fn get_or_encode(
        &mut self,
        image: &RgbaImage,
        size: TransferSize,
    ) -> io::Result<Arc<EncodedImage>> {
        let key = EncodingKey {
            image: image.id(),
            size,
        };
        let encoded = self.candidate(image, size)?;
        self.admit(key, &encoded);
        Ok(encoded)
    }
}

impl KittyState {
    pub(crate) fn new(single_command: bool) -> Self {
        Self {
            single_command,
            ..Self::default()
        }
    }

    pub(crate) fn prepare_with_viewport<'a>(
        &mut self,
        scene: &'a ImageScene,
        viewport: Option<TerminalViewport>,
    ) -> io::Result<PreparedUpdate<'a>> {
        self.prepare_with_budget(scene, viewport, MAX_PREPARED_IMAGE_BYTES)
    }

    fn prepare_with_budget<'a>(
        &mut self,
        scene: &'a ImageScene,
        viewport: Option<TerminalViewport>,
        prepared_limit: usize,
    ) -> io::Result<PreparedUpdate<'a>> {
        let delete_ids = self.possibly_owned.iter().copied().collect();
        let mut transfer_sizes = HashMap::new();
        for placement in scene.placements() {
            let image = placement.image();
            if image.width() > MAX_IMAGE_DIMENSION || image.height() > MAX_IMAGE_DIMENSION {
                continue;
            }
            let candidate = transfer_size(placement, viewport);
            transfer_sizes
                .entry(image.id())
                .and_modify(|size: &mut TransferSize| {
                    if candidate.width > size.width {
                        *size = candidate;
                    }
                })
                .or_insert(candidate);
        }

        let mut encoded_images = HashMap::new();
        let mut attempted_images = HashSet::new();
        let mut prepared_bytes = 0_usize;
        for placement in scene.placements() {
            let image = placement.image();
            let Some(size) = transfer_sizes.get(&image.id()).copied() else {
                continue;
            };
            if !attempted_images.insert(image.id()) {
                continue;
            }
            let key = EncodingKey {
                image: image.id(),
                size,
            };
            let encoded = self.cache.candidate(image, size)?;
            if prepared_bytes.saturating_add(encoded.payload.len()) > prepared_limit {
                continue;
            }
            prepared_bytes += encoded.payload.len();
            self.cache.admit(key, &encoded);
            encoded_images.insert(image.id(), encoded);
        }

        let mut placements = Vec::with_capacity(scene.placements().len());
        let mut desired_image_ids = HashSet::new();
        let mut desired_wire_ids = BTreeSet::new();

        for (index, placement) in scene.placements().iter().enumerate() {
            let image = placement.image();
            let Some(encoded) = encoded_images.get(&image.id()).cloned() else {
                continue;
            };
            let image_id = image.id();
            let wire_id = self.wire_id(image_id);
            let upload = desired_image_ids.insert(image_id);
            if upload {
                desired_wire_ids.insert(wire_id);
            }
            placements.push(PreparedPlacement {
                id: wire_id,
                z_index: i32::try_from(index + 1).unwrap_or(i32::MAX),
                placement,
                encoded,
                upload,
            });
        }

        // IDs are recorded before output starts so a partial write can always
        // be repaired by deleting a conservative superset on the next attempt.
        self.possibly_owned.extend(desired_wire_ids.iter().copied());
        Ok(PreparedUpdate {
            single_command: self.single_command,
            recover_stream: self.stream_uncertain,
            delete_ids,
            placements,
            desired_image_ids,
            desired_wire_ids,
        })
    }

    pub(crate) fn confirm(&mut self, update: &PreparedUpdate<'_>) {
        self.stream_uncertain = false;
        self.possibly_owned.clone_from(&update.desired_wire_ids);
        self.mappings
            .retain(|image, _| update.desired_image_ids.contains(image));
    }

    pub(crate) fn cleanup_ids(&self) -> Vec<u32> {
        self.possibly_owned.iter().copied().collect()
    }

    pub(crate) const fn stream_uncertain(&self) -> bool {
        self.stream_uncertain
    }

    pub(crate) fn mark_stream_uncertain(&mut self) {
        self.stream_uncertain = true;
    }

    pub(crate) fn confirm_cleanup(&mut self) {
        self.mappings.clear();
        self.possibly_owned.clear();
        self.stream_uncertain = false;
    }

    fn wire_id(&mut self, image: ImageId) -> u32 {
        if let Some(id) = self.mappings.get(&image) {
            return *id;
        }
        let id = loop {
            let candidate = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if candidate != 0
                && !self.possibly_owned.contains(&candidate)
                && !self.mappings.values().any(|id| *id == candidate)
            {
                break candidate;
            }
        };
        self.mappings.insert(image, id);
        id
    }
}

impl PreparedUpdate<'_> {
    pub(crate) fn has_output(&self) -> bool {
        self.recover_stream || !self.delete_ids.is_empty() || !self.placements.is_empty()
    }
}

pub(crate) fn write_recovery<W: Write>(
    writer: &mut W,
    update: &PreparedUpdate<'_>,
) -> io::Result<()> {
    write_recovery_if_needed(writer, update.recover_stream)
}

pub(crate) fn write_recovery_if_needed<W: Write>(
    writer: &mut W,
    recover_stream: bool,
) -> io::Result<()> {
    if recover_stream {
        // ST closes a truncated APC. The explicit synchronized-update end
        // repairs an envelope whose closing CSI was consumed by that APC.
        writer.write_all(b"\x1b\\")?;
        writer.queue(EndSynchronizedUpdate)?;
    }
    Ok(())
}

pub(crate) fn write_update_prefix<W: Write>(
    writer: &mut W,
    update: &PreparedUpdate<'_>,
) -> io::Result<()> {
    write_deletions(writer, &update.delete_ids)
}

pub(crate) fn write_update_content<W: Write>(
    writer: &mut W,
    update: &PreparedUpdate<'_>,
) -> io::Result<()> {
    for placement in &update.placements {
        if placement.upload {
            write_image(writer, placement, update.single_command)?;
        } else {
            write_placement(writer, placement)?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn write_update<W: Write>(writer: &mut W, update: &PreparedUpdate<'_>) -> io::Result<()> {
    write_recovery(writer, update)?;
    write_update_prefix(writer, update)?;
    write_update_content(writer, update)
}

pub(crate) fn write_deletions<W: Write>(writer: &mut W, ids: &[u32]) -> io::Result<()> {
    for id in ids {
        write!(writer, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\")?;
    }
    Ok(())
}

fn write_image<W: Write>(
    writer: &mut W,
    prepared: &PreparedPlacement<'_>,
    single_command: bool,
) -> io::Result<()> {
    let id = prepared.id;
    let placement = prepared.placement;
    let image = placement.image();
    let encoded = &prepared.encoded;
    let destination = placement.destination();
    let source = scaled_source(placement.source(), image, encoded.size);
    let x = coordinate(destination.x, 0)?;
    let y = coordinate(destination.y, 0)?;
    writer.queue(MoveTo(x, y))?;
    // Only identified xterm.js sessions need the single-command workaround.
    // Other terminals enforce Kitty's 4096-byte base64 payload limit.
    let first_len = if single_command {
        encoded.payload.len()
    } else {
        encoded.payload.len().min(MAX_CHUNK_BYTES)
    };
    let (first, remaining) = encoded.payload.split_at(first_len);
    let more = if remaining.is_empty() { "" } else { ",m=1" };
    let compression = if encoded.compressed { ",o=z" } else { "" };
    write!(
        writer,
        "\x1b_Ga=T,f={},t=d{compression},s={},v={},i={id},x={},y={},w={},h={},c={},r={},C=1,q=2,z={}{more};{first}\x1b\\",
        encoded.format,
        encoded.size.width,
        encoded.size.height,
        source.x,
        source.y,
        source.width,
        source.height,
        destination.width,
        destination.height,
        prepared.z_index,
    )?;
    let mut chunks = remaining.as_bytes().chunks(MAX_CHUNK_BYTES);
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.len() > 0);
        write!(writer, "\x1b_Gm={more},q=2;")?;
        writer.write_all(chunk)?;
        writer.write_all(b"\x1b\\")?;
    }
    Ok(())
}

fn encode_pixels(image: &RgbaImage, size: TransferSize) -> io::Result<EncodedImage> {
    let rgba = if size.width == image.width() && size.height == image.height() {
        Cow::Borrowed(image.pixels())
    } else {
        Cow::Owned(resize_pixels(image, size))
    };
    let opaque = rgba.chunks_exact(4).all(|pixel| pixel[3] == u8::MAX);
    let raw = if opaque {
        let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
        for pixel in rgba.chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
        }
        Cow::Owned(rgb)
    } else {
        rgba
    };
    let compressed_bytes = if raw.len() >= MIN_COMPRESSION_BYTES {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&raw)?;
        Some(encoder.finish()?)
    } else {
        None
    };
    let compressed = compressed_bytes
        .as_ref()
        .is_some_and(|bytes| bytes.len() < raw.len());
    let bytes = if compressed {
        compressed_bytes.as_deref().unwrap_or_default()
    } else {
        &raw
    };
    Ok(EncodedImage {
        format: if opaque { 24 } else { 32 },
        compressed,
        size,
        payload: STANDARD.encode(bytes),
    })
}

const LINEAR_TABLE_STEPS: usize = 4_096;

struct ResampleTables {
    srgb_to_linear: [f64; 256],
    linear_to_srgb: [u8; LINEAR_TABLE_STEPS + 1],
}

fn resize_pixels(image: &RgbaImage, size: TransferSize) -> Vec<u8> {
    static TABLES: OnceLock<ResampleTables> = OnceLock::new();
    let tables = TABLES.get_or_init(|| ResampleTables {
        srgb_to_linear: std::array::from_fn(|value| {
            let srgb = value as f64 / 255.0;
            if srgb <= 0.040_45 {
                srgb / 12.92
            } else {
                ((srgb + 0.055) / 1.055).powf(2.4)
            }
        }),
        linear_to_srgb: std::array::from_fn(|value| {
            let linear = value as f64 / LINEAR_TABLE_STEPS as f64;
            let srgb = if linear <= 0.003_130_8 {
                linear * 12.92
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            (srgb * 255.0).round().clamp(0.0, 255.0) as u8
        }),
    });
    let source_width = u64::from(image.width());
    let source_height = u64::from(image.height());
    let target_width = u64::from(size.width);
    let target_height = u64::from(size.height);
    let mut resized = Vec::with_capacity(size.width as usize * size.height as usize * 4);

    // Area filtering in premultiplied linear light avoids both downsampling
    // aliasing and dark fringes around transparent source pixels.
    for y in 0..target_height {
        let source_y_start = y * source_height;
        let source_y_end = (y + 1) * source_height;
        let first_y = source_y_start / target_height;
        let last_y = div_ceil(source_y_end, target_height);
        for x in 0..target_width {
            let source_x_start = x * source_width;
            let source_x_end = (x + 1) * source_width;
            let first_x = source_x_start / target_width;
            let last_x = div_ceil(source_x_end, target_width);
            let mut premultiplied = [0.0; 3];
            let mut alpha_sum = 0.0;
            let mut weight_sum = 0.0;

            for source_y in first_y..last_y {
                let pixel_y_start = source_y * target_height;
                let y_weight = source_y_end.min(pixel_y_start + target_height)
                    - source_y_start.max(pixel_y_start);
                for source_x in first_x..last_x {
                    let pixel_x_start = source_x * target_width;
                    let x_weight = source_x_end.min(pixel_x_start + target_width)
                        - source_x_start.max(pixel_x_start);
                    let weight = (x_weight * y_weight) as f64;
                    let offset = ((source_y * source_width + source_x) * 4) as usize;
                    let pixel = &image.pixels()[offset..offset + 4];
                    let alpha = f64::from(pixel[3]) / 255.0;
                    alpha_sum += alpha * weight;
                    weight_sum += weight;
                    for channel in 0..3 {
                        premultiplied[channel] +=
                            tables.srgb_to_linear[usize::from(pixel[channel])] * alpha * weight;
                    }
                }
            }

            if alpha_sum == 0.0 {
                resized.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            for channel in premultiplied {
                let linear = (channel / alpha_sum).clamp(0.0, 1.0);
                let table_index = (linear * LINEAR_TABLE_STEPS as f64).round() as usize;
                resized.push(tables.linear_to_srgb[table_index]);
            }
            resized.push((alpha_sum / weight_sum * 255.0).round() as u8);
        }
    }
    resized
}

fn write_placement<W: Write>(writer: &mut W, prepared: &PreparedPlacement<'_>) -> io::Result<()> {
    let id = prepared.id;
    let placement = prepared.placement;
    let destination = placement.destination();
    let source = scaled_source(placement.source(), placement.image(), prepared.encoded.size);
    let x = coordinate(destination.x, 0)?;
    let y = coordinate(destination.y, 0)?;
    writer.queue(MoveTo(x, y))?;
    write!(
        writer,
        "\x1b_Ga=p,i={id},x={},y={},w={},h={},c={},r={},C=1,q=2,z={}\x1b\\",
        source.x,
        source.y,
        source.width,
        source.height,
        destination.width,
        destination.height,
        prepared.z_index
    )?;
    Ok(())
}

fn transfer_size(placement: &ImagePlacement, viewport: Option<TerminalViewport>) -> TransferSize {
    let image = placement.image();
    let original = TransferSize {
        width: image.width(),
        height: image.height(),
    };
    let Some((viewport, pixels)) =
        viewport.and_then(|viewport| viewport.pixels.map(|pixels| (viewport, pixels)))
    else {
        return original;
    };
    if viewport.cells.is_empty() {
        return original;
    }

    let destination = placement.destination();
    let pixel_width = div_ceil(
        u64::from(destination.width) * u64::from(pixels.width),
        u64::from(viewport.cells.width),
    ) as u32;
    let pixel_height = div_ceil(
        u64::from(destination.height) * u64::from(pixels.height),
        u64::from(viewport.cells.height),
    ) as u32;
    if pixel_width >= image.width() || pixel_height >= image.height() {
        return original;
    }

    if u64::from(pixel_width) * u64::from(image.height())
        >= u64::from(pixel_height) * u64::from(image.width())
    {
        let width = bucket_up(pixel_width).min(image.width());
        TransferSize {
            width,
            height: div_ceil(
                u64::from(image.height()) * u64::from(width),
                u64::from(image.width()),
            ) as u32,
        }
    } else {
        let height = bucket_up(pixel_height).min(image.height());
        TransferSize {
            width: div_ceil(
                u64::from(image.width()) * u64::from(height),
                u64::from(image.height()),
            ) as u32,
            height,
        }
    }
}

fn bucket_up(value: u32) -> u32 {
    value
        .saturating_add(TRANSFER_BUCKET_PIXELS - 1)
        .div_euclid(TRANSFER_BUCKET_PIXELS)
        .saturating_mul(TRANSFER_BUCKET_PIXELS)
}

const fn div_ceil(numerator: u64, denominator: u64) -> u64 {
    numerator.saturating_add(denominator - 1) / denominator
}

fn scaled_source(source: PixelRect, image: &RgbaImage, size: TransferSize) -> PixelRect {
    let right = u64::from(source.x) + u64::from(source.width);
    let bottom = u64::from(source.y) + u64::from(source.height);
    let x = u64::from(source.x) * u64::from(size.width) / u64::from(image.width());
    let y = u64::from(source.y) * u64::from(size.height) / u64::from(image.height());
    let scaled_right = div_ceil(right * u64::from(size.width), u64::from(image.width()));
    let scaled_bottom = div_ceil(bottom * u64::from(size.height), u64::from(image.height()));
    PixelRect::new(
        x as u32,
        y as u32,
        (scaled_right - x) as u32,
        (scaled_bottom - y) as u32,
    )
}

#[cfg(test)]
mod tests {
    use std::{io::Read, time::Instant};

    use arborui_core::{Rect, Size};
    use arborui_render::ImagePlacement;
    use arborui_terminal::{TerminalPixelSize, TerminalViewport};
    use flate2::read::ZlibDecoder;

    use super::*;

    #[test]
    #[ignore = "manual release-mode image encoding metric"]
    fn kitty_encoding_metrics() -> Result<(), Box<dyn std::error::Error>> {
        let width = 1_680;
        let height = 2_240;
        let mut pixels = Vec::with_capacity(width * height * 4);
        let mut noise = 0x1234_5678_u32;
        for index in 0..width * height {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            let x = index % width;
            let y = index / width;
            pixels.extend_from_slice(&[
                (x * 255 / width) as u8 ^ noise as u8,
                (y * 255 / height) as u8 ^ (noise >> 8) as u8,
                ((x + y) * 255 / (width + height)) as u8 ^ (noise >> 16) as u8,
                u8::MAX,
            ]);
        }
        let image = RgbaImage::new(width as u32, height as u32, pixels)?;

        for (label, size) in [
            (
                "full",
                TransferSize {
                    width: image.width(),
                    height: image.height(),
                },
            ),
            (
                "preview",
                TransferSize {
                    width: 448,
                    height: 598,
                },
            ),
        ] {
            let started = Instant::now();
            let encoded = encode_pixels(&image, size)?;
            let elapsed = started.elapsed();

            eprintln!(
                "kitty-encode label={label} source={}x{} transfer={}x{} rgba_bytes={} format={} compressed={} payload_bytes={} elapsed_ms={:.3}",
                image.width(),
                image.height(),
                encoded.size.width,
                encoded.size.height,
                image.pixels().len(),
                encoded.format,
                encoded.compressed,
                encoded.payload.len(),
                elapsed.as_secs_f64() * 1_000.0,
            );
        }
        Ok(())
    }

    #[test]
    fn combines_upload_with_first_placement() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1, 1, vec![1, 2, 3, 4])?;
        let scene =
            ImageScene::from_placements([ImagePlacement::new(image, Rect::new(2, 3, 4, 5))]);
        let mut state = KittyState::default();
        let update = state.prepare_with_viewport(&scene, None)?;
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        assert!(output.contains("\x1b[4;3H"));
        assert!(
            output.contains("\x1b_Ga=T,f=32,t=d,s=1,v=1,i=1,x=0,y=0,w=1,h=1,c=4,r=5,C=1,q=2,z=1;")
        );
        assert!(!output.contains("\x1b_Ga=t,"));
        assert!(!output.contains("\x1b_Ga=p,"));
        Ok(())
    }

    #[test]
    fn upload_is_reduced_to_the_measured_destination_pixels()
    -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(80, 40, [10, 20, 30, 255].repeat(80 * 40))?;
        let scene =
            ImageScene::from_placements([ImagePlacement::new(image, Rect::new(0, 0, 4, 1))]);
        let viewport =
            TerminalViewport::with_pixels(Size::new(8, 4), TerminalPixelSize::new(80, 80));
        let mut state = KittyState::default();
        let update = state.prepare_with_viewport(&scene, Some(viewport))?;
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        assert!(output.contains("s=64,v=32"));
        Ok(())
    }

    #[test]
    fn target_sizing_never_upscales_and_requires_pixel_metrics()
    -> Result<(), Box<dyn std::error::Error>> {
        let small = RgbaImage::new(20, 10, vec![255; 20 * 10 * 4])?;
        let large = RgbaImage::new(80, 40, vec![255; 80 * 40 * 4])?;
        let destination = Rect::new(0, 0, 4, 1);
        let measured = Some(TerminalViewport::with_pixels(
            Size::new(8, 4),
            TerminalPixelSize::new(80, 80),
        ));

        assert_eq!(
            transfer_size(&ImagePlacement::new(small, destination), measured),
            TransferSize {
                width: 20,
                height: 10
            }
        );
        assert_eq!(
            transfer_size(&ImagePlacement::new(large, destination), None),
            TransferSize {
                width: 80,
                height: 40
            }
        );
        Ok(())
    }

    #[test]
    fn resize_filters_premultiplied_alpha_in_linear_light() -> Result<(), Box<dyn std::error::Error>>
    {
        let transparent_edge = RgbaImage::new(2, 1, vec![255, 0, 0, 0, 255, 255, 255, 255])?;
        let contrast = RgbaImage::new(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255])?;
        let target = TransferSize {
            width: 1,
            height: 1,
        };

        assert_eq!(
            resize_pixels(&transparent_edge, target),
            [255, 255, 255, 128]
        );
        assert_eq!(resize_pixels(&contrast, target), [188, 188, 188, 255]);
        Ok(())
    }

    #[test]
    fn movement_and_bucketed_resizes_reuse_the_encoded_image()
    -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(80, 40, [10, 20, 30, 255].repeat(80 * 40))?;
        let viewport =
            TerminalViewport::with_pixels(Size::new(8, 4), TerminalPixelSize::new(80, 80));
        let first_scene = ImageScene::from_placements([ImagePlacement::new(
            image.clone(),
            Rect::new(0, 0, 4, 1),
        )]);
        let mut state = KittyState::default();
        let first_update = state.prepare_with_viewport(&first_scene, Some(viewport))?;
        let first_encoded = Arc::clone(&first_update.placements[0].encoded);
        drop(first_update);
        let second_scene =
            ImageScene::from_placements([ImagePlacement::new(image, Rect::new(1, 1, 5, 1))]);

        let second_update = state.prepare_with_viewport(&second_scene, Some(viewport))?;

        assert!(Arc::ptr_eq(
            &first_encoded,
            &second_update.placements[0].encoded
        ));
        Ok(())
    }

    #[test]
    fn returning_to_an_image_reuses_its_cached_encoding() -> Result<(), Box<dyn std::error::Error>>
    {
        let first_image = RgbaImage::new(2, 2, [1, 2, 3, 255].repeat(4))?;
        let second_image = RgbaImage::new(2, 2, [4, 5, 6, 255].repeat(4))?;
        let first_scene = ImageScene::from_placements([ImagePlacement::new(
            first_image.clone(),
            Rect::new(0, 0, 1, 1),
        )]);
        let second_scene =
            ImageScene::from_placements([ImagePlacement::new(second_image, Rect::new(0, 0, 1, 1))]);
        let mut state = KittyState::default();
        let first_update = state.prepare_with_viewport(&first_scene, None)?;
        let first_encoded = Arc::clone(&first_update.placements[0].encoded);
        state.confirm(&first_update);
        drop(first_update);
        let second_update = state.prepare_with_viewport(&second_scene, None)?;
        state.confirm(&second_update);
        drop(second_update);

        let returned = state.prepare_with_viewport(&first_scene, None)?;

        assert!(Arc::ptr_eq(&first_encoded, &returned.placements[0].encoded));
        Ok(())
    }

    #[test]
    fn encoded_cache_evicts_the_least_recently_used_payload()
    -> Result<(), Box<dyn std::error::Error>> {
        let first = RgbaImage::new(1, 1, vec![1, 2, 3, 4])?;
        let second = RgbaImage::new(1, 1, vec![5, 6, 7, 8])?;
        let size = TransferSize {
            width: 1,
            height: 1,
        };
        let mut cache = EncodingCache::new(8);

        cache.get_or_encode(&first, size)?;
        cache.get_or_encode(&second, size)?;

        assert_eq!(cache.entries.len(), 1);
        assert!(cache.entries.contains_key(&EncodingKey {
            image: second.id(),
            size,
        }));
        assert!(cache.bytes <= cache.limit);
        Ok(())
    }

    #[test]
    fn encoded_cache_bounds_small_entry_overhead() -> Result<(), Box<dyn std::error::Error>> {
        let mut cache = EncodingCache::new(usize::MAX);
        let size = TransferSize {
            width: 1,
            height: 1,
        };
        let mut first_id = None;
        for value in 0..=MAX_ENCODED_CACHE_ENTRIES {
            let image = RgbaImage::new(1, 1, vec![value as u8, 0, 0, 1])?;
            first_id.get_or_insert(image.id());
            cache.get_or_encode(&image, size)?;
        }

        assert_eq!(cache.entries.len(), MAX_ENCODED_CACHE_ENTRIES);
        assert!(!cache.entries.contains_key(&EncodingKey {
            image: first_id.ok_or("missing first image identity")?,
            size,
        }));
        Ok(())
    }

    #[test]
    fn prepared_budget_rejection_does_not_evict_an_accepted_image()
    -> Result<(), Box<dyn std::error::Error>> {
        let first = RgbaImage::new(32, 32, pseudo_random_bytes(32 * 32 * 4))?;
        let second = RgbaImage::new(32, 32, pseudo_random_bytes(32 * 32 * 4))?;
        let size = TransferSize {
            width: 32,
            height: 32,
        };
        let mut state = KittyState {
            cache: EncodingCache::new(6_000),
            ..KittyState::default()
        };
        let first_encoded = state.cache.get_or_encode(&first, size)?;
        let scene = ImageScene::from_placements([
            ImagePlacement::new(first.clone(), Rect::new(0, 0, 1, 1)),
            ImagePlacement::new(second.clone(), Rect::new(1, 0, 1, 1)),
        ]);

        let update = state.prepare_with_budget(&scene, None, 6_000)?;

        assert_eq!(update.placements.len(), 1);
        assert!(Arc::ptr_eq(&first_encoded, &update.placements[0].encoded));
        assert!(state.cache.entries.contains_key(&EncodingKey {
            image: first.id(),
            size,
        }));
        assert!(!state.cache.entries.contains_key(&EncodingKey {
            image: second.id(),
            size,
        }));
        Ok(())
    }

    #[test]
    fn uses_image_ids_for_separate_placements() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1, 1, vec![1, 2, 3, 4])?;
        let scene = ImageScene::from_placements([
            ImagePlacement::new(image.clone(), Rect::new(0, 0, 1, 1)),
            ImagePlacement::new(image, Rect::new(1, 0, 1, 1)),
        ]);
        let mut state = KittyState::default();
        let update = state.prepare_with_viewport(&scene, None)?;
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        assert!(output.contains("\x1b_Ga=T,f=32,t=d,s=1,v=1,i=1,"));
        assert!(output.contains("\x1b_Ga=p,i=1,"));
        assert!(!output.contains(",I="));
        Ok(())
    }

    #[test]
    fn compresses_opaque_uploads_as_rgb() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(32, 32, [10, 20, 30, 255].repeat(32 * 32))?;
        let scene =
            ImageScene::from_placements([ImagePlacement::new(image, Rect::new(0, 0, 1, 1))]);
        let mut state = KittyState::default();
        let update = state.prepare_with_viewport(&scene, None)?;
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        assert!(output.contains("f=24,t=d,o=z,s=32,v=32"));
        let payload = output
            .split("z=1;")
            .nth(1)
            .and_then(|value| value.split("\x1b\\").next())
            .ok_or("missing compressed payload")?;
        let compressed = STANDARD.decode(payload)?;
        let mut decoded = Vec::new();
        ZlibDecoder::new(compressed.as_slice()).read_to_end(&mut decoded)?;
        assert_eq!(decoded, [10, 20, 30].repeat(32 * 32));
        Ok(())
    }

    #[test]
    fn leaves_incompressible_uploads_uncompressed() -> Result<(), Box<dyn std::error::Error>> {
        // Include exact chunk boundaries and the first pixel past each boundary.
        for width in [1, 768, 769, 1_536, 1_537] {
            let image = RgbaImage::new(width, 1, pseudo_random_bytes(width as usize * 4))?;
            let commands = assert_direct_transfer(image, false, false)?;
            let encoded_len = (width as usize * 4).div_ceil(3) * 4;
            assert_eq!(commands, encoded_len.div_ceil(4_096));
        }
        Ok(())
    }

    #[test]
    fn kitty_direct_transfer_obeys_protocol_chunk_limit() -> Result<(), Box<dyn std::error::Error>>
    {
        for (compressed, pixels) in [
            (false, pseudo_random_bytes(512 * 512 * 4)),
            (true, pseudo_random_bytes(8_192).repeat(128)),
        ] {
            let image = RgbaImage::new(512, 512, pixels)?;
            let commands = assert_direct_transfer(image, compressed, false)?;
            assert!(
                commands > 2,
                "exercise first, continuation, and final commands"
            );
        }
        Ok(())
    }

    #[test]
    fn xterm_js_workaround_keeps_one_direct_payload() -> Result<(), Box<dyn std::error::Error>> {
        for (compressed, pixels) in [
            (false, pseudo_random_bytes(512 * 512 * 4)),
            (true, pseudo_random_bytes(8_192).repeat(128)),
        ] {
            let image = RgbaImage::new(512, 512, pixels)?;
            assert_eq!(assert_direct_transfer(image, compressed, true)?, 1);
        }
        Ok(())
    }

    fn assert_direct_transfer(
        image: RgbaImage,
        compressed: bool,
        single_command: bool,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let width = image.width().to_string();
        let height = image.height().to_string();
        let pixels = image.pixels().to_vec();
        let scene = ImageScene::from_placements([
            ImagePlacement::new(image.clone(), Rect::new(2, 3, 4, 5)),
            ImagePlacement::new(image, Rect::new(6, 7, 4, 5)),
        ]);
        let mut state = KittyState::new(single_command);
        let update = state.prepare_with_viewport(&scene, None)?;
        let mut output = Vec::new();
        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        let mut remaining = output
            .strip_prefix("\x1b[4;3H")
            .ok_or("missing cursor move")?
            .strip_suffix(&format!(
                "\x1b[8;7H\x1b_Ga=p,i=1,x=0,y=0,w={width},h={height},c=4,r=5,C=1,q=2,z=2\x1b\\"
            ))
            .ok_or("missing separate placement after completed upload")?;
        let mut payload = String::new();
        let mut commands = 0;
        while !remaining.is_empty() {
            let (command, rest) = remaining
                .strip_prefix("\x1b_G")
                .ok_or("expected graphics APC")?
                .split_once("\x1b\\")
                .ok_or("unterminated graphics APC")?;
            let (header, chunk) = command.split_once(';').ok_or("missing payload")?;
            assert!(
                single_command || chunk.len() <= 4_096,
                "graphics payload is {} bytes, exceeding Kitty's 4096-byte limit",
                chunk.len()
            );
            assert!(!chunk.is_empty());
            assert_eq!(chunk.len() % 4, 0);
            let mut metadata = HashMap::new();
            for field in header.split(',') {
                let (key, value) = field.split_once('=').ok_or("invalid metadata")?;
                assert!(metadata.insert(key, value).is_none(), "duplicate key {key}");
            }
            let mut expected = HashMap::from([("q", "2")]);
            if commands > 0 || !rest.is_empty() {
                expected.insert("m", if rest.is_empty() { "0" } else { "1" });
            }
            if commands == 0 {
                expected.extend([
                    ("a", "T"),
                    ("f", "32"),
                    ("t", "d"),
                    ("s", &width),
                    ("v", &height),
                    ("i", "1"),
                    ("x", "0"),
                    ("y", "0"),
                    ("w", &width),
                    ("h", &height),
                    ("c", "4"),
                    ("r", "5"),
                    ("C", "1"),
                    ("z", "1"),
                ]);
                if compressed {
                    expected.insert("o", "z");
                }
            }
            assert_eq!(metadata, expected);
            if !rest.is_empty() {
                assert!(!single_command);
                assert_eq!(chunk.len(), 4_096);
            }
            payload.push_str(chunk);
            commands += 1;
            remaining = rest;
        }
        let bytes = STANDARD.decode(payload)?;
        let decoded = if compressed {
            let mut decoded = Vec::new();
            ZlibDecoder::new(bytes.as_slice()).read_to_end(&mut decoded)?;
            decoded
        } else {
            bytes
        };
        assert_eq!(decoded, pixels);
        Ok(commands)
    }

    fn pseudo_random_bytes(length: usize) -> Vec<u8> {
        let mut value = 0x1234_5678_u32;
        (0..length)
            .map(|_| {
                value ^= value << 13;
                value ^= value >> 17;
                value ^= value << 5;
                value as u8
            })
            .collect()
    }

    #[test]
    fn placement_z_indexes_preserve_scene_order() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1, 1, vec![0; 4])?;
        let scene = ImageScene::from_placements([
            ImagePlacement::new(image.clone(), Rect::new(0, 0, 1, 1)),
            ImagePlacement::new(image, Rect::new(1, 0, 1, 1)),
        ]);
        let mut state = KittyState::default();
        let update = state.prepare_with_viewport(&scene, None)?;
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        let first = output.find("q=2,z=1").ok_or("missing first placement")?;
        let second = output.find("q=2,z=2").ok_or("missing second placement")?;
        assert!(first < second);
        Ok(())
    }

    #[test]
    fn oversized_kitty_sources_remain_fallback_only() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(MAX_IMAGE_DIMENSION + 1, 1, vec![0; 40_004])?;
        let scene =
            ImageScene::from_placements([ImagePlacement::new(image, Rect::new(0, 0, 1, 1))]);
        let mut state = KittyState::default();
        let update = state.prepare_with_viewport(&scene, None)?;
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        assert!(!update.has_output());
        assert!(output.is_empty());
        Ok(())
    }

    #[test]
    fn uncertain_updates_retain_a_cleanup_superset() -> Result<(), Box<dyn std::error::Error>> {
        let first = RgbaImage::new(1, 1, vec![0; 4])?;
        let second = RgbaImage::new(1, 1, vec![1; 4])?;
        let mut state = KittyState::default();
        let first_scene =
            ImageScene::from_placements([ImagePlacement::new(first, Rect::new(0, 0, 1, 1))]);
        let first_update = state.prepare_with_viewport(&first_scene, None)?;
        state.confirm(&first_update);
        let second_scene =
            ImageScene::from_placements([ImagePlacement::new(second, Rect::new(0, 0, 1, 1))]);

        let _uncertain = state.prepare_with_viewport(&second_scene, None)?;

        assert_eq!(state.cleanup_ids(), [1, 2]);
        Ok(())
    }
}
