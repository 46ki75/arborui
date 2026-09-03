use std::{
    collections::{BTreeSet, HashMap, HashSet},
    io::{self, Write},
};

use arborui_render::{ImageId, ImagePlacement, ImageScene, RgbaImage};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use crossterm::{QueueableCommand, cursor::MoveTo, terminal::EndSynchronizedUpdate};
use flate2::{Compression, write::ZlibEncoder};

const MAX_IMAGE_DIMENSION: u32 = 10_000;

#[derive(Debug)]
pub(crate) struct KittyState {
    mappings: HashMap<ImageId, u32>,
    possibly_owned: BTreeSet<u32>,
    next_id: u32,
    stream_uncertain: bool,
}

impl Default for KittyState {
    fn default() -> Self {
        Self {
            mappings: HashMap::new(),
            possibly_owned: BTreeSet::new(),
            next_id: 1,
            stream_uncertain: false,
        }
    }
}

pub(crate) struct PreparedUpdate<'a> {
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
    upload: bool,
}

impl KittyState {
    pub(crate) fn prepare<'a>(&mut self, scene: &'a ImageScene) -> PreparedUpdate<'a> {
        let delete_ids = self.possibly_owned.iter().copied().collect();
        let mut placements = Vec::with_capacity(scene.placements().len());
        let mut desired_image_ids = HashSet::new();
        let mut desired_wire_ids = BTreeSet::new();

        for (index, placement) in scene.placements().iter().enumerate() {
            let image = placement.image();
            if image.width() > MAX_IMAGE_DIMENSION || image.height() > MAX_IMAGE_DIMENSION {
                continue;
            }
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
                upload,
            });
        }

        // IDs are recorded before output starts so a partial write can always
        // be repaired by deleting a conservative superset on the next attempt.
        self.possibly_owned.extend(desired_wire_ids.iter().copied());
        PreparedUpdate {
            recover_stream: self.stream_uncertain,
            delete_ids,
            placements,
            desired_image_ids,
            desired_wire_ids,
        }
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
            write_image(writer, placement)?;
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

fn write_image<W: Write>(writer: &mut W, prepared: &PreparedPlacement<'_>) -> io::Result<()> {
    let id = prepared.id;
    let placement = prepared.placement;
    let image = placement.image();
    let (format, encoded) = encode_pixels(image)?;
    let destination = placement.destination();
    let source = placement.source();
    let x = u16::try_from(destination.x).map_err(|_| invalid_coordinate(destination.x))?;
    let y = u16::try_from(destination.y).map_err(|_| invalid_coordinate(destination.y))?;
    writer.queue(MoveTo(x, y))?;
    // xterm.js accepts direct payloads of this size but fails to complete some
    // multi-command uploads. Auto detection already excludes indirect sessions.
    let payload = STANDARD.encode(encoded);
    write!(
        writer,
        "\x1b_Ga=T,f={format},t=d,o=z,s={},v={},i={id},x={},y={},w={},h={},c={},r={},C=1,q=2,z={};{payload}\x1b\\",
        image.width(),
        image.height(),
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

fn encode_pixels(image: &RgbaImage) -> io::Result<(u8, Vec<u8>)> {
    let opaque = image
        .pixels()
        .chunks_exact(4)
        .all(|pixel| pixel[3] == u8::MAX);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    if opaque {
        let mut rgb = Vec::with_capacity(image.pixels().len() / 4 * 3);
        for pixel in image.pixels().chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
        }
        encoder.write_all(&rgb)?;
    } else {
        encoder.write_all(image.pixels())?;
    }
    Ok((if opaque { 24 } else { 32 }, encoder.finish()?))
}

fn write_placement<W: Write>(writer: &mut W, prepared: &PreparedPlacement<'_>) -> io::Result<()> {
    let id = prepared.id;
    let placement = prepared.placement;
    let destination = placement.destination();
    let source = placement.source();
    let x = u16::try_from(destination.x).map_err(|_| invalid_coordinate(destination.x))?;
    let y = u16::try_from(destination.y).map_err(|_| invalid_coordinate(destination.y))?;
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

fn invalid_coordinate(value: i32) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("terminal coordinate {value} is outside the supported range"),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use arborui_core::Rect;
    use arborui_render::ImagePlacement;
    use flate2::read::ZlibDecoder;

    use super::*;

    #[test]
    fn combines_upload_with_first_placement() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1, 1, vec![1, 2, 3, 4])?;
        let scene =
            ImageScene::from_placements([ImagePlacement::new(image, Rect::new(2, 3, 4, 5))]);
        let mut state = KittyState::default();
        let update = state.prepare(&scene);
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        assert!(output.contains("\x1b[4;3H"));
        assert!(
            output.contains(
                "\x1b_Ga=T,f=32,t=d,o=z,s=1,v=1,i=1,x=0,y=0,w=1,h=1,c=4,r=5,C=1,q=2,z=1;"
            )
        );
        assert!(!output.contains("\x1b_Ga=t,"));
        assert!(!output.contains("\x1b_Ga=p,"));
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
        let update = state.prepare(&scene);
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        assert!(output.contains("\x1b_Ga=T,f=32,t=d,o=z,s=1,v=1,i=1,"));
        assert!(output.contains("\x1b_Ga=p,i=1,"));
        assert!(!output.contains(",I="));
        Ok(())
    }

    #[test]
    fn compresses_opaque_uploads_as_rgb() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(2, 2, [10, 20, 30, 255].repeat(4))?;
        let scene =
            ImageScene::from_placements([ImagePlacement::new(image, Rect::new(0, 0, 1, 1))]);
        let mut state = KittyState::default();
        let update = state.prepare(&scene);
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        assert!(output.contains("f=24,t=d,o=z,s=2,v=2"));
        let payload = output
            .split("z=1;")
            .nth(1)
            .and_then(|value| value.split("\x1b\\").next())
            .ok_or("missing compressed payload")?;
        let compressed = STANDARD.decode(payload)?;
        let mut decoded = Vec::new();
        ZlibDecoder::new(compressed.as_slice()).read_to_end(&mut decoded)?;
        assert_eq!(decoded, [10, 20, 30].repeat(4));
        Ok(())
    }

    #[test]
    fn writes_image_as_one_direct_payload() -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbaImage::new(1_025, 1, pseudo_random_bytes(1_025 * 4))?;
        let scene =
            ImageScene::from_placements([ImagePlacement::new(image, Rect::new(0, 0, 1, 1))]);
        let mut state = KittyState::default();
        let update = state.prepare(&scene);
        let mut output = Vec::new();

        write_update(&mut output, &update)?;

        let output = String::from_utf8(output)?;
        assert!(!output.contains("\x1b_Gm="));
        assert!(!output.contains("m=1"));
        Ok(())
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
        let update = state.prepare(&scene);
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
        let update = state.prepare(&scene);
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
        let first_update = state.prepare(&first_scene);
        state.confirm(&first_update);
        let second_scene =
            ImageScene::from_placements([ImagePlacement::new(second, Rect::new(0, 0, 1, 1))]);

        let _uncertain = state.prepare(&second_scene);

        assert_eq!(state.cleanup_ids(), [1, 2]);
        Ok(())
    }
}
