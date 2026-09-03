//! Decoding of common encoded raster formats into ArborUI images.
//!
//! This adapter keeps codec dependencies separate from rendering. It detects
//! formats from their contents, applies supported orientation and color-space
//! metadata, and returns validated 8-bit sRGB RGBA pixels as
//! [`arborui_render::RgbaImage`]. Animated inputs decode their first frame.

use std::{
    error::Error,
    fmt,
    fs::File,
    io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom},
    path::Path,
};

use arborui_render::{ImageError as RgbaImageError, MAX_IMAGE_BYTES, RgbaImage};
use image::{
    ColorType, ConvertColorOptions, DynamicImage, ImageDecoder, ImageError as DecoderError,
    ImageFormat, ImageReader, Limits, metadata::Cicp,
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
            DecoderError::IoError(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
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
pub fn load(path: impl AsRef<Path>) -> Result<RgbaImage, DecodeError> {
    let file = File::open(path.as_ref()).map_err(|error| {
        DecodeError::with_source(DecodeErrorKind::Io, "failed to open image", error)
    })?;
    decode_reader(BufReader::new(file))
}

/// Decodes encoded image bytes, detecting their raster format from their contents.
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
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_AXIS);
    limits.max_image_height = Some(MAX_IMAGE_AXIS);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);

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

    let orientation = decoder
        .orientation()
        .map_err(|error| DecodeError::decoder("failed to read image orientation", error))?;
    let mut decoded = DynamicImage::from_decoder(decoder)
        .map_err(|error| DecodeError::decoder("failed to decode image pixels", error))?;
    decoded.apply_orientation(orientation);
    decoded
        .convert_color_space(Cicp::SRGB, ConvertColorOptions::default(), ColorType::Rgba8)
        .map_err(|error| DecodeError::decoder("failed to convert image to sRGB RGBA", error))?;
    let rgba = decoded.into_rgba8();
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
        oriented.extend_from_slice(b"II");
        oriented.extend_from_slice(&42_u16.to_le_bytes());
        oriented.extend_from_slice(&8_u32.to_le_bytes());
        oriented.extend_from_slice(&1_u16.to_le_bytes());
        oriented.extend_from_slice(&0x0112_u16.to_le_bytes());
        oriented.extend_from_slice(&3_u16.to_le_bytes());
        oriented.extend_from_slice(&1_u32.to_le_bytes());
        oriented.extend_from_slice(&6_u16.to_le_bytes());
        oriented.extend_from_slice(&0_u16.to_le_bytes());
        oriented.extend_from_slice(&0_u32.to_le_bytes());
        oriented.extend_from_slice(&encoded[2..]);
        oriented
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
