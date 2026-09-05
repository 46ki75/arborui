//! Decoding of common encoded raster formats into ArborUI images.
//!
//! This adapter keeps codec dependencies separate from rendering. It detects
//! formats from their contents, applies supported orientation and color-space
//! metadata, and returns validated 8-bit sRGB RGBA pixels as
//! [`arborui_render::RgbaImage`]. Animated inputs decode their first frame.
//!
//! # Color Metadata
//!
//! This is a limited color adapter, not a general-purpose color-management
//! system. Untagged RGB/grayscale inputs are assumed to use sRGB primaries and
//! transfer, except Netpbm inputs, which use their specified BT.709 transfer.
//! Color conversion leaves straight alpha unchanged (apart from reducing
//! higher bit depths to 8 bits). Supported linear and BT.709 transfers operate
//! independently on original 8/16-bit samples before rounding to 8 bits, using
//! at most 64 KiB of lookup storage in addition to the bounded output image.
//!
//! - PNG: supports `sRGB`, `gAMA=1.0` (linear), absent or sRGB `cHRM` primaries,
//!   and full-range identity-matrix `cICP` with sRGB primaries and either sRGB
//!   or linear transfer. `cICP`
//!   overrides `sRGB`, which overrides `gAMA`/`cHRM`. Other gamma, primaries,
//!   or CICP values, all `iCCP` profiles, and HDR mastering metadata are rejected.
//!   In particular, `gAMA=0.45455` alone is gamma 2.2, not an sRGB declaration,
//!   and is outside this adapter's supported transfers.
//! - QOI: honors both the sRGB and linear header flags.
//! - BMP: accepts untagged headers and explicit sRGB/Windows sRGB; calibrated
//!   colors and embedded or linked profiles are rejected. Linked profiles are
//!   never opened.
//! - ICO: applies the PNG/BMP policy to the entry selected by the pixel decoder.
//! - JPEG and WebP: rejects ICC profiles reported by the decoder.
//! - TIFF: rejects ICC and explicit transfer-function, transfer-range,
//!   reference-black/white (`ReferenceBlackWhite`, tag 532), white-point, or
//!   primary-chromaticity tags in the first image directory.
//! - GIF: rejects the ICC application extension before the first image.
//! - TGA: rejects files with an extension area, whose gamma/color-correction
//!   metadata is unsupported. Legacy files and extension-free files assume sRGB.
//! - PNM/PAM: uses full-range BT.709 for visual samples. Nonstandard linear or
//!   sRGB variants cannot be distinguished and must be converted before loading.
//!
//! EXIF orientation is applied where exposed by the codec. Descriptive EXIF/XMP
//! color labels and private extensions are not used as color profiles. For
//! sources outside this policy, convert to an sRGB PNG without an ICC profile
//! before loading. Recognized but unsupported declarations return
//! [`DecodeErrorKind::Unsupported`], not silently relabeled pixels. Malformed
//! headers return [`DecodeErrorKind::InvalidData`]; resource-limit failures
//! return [`DecodeErrorKind::LimitExceeded`]. Rejected profiles are not validated.

mod color;

use std::{
    error::Error,
    fmt,
    fs::File,
    io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom},
    path::Path,
};

use arborui_render::{ImageError as RgbaImageError, MAX_IMAGE_BYTES, RgbaImage};
use image::{
    DynamicImage, ImageDecoder, ImageError as DecoderError, ImageFormat, ImageReader, Limits,
    metadata::Orientation,
};

const MAX_DECODED_BYTES: u64 = MAX_IMAGE_BYTES as u64;
const MAX_IMAGE_AXIS: u32 = (MAX_IMAGE_BYTES / 4) as u32;

/// Broad category of a decoding failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DecodeErrorKind {
    /// The encoded source could not be read.
    Io,
    /// The source format or color space is unsupported.
    Unsupported,
    /// The encoded source is malformed or cannot produce valid RGBA pixels.
    InvalidData,
    /// Decoding would exceed ArborUI's image resource limits.
    LimitExceeded,
}

/// Failure to read or decode an encoded raster image.
#[derive(Debug)]
pub struct DecodeError {
    kind: DecodeErrorKind,
    message: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl DecodeError {
    /// Returns the broad failure category without exposing the codec library.
    #[must_use]
    pub const fn kind(&self) -> DecodeErrorKind {
        self.kind
    }

    fn decoder(context: &'static str, error: DecoderError) -> Self {
        let kind = match &error {
            DecoderError::IoError(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::InvalidData
                ) =>
            {
                DecodeErrorKind::InvalidData
            }
            DecoderError::IoError(_) => DecodeErrorKind::Io,
            DecoderError::Unsupported(_) => DecodeErrorKind::Unsupported,
            DecoderError::Limits(_) => DecodeErrorKind::LimitExceeded,
            DecoderError::Decoding(_) | DecoderError::Encoding(_) | DecoderError::Parameter(_) => {
                DecodeErrorKind::InvalidData
            }
        };
        Self::with_source(kind, context, error)
    }

    fn rgba(error: RgbaImageError) -> Self {
        let kind = match error {
            RgbaImageError::TooLarge { .. } | RgbaImageError::SizeOverflow => {
                DecodeErrorKind::LimitExceeded
            }
            RgbaImageError::Empty | RgbaImageError::LengthMismatch { .. } => {
                DecodeErrorKind::InvalidData
            }
        };
        Self::with_source(kind, "decoded image is invalid", error)
    }

    fn webp(context: &'static str, error: image_webp::DecodingError) -> Self {
        let kind = match &error {
            image_webp::DecodingError::MemoryLimitExceeded
            | image_webp::DecodingError::ImageTooLarge => DecodeErrorKind::LimitExceeded,
            image_webp::DecodingError::IoError(error)
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                DecodeErrorKind::InvalidData
            }
            image_webp::DecodingError::IoError(_) => DecodeErrorKind::Io,
            image_webp::DecodingError::UnsupportedFeature(_) => DecodeErrorKind::Unsupported,
            _ => DecodeErrorKind::InvalidData,
        };
        Self::with_source(kind, context, error)
    }

    fn with_source(
        kind: DecodeErrorKind,
        context: &'static str,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            message: format!("{context}: {source}"),
            source: Some(Box::new(source)),
        }
    }

    fn message(kind: DecodeErrorKind, message: String) -> Self {
        Self {
            kind,
            message,
            source: None,
        }
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

/// Loads and decodes one image file, detecting its format from its contents.
///
/// Uses the crate's [color metadata policy](self#color-metadata). Unsupported
/// profiles return [`DecodeErrorKind::Unsupported`].
pub fn load(path: impl AsRef<Path>) -> Result<RgbaImage, DecodeError> {
    let file = File::open(path.as_ref()).map_err(|error| {
        DecodeError::with_source(DecodeErrorKind::Io, "failed to open image", error)
    })?;
    decode_reader(BufReader::new(file))
}

/// Decodes encoded image bytes, detecting their raster format from their contents.
///
/// Uses the crate's [color metadata policy](self#color-metadata). Unsupported
/// profiles return [`DecodeErrorKind::Unsupported`].
pub fn decode(bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
    decode_reader(Cursor::new(bytes))
}

fn decode_reader<R>(reader: R) -> Result<RgbaImage, DecodeError>
where
    R: BufRead + Seek,
{
    let mut source = reader;
    let is_tga = has_supported_tga_header(&mut source)?;
    let mut reader = ImageReader::new(source)
        .with_guessed_format()
        .map_err(|error| {
            DecodeError::with_source(
                DecodeErrorKind::Io,
                "failed to inspect image contents",
                error,
            )
        })?;
    // TGA has no fixed magic bytes, so image's generic format guesser omits it.
    if reader.format().is_none() && is_tga {
        reader.set_format(ImageFormat::Tga);
    }
    let format = reader.format();
    let mut source = reader.into_inner();
    let source_transfer = color::inspect(&mut source, format)
        .map_err(|error| DecodeError::decoder("failed to inspect image color metadata", error))?;
    let mut reader = ImageReader::new(source);
    if let Some(format) = format {
        reader.set_format(format);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_AXIS);
    limits.max_image_height = Some(MAX_IMAGE_AXIS);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);

    let (mut decoded, orientation) = if reader.format() == Some(ImageFormat::WebP) {
        // image 0.25.8 does not forward max_alloc to WebP. Configure the codec
        // directly so every ICC/EXIF range is bounded before metadata allocation.
        let mut decoder = image_webp::WebPDecoder::new(reader.into_inner())
            .map_err(|error| DecodeError::webp("failed to initialize WebP decoder", error))?;
        decoder.set_memory_limit(MAX_IMAGE_BYTES);
        let (width, height) = decoder.dimensions();
        validate_output_size(width, height)?;
        if decoder
            .icc_profile()
            .map_err(|error| DecodeError::webp("failed to read WebP color profile", error))?
            .is_some()
        {
            return Err(DecodeError::decoder(
                "unsupported image color metadata",
                color::unsupported("WebP ICC profiles"),
            ));
        }
        let orientation = decoder
            .exif_metadata()
            .map_err(|error| DecodeError::webp("failed to read WebP orientation", error))?
            .as_deref()
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms);
        let decoded = if decoder.has_alpha() {
            let mut pixels = image::RgbaImage::new(width, height);
            decoder
                .read_image(&mut pixels)
                .map_err(|error| DecodeError::webp("failed to decode WebP pixels", error))?;
            DynamicImage::ImageRgba8(pixels)
        } else {
            let mut pixels = image::RgbImage::new(width, height);
            decoder
                .read_image(&mut pixels)
                .map_err(|error| DecodeError::webp("failed to decode WebP pixels", error))?;
            DynamicImage::ImageRgb8(pixels)
        };
        (decoded, orientation)
    } else {
        let mut decoder = reader
            .into_decoder()
            .map_err(|error| DecodeError::decoder("failed to initialize image decoder", error))?;
        let (width, height) = decoder.dimensions();
        validate_output_size(width, height)?;
        if decoder.total_bytes() > MAX_DECODED_BYTES {
            return Err(DecodeError::message(
                DecodeErrorKind::LimitExceeded,
                format!(
                    "decoder requires {} bytes, exceeding the {MAX_IMAGE_BYTES}-byte limit",
                    decoder.total_bytes()
                ),
            ));
        }
        if decoder
            .icc_profile()
            .map_err(|error| DecodeError::decoder("failed to read image color profile", error))?
            .is_some()
        {
            return Err(DecodeError::decoder(
                "unsupported image color metadata",
                color::unsupported("ICC profiles"),
            ));
        }
        let orientation = decoder
            .orientation()
            .map_err(|error| DecodeError::decoder("failed to read image orientation", error))?;
        let decoded = DynamicImage::from_decoder(decoder)
            .map_err(|error| DecodeError::decoder("failed to decode image pixels", error))?;
        (decoded, orientation)
    };
    decoded.apply_orientation(orientation);
    // from_decoder defaults to sRGB without interpreting the source metadata.
    let rgba = color::into_rgba8(decoded, source_transfer)
        .map_err(|error| DecodeError::decoder("failed to convert image to sRGB RGBA", error))?;
    let (width, height) = rgba.dimensions();
    RgbaImage::new(width, height, rgba.into_raw()).map_err(DecodeError::rgba)
}

fn has_supported_tga_header<R>(reader: &mut R) -> Result<bool, DecodeError>
where
    R: Read + Seek,
{
    let position = reader.stream_position().map_err(|error| {
        DecodeError::with_source(
            DecodeErrorKind::Io,
            "failed to inspect image contents",
            error,
        )
    })?;
    let mut header = [0_u8; 18];
    let mut length = 0;
    while length < header.len() {
        let read = reader.read(&mut header[length..]).map_err(|error| {
            DecodeError::with_source(
                DecodeErrorKind::Io,
                "failed to inspect image contents",
                error,
            )
        })?;
        if read == 0 {
            break;
        }
        length += read;
    }
    reader.seek(SeekFrom::Start(position)).map_err(|error| {
        DecodeError::with_source(
            DecodeErrorKind::Io,
            "failed to inspect image contents",
            error,
        )
    })?;

    if length != header.len() {
        return Ok(false);
    }
    let image_type = header[2];
    let color_map_valid = if matches!(image_type, 1 | 9) {
        header[1] == 1
    } else {
        header[1] <= 1
    };
    let pixel_depth_valid = match image_type {
        1 | 9 => matches!(header[16], 8 | 16),
        2 | 10 => matches!(header[16], 24 | 32),
        3 | 11 => matches!(header[16], 8 | 16),
        _ => false,
    };
    let width = u16::from_le_bytes([header[12], header[13]]);
    let height = u16::from_le_bytes([header[14], header[15]]);
    let alpha_bits = header[17] & 0x0f;
    Ok(color_map_valid
        && pixel_depth_valid
        && width != 0
        && height != 0
        && matches!(alpha_bits, 0 | 8))
}

fn validate_output_size(width: u32, height: u32) -> Result<(), DecodeError> {
    if width == 0 || height == 0 {
        return Err(DecodeError::message(
            DecodeErrorKind::InvalidData,
            "decoded image dimensions must be nonzero".to_owned(),
        ));
    }
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            DecodeError::message(
                DecodeErrorKind::LimitExceeded,
                "decoded RGBA byte length overflows u64".to_owned(),
            )
        })?;
    if bytes > MAX_DECODED_BYTES {
        return Err(DecodeError::message(
            DecodeErrorKind::LimitExceeded,
            format!(
                "decoded image requires {bytes} RGBA bytes, exceeding the {MAX_IMAGE_BYTES}-byte limit"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, path::PathBuf};

    use image::{DynamicImage, Frame, ImageFormat, Rgba};

    use super::*;

    fn encode(format: ImageFormat) -> Result<Vec<u8>, DecoderError> {
        let source = image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 40, 200, 180, 160, u8::MAX])
            .ok_or_else(|| {
                DecoderError::Parameter(image::error::ParameterError::from_kind(
                    image::error::ParameterErrorKind::DimensionMismatch,
                ))
            })?;
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(source).write_to(&mut encoded, format)?;
        Ok(encoded.into_inner())
    }

    #[test]
    fn decodes_common_static_raster_formats() -> Result<(), Box<dyn Error>> {
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::WebP,
            ImageFormat::Bmp,
            ImageFormat::Ico,
            ImageFormat::Pnm,
            ImageFormat::Qoi,
            ImageFormat::Tga,
            ImageFormat::Tiff,
        ] {
            let decoded = decode(&encode(format)?).map_err(|error| {
                std::io::Error::other(format!("failed to decode {format:?}: {error}"))
            })?;
            assert_eq!((decoded.width(), decoded.height()), (2, 1));
            assert_eq!(decoded.pixels().len(), 8);
        }
        Ok(())
    }

    #[test]
    fn linear_png_is_converted_to_srgb() -> Result<(), Box<dyn Error>> {
        let mut encoded = Vec::new();
        image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut encoded),
            &[128, 128, 128],
            1,
            1,
            image::ExtendedColorType::Rgb8,
        )?;
        insert_png_chunk(&mut encoded, *b"gAMA", &100_000_u32.to_be_bytes());

        let decoded = decode(&encoded)?;

        assert_eq!(decoded.pixels(), [188, 188, 188, 255]);
        Ok(())
    }

    #[test]
    fn linear_png_distinct_black_white_pixels() -> Result<(), Box<dyn Error>> {
        let samples = [
            0, 0, 0, 40, 255, 255, 255, 128, 0, 0, 0, 40, 255, 255, 255, 128,
        ];
        for color in [
            image::ExtendedColorType::Rgba8,
            image::ExtendedColorType::Rgba16,
        ] {
            let bytes = samples
                .iter()
                .flat_map(|sample| {
                    if color == image::ExtendedColorType::Rgba16 {
                        (u16::from(*sample) * 257).to_ne_bytes().to_vec()
                    } else {
                        vec![*sample]
                    }
                })
                .collect::<Vec<_>>();
            let mut encoded = Vec::new();
            image::ImageEncoder::write_image(
                image::codecs::png::PngEncoder::new(&mut encoded),
                &bytes,
                4,
                1,
                color,
            )?;
            insert_png_chunk(&mut encoded, *b"gAMA", &100_000_u32.to_be_bytes());

            assert_eq!(decode(&encoded)?.pixels(), samples);
        }
        Ok(())
    }

    #[test]
    fn tiff_reference_black_white_must_not_be_ignored() -> Result<(), Box<dyn Error>> {
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1, 1, image::Rgb([128; 3])))
            .write_to(&mut encoded, ImageFormat::Tiff)?;
        let mut encoded = encoded.into_inner();
        assert_eq!(&encoded[..2], b"II");
        let offset = usize::try_from(u32::from_le_bytes(encoded[4..8].try_into()?))?;
        let count = u16::from_le_bytes(encoded[offset..offset + 2].try_into()?);
        let directory = encoded[offset + 2..offset + 2 + usize::from(count) * 12].to_vec();
        let offset = u32::try_from(encoded.len())?;
        encoded[4..8].copy_from_slice(&offset.to_le_bytes());
        encoded.extend_from_slice(&(count + 1).to_le_bytes());
        encoded.extend_from_slice(&directory);
        encoded.extend_from_slice(&532_u16.to_le_bytes()); // ReferenceBlackWhite
        encoded.extend_from_slice(&5_u16.to_le_bytes()); // RATIONAL
        encoded.extend_from_slice(&6_u32.to_le_bytes());
        let values_offset = u32::try_from(encoded.len() + 8)?;
        encoded.extend_from_slice(&values_offset.to_le_bytes());
        encoded.extend_from_slice(&0_u32.to_le_bytes()); // End of image directories.
        for value in [128_u32, 255, 128, 255, 128, 255] {
            encoded.extend_from_slice(&value.to_le_bytes());
            encoded.extend_from_slice(&1_u32.to_le_bytes());
        }
        assert_eq!(
            image::load_from_memory(&encoded)?.into_rgb8().as_raw(),
            &[128; 3]
        );

        let error = decode(&encoded).expect_err("unsupported reference range must be rejected");
        assert_eq!(error.kind(), DecodeErrorKind::Unsupported);
        Ok(())
    }

    fn insert_png_chunk(encoded: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
        // Color chunks precede IDAT, immediately after the fixed-size IHDR.
        encoded.splice(33..33, png_chunk(kind, data));
    }

    fn png_chunk(kind: [u8; 4], data: &[u8]) -> Vec<u8> {
        let mut chunk = u32::try_from(data.len())
            .expect("test chunk length fits u32")
            .to_be_bytes()
            .to_vec();
        chunk.extend_from_slice(&kind);
        chunk.extend_from_slice(data);
        let mut crc = u32::MAX;
        for byte in &chunk[4..] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
            }
        }
        chunk.extend_from_slice(&(!crc).to_be_bytes());
        chunk
    }

    #[test]
    fn integer_transfers_preserve_distinct_channels_and_alpha() -> Result<(), Box<dyn Error>> {
        use image::ExtendedColorType::*;

        for (color, channels, tuple, maximum) in [
            (L8, 1, "GRAYSCALE", 255_u32),
            (La8, 2, "GRAYSCALE_ALPHA", 255),
            (Rgb8, 3, "RGB", 255),
            (Rgba8, 4, "RGB_ALPHA", 255),
            (L16, 1, "GRAYSCALE", 65_535),
            (La16, 2, "GRAYSCALE_ALPHA", 65_535),
            (Rgb16, 3, "RGB", 65_535),
            (Rgba16, 4, "RGB_ALPHA", 65_535),
        ] {
            // Every channel and alpha traverse their full input range, with
            // different adjacent pixels and different RGB values per pixel.
            let samples = (0..=maximum)
                .flat_map(|i| {
                    (0..channels)
                        .map(move |channel| ((i + 11_051 * channel) % (maximum + 1)) as u16)
                })
                .collect::<Vec<_>>();
            for transfer in ["srgb", "linear", "bt709"] {
                let mut bytes = Vec::new();
                for sample in &samples {
                    if maximum == 255 {
                        bytes.push(*sample as u8);
                    } else if transfer == "bt709" {
                        bytes.extend_from_slice(&sample.to_be_bytes());
                    } else {
                        bytes.extend_from_slice(&sample.to_ne_bytes());
                    }
                }
                for width in [1, 4, 7, maximum + 1] {
                    let byte_length =
                        usize::try_from(width * channels)? * if maximum == 255 { 1 } else { 2 };
                    let bytes = &bytes[..byte_length];
                    let mut encoded = Vec::new();
                    if transfer == "bt709" {
                        encoded.extend_from_slice(format!("P7\nWIDTH {width}\nHEIGHT 1\nDEPTH {channels}\nMAXVAL {maximum}\nTUPLTYPE {tuple}\nENDHDR\n").as_bytes());
                        encoded.extend_from_slice(bytes);
                    } else {
                        image::ImageEncoder::write_image(
                            image::codecs::png::PngEncoder::new(&mut encoded),
                            bytes,
                            width,
                            1,
                            color,
                        )?;
                        if transfer == "linear" {
                            insert_png_chunk(&mut encoded, *b"gAMA", &100_000_u32.to_be_bytes());
                        }
                    }
                    let decoded = decode(&encoded)?;
                    assert_eq!(decoded.pixels().len(), usize::try_from(width)? * 4);
                    let channels = usize::try_from(channels)?;
                    for (index, (sample, pixel)) in samples
                        .chunks_exact(channels)
                        .zip(decoded.pixels().chunks_exact(4))
                        .enumerate()
                    {
                        for channel in 0..3 {
                            let value = f64::from(sample[if channels <= 2 { 0 } else { channel }])
                                / f64::from(maximum);
                            let expected = match transfer {
                                "linear" => srgb_sample(value),
                                "bt709" => srgb_sample(if value < 0.081 {
                                    value / 4.5
                                } else {
                                    ((value + 0.099) / 1.099).powf(1.0 / 0.45)
                                }),
                                _ => (value * 255.0).round() as u8,
                            };
                            assert!(
                                pixel[channel].abs_diff(expected) <= u8::from(transfer != "srgb"),
                                "{color:?} {transfer} pixel {index} channel {channel}: {pixel:?}, expected {expected}"
                            );
                        }
                        let alpha = if channels == 2 || channels == 4 {
                            (f64::from(sample[channels - 1]) * 255.0 / f64::from(maximum)).round()
                                as u8
                        } else {
                            255
                        };
                        assert_eq!(
                            pixel[3], alpha,
                            "{color:?} {transfer} alpha at pixel {index}"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn linear_palette_png_preserves_distinct_colors_and_transparency() -> Result<(), Box<dyn Error>>
    {
        let mut encoded = b"\x89PNG\r\n\x1a\n".to_vec();
        encoded.extend(png_chunk(
            *b"IHDR",
            &[0, 0, 0, 4, 0, 0, 0, 1, 2, 3, 0, 0, 0],
        ));
        encoded.extend(png_chunk(*b"gAMA", &100_000_u32.to_be_bytes()));
        encoded.extend(png_chunk(
            *b"PLTE",
            &[0, 20, 40, 60, 80, 100, 120, 140, 160, 180, 200, 255],
        ));
        encoded.extend(png_chunk(*b"tRNS", &[0, 40, 128, 255]));
        // zlib stored block containing filter byte 0 and four 2-bit indices.
        encoded.extend(png_chunk(
            *b"IDAT",
            &[0x78, 1, 1, 2, 0, 0xfd, 0xff, 0, 0x1b, 0, 0x1d, 0, 0x1c],
        ));
        encoded.extend(png_chunk(*b"IEND", &[]));
        let decoded = decode(&encoded)?;
        for (pixel, sample) in decoded.pixels().chunks_exact(4).zip([
            [0, 20, 40, 0],
            [60, 80, 100, 40],
            [120, 140, 160, 128],
            [180, 200, 255, 255],
        ]) {
            for channel in 0..3 {
                assert!(
                    pixel[channel].abs_diff(srgb_sample(f64::from(sample[channel]) / 255.0)) <= 1,
                    "{pixel:?} from {sample:?}"
                );
            }
            assert_eq!(pixel[3], sample[3]);
        }
        Ok(())
    }

    #[test]
    fn srgb_float_tiff_only_casts_depth_and_layout() -> Result<(), Box<dyn Error>> {
        let samples = [
            0.0_f32, 0.25, 0.5, 0.0, 1.0, 0.75, 0.2, 0.5, 0.125, 0.5, 1.0, 1.0, -1.0, 2.0, 0.0,
            0.25,
        ];
        for alpha in [false, true] {
            let source =
                image::ImageBuffer::from_raw(4, 1, samples.to_vec()).expect("four RGBA pixels");
            let source = DynamicImage::ImageRgba32F(source);
            let source = if alpha {
                source
            } else {
                DynamicImage::ImageRgb32F(source.into_rgb32f())
            };
            let mut encoded = Cursor::new(Vec::new());
            source.write_to(&mut encoded, ImageFormat::Tiff)?;
            let decoded = decode(encoded.get_ref())?;
            for (pixel, sample) in decoded
                .pixels()
                .chunks_exact(4)
                .zip(samples.chunks_exact(4))
            {
                for channel in 0..4 {
                    let expected = if channel == 3 && !alpha {
                        255
                    } else {
                        (sample[channel].clamp(0.0, 1.0) * 255.0).round() as u8
                    };
                    assert_eq!(pixel[channel], expected);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn source_color_metadata_preserves_alpha_and_srgb_controls() -> Result<(), Box<dyn Error>> {
        for alpha in [0, 40, 128, 255] {
            let mut encoded = Vec::new();
            image::ImageEncoder::write_image(
                image::codecs::png::PngEncoder::new(&mut encoded),
                &[128, 128, 128, alpha],
                1,
                1,
                image::ExtendedColorType::Rgba8,
            )?;
            assert_eq!(decode(&encoded)?.pixels(), [128, 128, 128, alpha]);
            insert_png_chunk(&mut encoded, *b"gAMA", &100_000_u32.to_be_bytes());
            assert_eq!(decode(&encoded)?.pixels(), [188, 188, 188, alpha]);
            // PNG sRGB takes precedence over the lower-priority gAMA chunk.
            insert_png_chunk(&mut encoded, *b"sRGB", &[0]);
            assert_eq!(decode(&encoded)?.pixels(), [128, 128, 128, alpha]);
        }
        Ok(())
    }

    #[test]
    fn rejects_unsupported_png_color_metadata() -> Result<(), Box<dyn Error>> {
        for (kind, data) in [
            (*b"gAMA", 50_000_u32.to_be_bytes().to_vec()),
            (*b"gAMA", 45_455_u32.to_be_bytes().to_vec()),
            (*b"cHRM", vec![0; 32]),
            (*b"cICP", vec![9, 16, 0, 1]),
            (*b"cICP", vec![1, 13, 0, 0]),
            (*b"cICP", vec![1, 13, 1, 1]),
            (*b"iCCP", vec![0; 4]),
            (*b"mDCV", vec![0; 24]),
            (*b"cLLI", vec![0; 8]),
        ] {
            let mut encoded = encode(ImageFormat::Png)?;
            insert_png_chunk(&mut encoded, kind, &data);
            let error = decode(&encoded).expect_err("unsupported color metadata must fail");
            assert_eq!(
                error.kind(),
                DecodeErrorKind::Unsupported,
                "{kind:?}: {error}"
            );
        }
        Ok(())
    }

    #[test]
    fn rejects_embedded_jpeg_and_webp_profiles() -> Result<(), Box<dyn Error>> {
        use image::{ExtendedColorType, ImageEncoder};

        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
        encoder.set_icc_profile(vec![1; 128])?;
        encoder.write_image(&[128; 3], 1, 1, ExtendedColorType::Rgb8)?;
        let mut webp = Vec::new();
        let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut webp);
        encoder.set_icc_profile(vec![1; 128])?;
        encoder.write_image(&[128; 3], 1, 1, ExtendedColorType::Rgb8)?;
        for encoded in [jpeg, webp] {
            let error = decode(&encoded).expect_err("ICC profiles must not be silently ignored");
            assert_eq!(error.kind(), DecodeErrorKind::Unsupported);
        }
        Ok(())
    }

    #[test]
    fn malformed_color_headers_return_invalid_data() -> Result<(), Box<dyn Error>> {
        for (kind, data) in [
            (*b"gAMA", vec![0; 4]),
            (*b"sRGB", vec![4]),
            (*b"gAMA", vec![1]),
        ] {
            let mut encoded = encode(ImageFormat::Png)?;
            insert_png_chunk(&mut encoded, kind, &data);
            assert_eq!(
                decode(&encoded)
                    .expect_err("malformed PNG color chunk must fail")
                    .kind(),
                DecodeErrorKind::InvalidData
            );
        }
        let mut encoded = encode(ImageFormat::Png)?;
        insert_png_chunk(&mut encoded, *b"gAMA", &100_000_u32.to_be_bytes());
        insert_png_chunk(&mut encoded, *b"gAMA", &100_000_u32.to_be_bytes());
        assert_eq!(
            decode(&encoded)
                .expect_err("duplicate PNG color chunk must fail")
                .kind(),
            DecodeErrorKind::InvalidData
        );
        let mut encoded = encode(ImageFormat::Png)?;
        insert_png_chunk(&mut encoded, *b"gAMA", &100_000_u32.to_be_bytes());
        encoded[45] ^= 1;
        assert_eq!(
            decode(&encoded)
                .expect_err("color CRC must be checked before using the declaration")
                .kind(),
            DecodeErrorKind::InvalidData
        );
        let mut encoded = encode(ImageFormat::Qoi)?;
        encoded[13] = 2;
        assert_eq!(
            decode(&encoded)
                .expect_err("invalid QOI color flag must fail")
                .kind(),
            DecodeErrorKind::InvalidData
        );
        Ok(())
    }

    #[test]
    fn tiff_profiles_are_rejected_before_reading_values_in_either_byte_order() {
        for little in [false, true] {
            let mut encoded = if little {
                b"II*\0\x08\0\0\0\x01\0".to_vec()
            } else {
                b"MM\0*\0\0\0\x08\0\x01".to_vec()
            };
            encoded.extend_from_slice(&if little {
                34675_u16.to_le_bytes()
            } else {
                34675_u16.to_be_bytes()
            });
            // Even the entry value/count/offset is absent. Reject the tag before
            // the codec can allocate from a value or swallow an ICC read error.
            let error =
                decode(&encoded).expect_err("TIFF ICC tag must fail before reading its value");
            assert_eq!(error.kind(), DecodeErrorKind::Unsupported);
            assert!(
                error
                    .to_string()
                    .contains("TIFF colorimetry or ICC profiles")
            );
        }
    }

    #[test]
    fn linear_qoi_is_converted_without_changing_alpha() -> Result<(), Box<dyn Error>> {
        let mut encoded = encode(ImageFormat::Qoi)?;
        let srgb = decode(&encoded)?;
        assert_eq!(srgb.pixels(), [10, 20, 30, 40, 200, 180, 160, 255]);
        encoded[13] = 1;
        let linear = decode(&encoded)?;
        for (actual, expected) in linear
            .pixels()
            .chunks_exact(4)
            .zip([[56_u8, 79, 96, 40], [229, 219, 208, 255]])
        {
            for channel in 0..3 {
                assert!(actual[channel].abs_diff(expected[channel]) <= 1);
            }
            assert_eq!(actual[3], expected[3]);
        }
        Ok(())
    }

    #[test]
    fn converts_supported_png_transfers_across_sample_depths() -> Result<(), Box<dyn Error>> {
        for depth in [
            image::ExtendedColorType::La8,
            image::ExtendedColorType::La16,
        ] {
            let samples = [0_u16, 128, 32_768, 65_535];
            for (kind, metadata, is_linear) in [
                (*b"gAMA", 100_000_u32.to_be_bytes(), true),
                (*b"cICP", [1, 8, 0, 1], true),
                (*b"cICP", [1, 13, 0, 1], false),
            ] {
                let pixels = samples
                    .iter()
                    .flat_map(|sample| {
                        if depth == image::ExtendedColorType::La16 {
                            [sample.to_ne_bytes(), 10_280_u16.to_ne_bytes()].concat()
                        } else {
                            vec![(sample / 257) as u8, 40]
                        }
                    })
                    .collect::<Vec<_>>();
                let mut encoded = Vec::new();
                image::ImageEncoder::write_image(
                    image::codecs::png::PngEncoder::new(&mut encoded),
                    &pixels,
                    4,
                    1,
                    depth,
                )?;
                insert_png_chunk(&mut encoded, kind, &metadata);
                let decoded = decode(&encoded)?;
                for (pixel, sample) in decoded.pixels().chunks_exact(4).zip(samples) {
                    let value = if depth == image::ExtendedColorType::La16 {
                        f64::from(sample) / 65_535.0
                    } else {
                        f64::from(sample / 257) / 255.0
                    };
                    let expected = if is_linear {
                        srgb_sample(value)
                    } else {
                        (value * 255.0).round() as u8
                    };
                    assert!(
                        pixel[..3]
                            .iter()
                            .all(|channel| channel.abs_diff(expected) <= 1),
                        "{depth:?} {kind:?}: {pixel:?}, expected {expected}"
                    );
                    assert_eq!(pixel[3], 40);
                }
            }
        }
        Ok(())
    }

    fn srgb_sample(linear: f64) -> u8 {
        let value = if linear <= 0.003_130_8 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (value * 255.0).round() as u8
    }

    #[test]
    fn png_metadata_precedence_and_srgb_chromaticities() -> Result<(), Box<dyn Error>> {
        let mut encoded = encode(ImageFormat::Png)?;
        let chromaticities = [
            31_270_u32, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000,
        ]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect::<Vec<_>>();
        insert_png_chunk(&mut encoded, *b"cHRM", &chromaticities);
        assert_eq!(
            decode(&encoded)?.pixels(),
            decode(&encode(ImageFormat::Png)?)?.pixels()
        );

        let mut encoded = encode(ImageFormat::Png)?;
        insert_png_chunk(&mut encoded, *b"gAMA", &50_000_u32.to_be_bytes());
        insert_png_chunk(&mut encoded, *b"cHRM", &[0; 32]);
        insert_png_chunk(&mut encoded, *b"sRGB", &[0]);
        assert_eq!(
            decode(&encoded)?.pixels(),
            decode(&encode(ImageFormat::Png)?)?.pixels()
        );
        insert_png_chunk(&mut encoded, *b"cICP", &[1, 8, 0, 1]);
        let decoded = decode(&encoded)?;
        assert!(decoded.pixels()[0].abs_diff(srgb_sample(10.0 / 255.0)) <= 1);
        insert_png_chunk(&mut encoded, *b"iCCP", &[0; 4]);
        assert_eq!(
            decode(&encoded).expect_err("ICC is always rejected").kind(),
            DecodeErrorKind::Unsupported
        );
        Ok(())
    }

    #[test]
    fn ico_color_metadata_matches_the_selected_entry() -> Result<(), Box<dyn Error>> {
        use image::codecs::ico::{IcoEncoder, IcoFrame};

        let srgb = encode(ImageFormat::Png)?;
        let mut linear = srgb.clone();
        insert_png_chunk(&mut linear, *b"gAMA", &100_000_u32.to_be_bytes());
        for (images, expected) in [
            ([srgb.clone(), linear.clone(), srgb.clone()], srgb.clone()),
            ([srgb.clone(), srgb.clone(), linear.clone()], linear.clone()),
        ] {
            let frames = images
                .into_iter()
                .map(|image| IcoFrame::with_encoded(image, 2, 1, image::ExtendedColorType::Rgba8))
                .collect::<Result<Vec<_>, _>>()?;
            let mut encoded = Vec::new();
            IcoEncoder::new(&mut encoded).encode_images(&frames)?;
            assert_eq!(decode(&encoded)?.pixels(), decode(&expected)?.pixels());
        }
        insert_png_chunk(&mut linear, *b"iCCP", &[0; 4]);
        let mut encoded = Vec::new();
        IcoEncoder::new(&mut encoded).encode_images(&[IcoFrame::with_encoded(
            linear,
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )?])?;
        assert_eq!(
            decode(&encoded)
                .expect_err("embedded PNG profile must fail")
                .kind(),
            DecodeErrorKind::Unsupported
        );
        Ok(())
    }

    #[test]
    fn rejects_color_declarations_hidden_by_other_codecs() -> Result<(), Box<dyn Error>> {
        let mut bmp = encode(ImageFormat::Bmp)?;
        assert_eq!(&bmp[70..74], &0x7352_4742_u32.to_le_bytes());
        for color in [0_u32, 0x4d42_4544, 0x4c49_4e4b] {
            bmp[70..74].copy_from_slice(&color.to_le_bytes());
            assert_eq!(
                decode(&bmp).expect_err("BMP color space must fail").kind(),
                DecodeErrorKind::Unsupported
            );
        }
        let mut tga = encode(ImageFormat::Tga)?;
        tga.extend_from_slice(&1_u32.to_le_bytes());
        tga.extend_from_slice(&0_u32.to_le_bytes());
        tga.extend_from_slice(b"TRUEVISION-XFILE.\0");
        assert_eq!(
            decode(&tga).expect_err("TGA extension must fail").kind(),
            DecodeErrorKind::Unsupported
        );

        let mut gif = encode(ImageFormat::Gif)?;
        let offset = 13
            + if gif[10] & 0x80 == 0 {
                0
            } else {
                3 * (2_usize << (gif[10] & 7))
            };
        gif.splice(
            offset..offset,
            b"\x21\xff\x0bICCRGBG1012\x01\0\0".iter().copied(),
        );
        assert_eq!(
            decode(&gif).expect_err("GIF ICC must fail").kind(),
            DecodeErrorKind::Unsupported
        );

        for tag in [301_u16, 318, 319, 342, 532, 34675] {
            let mut tiff = encode(ImageFormat::Tiff)?;
            assert_eq!(&tiff[..2], b"II");
            let directory = usize::try_from(u32::from_le_bytes(tiff[4..8].try_into()?))?;
            // Replace the first tag identity, without ever reading its value.
            tiff[directory + 2..directory + 4].copy_from_slice(&tag.to_le_bytes());
            assert_eq!(
                decode(&tiff).expect_err("TIFF color tag must fail").kind(),
                DecodeErrorKind::Unsupported
            );
        }
        Ok(())
    }

    #[test]
    fn netpbm_uses_bt709_not_srgb() -> Result<(), Box<dyn Error>> {
        let decoded = decode(b"P6\n1 1\n255\n\x80\x80\x80")?;
        let expected = srgb_sample(((128.0 / 255.0 + 0.099) / 1.099_f64).powf(1.0 / 0.45));
        assert!(
            decoded.pixels()[..3]
                .iter()
                .all(|channel| channel.abs_diff(expected) <= 1)
        );
        assert_eq!(decoded.pixels()[3], 255);
        Ok(())
    }

    #[test]
    fn preserves_png_rgba_values() -> Result<(), Box<dyn Error>> {
        let decoded = decode(&encode(ImageFormat::Png)?)?;

        assert_eq!(decoded.pixels(), [10, 20, 30, 40, 200, 180, 160, u8::MAX]);
        Ok(())
    }

    #[test]
    fn animated_gif_uses_its_first_frame() -> Result<(), Box<dyn Error>> {
        let mut encoded = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut encoded);
            encoder.encode_frame(Frame::new(image::RgbaImage::from_pixel(
                1,
                1,
                Rgba([255, 0, 0, 255]),
            )))?;
            encoder.encode_frame(Frame::new(image::RgbaImage::from_pixel(
                1,
                1,
                Rgba([0, 0, 255, 255]),
            )))?;
        }

        let decoded = decode(&encoded)?;

        assert_eq!(decoded.pixels(), [255, 0, 0, 255]);
        Ok(())
    }

    #[test]
    fn applies_exif_orientation() -> Result<(), Box<dyn Error>> {
        let encoded = jpeg_with_orientation(encode(ImageFormat::Jpeg)?);

        let decoded = decode(&encoded)?;

        assert_eq!((decoded.width(), decoded.height()), (1, 2));
        Ok(())
    }

    #[test]
    fn webp_preserves_rgb_rgba_and_bounded_orientation() -> Result<(), Box<dyn Error>> {
        use image::{ExtendedColorType, ImageEncoder};

        for color in [ExtendedColorType::Rgb8, ExtendedColorType::Rgba8] {
            let pixels = if color == ExtendedColorType::Rgb8 {
                vec![10, 20, 30, 200, 180, 160]
            } else {
                vec![10, 20, 30, 40, 200, 180, 160, 255]
            };
            for oriented in [false, true] {
                let mut encoded = Vec::new();
                let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut encoded);
                if oriented {
                    encoder.set_exif_metadata(exif_orientation())?;
                }
                encoder.encode(&pixels, 2, 1, color)?;

                let decoded = decode(&encoded)?;

                assert_eq!(
                    (decoded.width(), decoded.height()),
                    if oriented { (1, 2) } else { (2, 1) }
                );
                assert_eq!(
                    decoded.pixels(),
                    [
                        10,
                        20,
                        30,
                        if color == ExtendedColorType::Rgb8 {
                            255
                        } else {
                            40
                        },
                        200,
                        180,
                        160,
                        255
                    ]
                );
            }
        }
        Ok(())
    }

    #[test]
    fn animated_webp_uses_its_first_frame() -> Result<(), Box<dyn Error>> {
        let mut encoded = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0".to_vec();
        encoded.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        encoded.extend_from_slice(b"ANIM\x06\0\0\0\0\0\0\0\0\0");
        for pixel in [[255, 0, 0], [0, 0, 255]] {
            let mut frame = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut frame).encode(
                &pixel,
                1,
                1,
                image::ExtendedColorType::Rgb8,
            )?;
            let chunk = &frame[12..];
            encoded.extend_from_slice(b"ANMF");
            encoded.extend_from_slice(&u32::try_from(16 + chunk.len())?.to_le_bytes());
            // No-blend frames keep this exact-pixel test independent of blending rounding.
            encoded.extend_from_slice(&[0; 15]);
            encoded.push(2);
            encoded.extend_from_slice(chunk);
        }
        let riff_size = u32::try_from(encoded.len() - 8)?;
        encoded[4..8].copy_from_slice(&riff_size.to_le_bytes());

        let decoded = decode(&encoded)?;

        assert_eq!((decoded.width(), decoded.height()), (1, 1));
        assert_eq!(decoded.pixels(), [255, 0, 0, 255]);
        Ok(())
    }

    #[test]
    fn oversized_webp_exif_is_rejected_before_metadata_allocation() {
        for nested in [false, true] {
            let bytes = oversized_webp_metadata(nested, *b"EXIF");
            let mut source = MetadataReadGuard::new(&bytes);

            let error = decode_reader(&mut source).expect_err("oversized EXIF must fail");

            assert!(
                !source.metadata_read_reached,
                "WebP metadata allocation path was reached (nested={nested})"
            );
            assert_eq!(error.kind(), DecodeErrorKind::LimitExceeded);
        }
    }

    #[test]
    fn webp_exif_guard_stops_the_unlimited_decoder_before_allocation() -> Result<(), Box<dyn Error>>
    {
        for nested in [false, true] {
            let bytes = oversized_webp_metadata(nested, *b"EXIF");
            let mut source = MetadataReadGuard::new(&bytes);
            let mut decoder = image::codecs::webp::WebPDecoder::new(&mut source)?;

            let error = decoder
                .orientation()
                .expect_err("guard must stop EXIF reading");

            assert!(matches!(error, DecoderError::IoError(_)));
            assert!(source.metadata_read_reached);
        }
        Ok(())
    }

    #[test]
    fn oversized_webp_icc_is_rejected_before_metadata_allocation() {
        for nested in [false, true] {
            let bytes = oversized_webp_metadata(nested, *b"ICCP");
            let mut source = MetadataReadGuard::new(&bytes);
            let error = decode_reader(&mut source).expect_err("oversized ICC must fail");
            assert!(
                !source.metadata_read_reached,
                "WebP ICC allocation was reached (nested={nested})"
            );
            assert_eq!(error.kind(), DecodeErrorKind::LimitExceeded);
        }
    }

    #[test]
    fn oversized_png_profile_is_rejected_before_reading_its_payload() -> Result<(), Box<dyn Error>>
    {
        use image::codecs::ico::{IcoEncoder, IcoFrame};

        let mut png = encode(ImageFormat::Png)?;
        png.truncate(33);
        png.extend_from_slice(&u32::try_from(MAX_IMAGE_BYTES + 1)?.to_be_bytes());
        png.extend_from_slice(b"iCCP");
        let mut ico = Vec::new();
        IcoEncoder::new(&mut ico).encode_images(&[IcoFrame::with_encoded(
            png.clone(),
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )?])?;
        for encoded in [png, ico] {
            // No profile payload is present: trying to read it would give EOF.
            assert_eq!(
                decode(&encoded)
                    .expect_err("oversized profile must fail")
                    .kind(),
                DecodeErrorKind::LimitExceeded
            );
        }
        Ok(())
    }

    fn oversized_webp_metadata(nested: bool, kind: [u8; 4]) -> Vec<u8> {
        let length = u32::try_from(MAX_IMAGE_BYTES + 1).expect("image limit fits u32");
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0".to_vec();
        bytes.extend_from_slice(&[
            if nested {
                2
            } else if kind == *b"EXIF" {
                8
            } else {
                32
            },
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ]);
        if nested {
            // The pinned decoder can mistake the first frame's VP8L payload for
            // a second chunk header and import its range into image metadata.
            bytes.extend_from_slice(b"ANIM\x06\0\0\0\0\0\0\0\0\0ANMF\x28\0\0\0");
            bytes.extend_from_slice(&[0; 16]);
            bytes.extend_from_slice(b"VP8L\x08\0\0\0");
            bytes.extend_from_slice(&kind);
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(b"JUNK\0\0\0\0");
            assert_eq!(bytes.len(), 92);
        } else {
            bytes.extend_from_slice(b"VP8L\0\0\0\0");
            bytes.extend_from_slice(&kind);
            bytes.extend_from_slice(&length.to_le_bytes());
            assert_eq!(bytes.len(), 46);
        }
        let riff_size = u32::try_from(bytes.len() - 8).expect("fixture size fits u32");
        bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
        bytes
    }

    struct MetadataReadGuard<'a> {
        source: Cursor<&'a [u8]>,
        metadata_read_reached: bool,
    }

    impl<'a> MetadataReadGuard<'a> {
        fn new(bytes: &'a [u8]) -> Self {
            Self {
                source: Cursor::new(bytes),
                metadata_read_reached: false,
            }
        }
    }

    impl Read for MetadataReadGuard<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.source.read(buffer)
        }
    }

    impl BufRead for MetadataReadGuard<'_> {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            self.source.fill_buf()
        }

        fn consume(&mut self, amount: usize) {
            self.source.consume(amount);
        }
    }

    impl Seek for MetadataReadGuard<'_> {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            // image-webp 0.2.4 seeks to metadata before allocating its declared
            // length. Stop there so this regression never makes a large allocation.
            if position == SeekFrom::Start(self.source.get_ref().len() as u64) {
                self.metadata_read_reached = true;
                return Err(std::io::Error::other("blocked metadata allocation path"));
            }
            self.source.seek(position)
        }
    }

    #[test]
    fn load_detects_contents_without_a_matching_extension() -> Result<(), Box<dyn Error>> {
        let path = TemporaryImage::write(&encode(ImageFormat::Png)?)?;

        let decoded = load(path.as_ref())?;

        assert_eq!((decoded.width(), decoded.height()), (2, 1));
        Ok(())
    }

    #[test]
    fn rejects_unknown_malformed_and_oversized_sources() -> Result<(), Box<dyn Error>> {
        let unknown = decode(b"not an encoded image").expect_err("unknown data must fail");
        assert_eq!(unknown.kind(), DecodeErrorKind::Unsupported);

        let mut malformed = encode(ImageFormat::Png)?;
        malformed.truncate(16);
        let malformed = decode(&malformed).expect_err("truncated PNG must fail");
        assert_eq!(malformed.kind(), DecodeErrorKind::InvalidData);

        let oversized = decode(&oversized_bmp_header()).expect_err("oversized image must fail");
        assert_eq!(oversized.kind(), DecodeErrorKind::LimitExceeded);
        Ok(())
    }

    fn oversized_bmp_header() -> Vec<u8> {
        let mut bytes = vec![0; 54];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&54_u32.to_le_bytes());
        bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&5_000_i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&5_000_i32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
        bytes
    }

    fn jpeg_with_orientation(encoded: Vec<u8>) -> Vec<u8> {
        assert!(encoded.starts_with(&[0xff, 0xd8]));
        let mut oriented = Vec::with_capacity(encoded.len() + 36);
        oriented.extend_from_slice(&encoded[..2]);
        oriented.extend_from_slice(&[0xff, 0xe1, 0x00, 0x22]);
        oriented.extend_from_slice(b"Exif\0\0");
        oriented.extend_from_slice(&exif_orientation());
        oriented.extend_from_slice(&encoded[2..]);
        oriented
    }

    fn exif_orientation() -> Vec<u8> {
        let mut exif = b"II".to_vec();
        exif.extend_from_slice(&42_u16.to_le_bytes());
        exif.extend_from_slice(&8_u32.to_le_bytes());
        exif.extend_from_slice(&1_u16.to_le_bytes());
        exif.extend_from_slice(&0x0112_u16.to_le_bytes());
        exif.extend_from_slice(&3_u16.to_le_bytes());
        exif.extend_from_slice(&1_u32.to_le_bytes());
        exif.extend_from_slice(&6_u16.to_le_bytes());
        exif.extend_from_slice(&0_u16.to_le_bytes());
        exif.extend_from_slice(&0_u32.to_le_bytes());
        exif
    }

    struct TemporaryImage(PathBuf);

    impl TemporaryImage {
        fn write(bytes: &[u8]) -> std::io::Result<Self> {
            let path = std::env::temp_dir()
                .join(format!("arborui-image-{}-content.data", std::process::id()));
            std::fs::write(&path, bytes)?;
            Ok(Self(path))
        }
    }

    impl AsRef<Path> for TemporaryImage {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TemporaryImage {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}
