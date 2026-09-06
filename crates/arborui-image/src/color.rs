use std::io::{self, Read, Seek, SeekFrom};

use image::{
    DynamicImage, ImageError, ImageFormat,
    error::{LimitError, LimitErrorKind, UnsupportedError, UnsupportedErrorKind},
};

use super::MAX_DECODED_BYTES;

// Every accepted declaration uses full-range samples and sRGB primaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Transfer {
    Srgb,
    Linear,
    Bt709,
}

pub(super) fn into_rgba8(
    decoded: DynamicImage,
    transfer: Transfer,
) -> Result<image::RgbaImage, ImageError> {
    if transfer == Transfer::Srgb {
        // This only casts sample depth/layout, without a color-space transform.
        return Ok(decoded.into_rgba8());
    }
    let color = decoded.color();
    let channels = usize::from(color.channel_count());
    let pixel_bytes = usize::from(color.bytes_per_pixel());
    let sample_bytes = pixel_bytes / channels;
    let maximum = match sample_bytes {
        1 => 255_u32,
        2 => 65_535,
        _ => return Err(unsupported("non-sRGB samples other than 8/16-bit integers")),
    };
    // No gamut/matrix conversion is needed. Avoid moxcms 0.7.11's NEON
    // neighbor-blue defect, including its float path. The LUT is at most
    // 64 KiB and quantizes only after transferring the original sample.
    let table = (0..=maximum)
        .map(|sample| {
            let value = f64::from(sample) / f64::from(maximum);
            let linear = if transfer == Transfer::Bt709 {
                if value < 0.081 {
                    value / 4.5
                } else {
                    ((value + 0.099) / 1.099).powf(1.0 / 0.45)
                }
            } else {
                value
            };
            let srgb = if linear <= 0.003_130_8 {
                12.92 * linear
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            (srgb * 255.0).round() as u8
        })
        .collect::<Vec<_>>();
    // decode_reader validated this RGBA allocation before decoding pixels;
    // orientation can swap the axes but cannot change the pixel count.
    let mut rgba = image::RgbaImage::new(decoded.width(), decoded.height());
    for (source, target) in decoded
        .as_bytes()
        .chunks_exact(pixel_bytes)
        .zip(rgba.pixels_mut())
    {
        let sample = |channel: usize| {
            let offset = channel * sample_bytes;
            if sample_bytes == 1 {
                u16::from(source[offset])
            } else {
                u16::from_ne_bytes([source[offset], source[offset + 1]])
            }
        };
        for (channel, target) in target.0[..3].iter_mut().enumerate() {
            *target = table[usize::from(sample(if channels <= 2 { 0 } else { channel }))];
        }
        target.0[3] = if color.has_alpha() {
            ((u32::from(sample(channels - 1)) * 255 + maximum / 2) / maximum) as u8
        } else {
            255
        };
    }
    Ok(rgba)
}

// image 0.25.8's ImageDecoder trait does not expose these format-specific
// declarations. Inspect only headers, using fixed-size buffers, before any
// decoder can discard them or allocate/decompress an unsupported profile.
pub(super) fn inspect<R: Read + Seek>(
    reader: &mut R,
    format: Option<ImageFormat>,
) -> Result<Transfer, ImageError> {
    let start = reader.stream_position()?;
    let color = match format {
        Some(ImageFormat::Png) => png(reader)?,
        Some(ImageFormat::Bmp) => {
            reader.seek(SeekFrom::Current(14))?;
            bmp(reader)?;
            Transfer::Srgb
        }
        Some(ImageFormat::Ico) => ico(reader)?,
        Some(ImageFormat::Qoi) => {
            let header = read::<14>(reader)?;
            match header[13] {
                0 => Transfer::Srgb,
                1 => Transfer::Linear,
                _ => return Err(invalid("invalid QOI color-space flag")),
            }
        }
        Some(ImageFormat::Tga) => {
            let end = reader.seek(SeekFrom::End(0))?;
            if end.saturating_sub(start) >= 26 {
                reader.seek(SeekFrom::End(-26))?;
                let footer = read::<26>(reader)?;
                if &footer[8..] == b"TRUEVISION-XFILE.\0" && footer[..4] != [0; 4] {
                    return Err(unsupported("TGA extension-area color metadata"));
                }
            }
            Transfer::Srgb
        }
        Some(ImageFormat::Tiff) => {
            tiff(reader)?;
            Transfer::Srgb
        }
        Some(ImageFormat::Gif) => {
            gif(reader)?;
            Transfer::Srgb
        }
        // Netpbm specifies full-range BT.709, not sRGB, for grayscale/RGB
        // samples. The transfer is immaterial for the bilevel PBM subtype.
        Some(ImageFormat::Pnm) => Transfer::Bt709,
        _ => Transfer::Srgb,
    };
    reader.seek(SeekFrom::Start(start))?;
    Ok(color)
}

fn png<R: Read + Seek>(reader: &mut R) -> Result<Transfer, ImageError> {
    reader.seek(SeekFrom::Current(8))?;
    let (mut gamma, mut chromaticities, mut srgb, mut cicp) = (None, None, None, None);
    loop {
        let length = u32::from_be_bytes(read(reader)?);
        let kind = read::<4>(reader)?;
        if u64::from(length) > MAX_DECODED_BYTES {
            return Err(limit());
        }
        match (&kind, length) {
            (b"IDAT" | b"IEND", _) => break,
            (b"iCCP", _) => return Err(unsupported("PNG ICC profiles")),
            (b"mDCV" | b"cLLI", _) => return Err(unsupported("PNG HDR mastering metadata")),
            (b"gAMA", 4) if gamma.is_none() => {
                gamma = Some(u32::from_be_bytes(png_color_chunk(reader, kind)?))
            }
            (b"cHRM", 32) if chromaticities.is_none() => {
                chromaticities = Some(png_color_chunk::<32>(reader, kind)?)
            }
            (b"sRGB", 1) if srgb.is_none() => srgb = Some(png_color_chunk::<1>(reader, kind)?[0]),
            (b"cICP", 4) if cicp.is_none() => cicp = Some(png_color_chunk::<4>(reader, kind)?),
            (b"gAMA" | b"cHRM" | b"sRGB" | b"cICP", _) => {
                return Err(invalid("invalid or duplicate PNG color chunk"));
            }
            _ => {
                reader.seek(SeekFrom::Current(i64::from(length) + 4))?;
            }
        }
    }
    // PNG 3, section 4.3: cICP > iCCP > sRGB > cHRM/gAMA. ICC and
    // mastering metadata are conservatively rejected even alongside cICP.
    if let Some(cicp) = cicp {
        return match cicp {
            [1, 13, 0, 1] => Ok(Transfer::Srgb),
            [1, 8, 0, 1] => Ok(Transfer::Linear),
            _ => Err(unsupported("PNG cICP color space")),
        };
    }
    if let Some(intent) = srgb {
        return if intent <= 3 {
            Ok(Transfer::Srgb)
        } else {
            Err(invalid("invalid PNG sRGB rendering intent"))
        };
    }
    if let Some(chromaticities) = chromaticities {
        let srgb = [
            31_270_u32, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000,
        ];
        if !chromaticities
            .chunks_exact(4)
            .zip(srgb)
            .all(|(actual, expected)| actual == expected.to_be_bytes())
        {
            return Err(unsupported("PNG non-sRGB chromaticities"));
        }
    }
    match gamma {
        None => Ok(Transfer::Srgb),
        Some(100_000) => Ok(Transfer::Linear),
        Some(0) => Err(invalid("PNG gamma must be nonzero")),
        // Keep the supported transfers explicit: gAMA=0.45455 alone is
        // gamma 2.2, not an sRGB tag.
        _ => Err(unsupported("PNG gamma other than 1.0")),
    }
}

fn png_color_chunk<const N: usize>(
    reader: &mut impl Read,
    kind: [u8; 4],
) -> Result<[u8; N], ImageError> {
    let data = read::<N>(reader)?;
    let expected = u32::from_be_bytes(read(reader)?);
    // png 0.18.1 can discard ancillary chunks with bad checksums. Never
    // apply a color declaration that the pixel decoder would discard.
    let mut crc = u32::MAX;
    for byte in kind.iter().chain(&data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    if !crc != expected {
        return Err(invalid("invalid PNG color chunk checksum"));
    }
    Ok(data)
}

fn bmp<R: Read + Seek>(reader: &mut R) -> Result<(), ImageError> {
    let size = u32::from_le_bytes(read(reader)?);
    if matches!(size, 108 | 124) {
        reader.seek(SeekFrom::Current(52))?;
        // BITMAPV4/V5HEADER: only LCS_sRGB and LCS_WINDOWS_COLOR_SPACE
        // guarantee sRGB. Never follow a PROFILE_LINKED filesystem path.
        if !matches!(u32::from_le_bytes(read(reader)?), 0x7352_4742 | 0x5769_6e20) {
            return Err(unsupported("BMP calibrated colors or color profiles"));
        }
    }
    Ok(())
}

fn ico<R: Read + Seek>(reader: &mut R) -> Result<Transfer, ImageError> {
    let header = read::<6>(reader)?;
    let count = u16::from_le_bytes([header[4], header[5]]);
    let mut selected = None;
    for index in 0..count {
        let entry = read::<16>(reader)?;
        let size = |axis| if axis == 0 { 256 } else { u32::from(axis) };
        let score = (
            u16::from_le_bytes([entry[6], entry[7]]),
            size(entry[0]) * size(entry[1]),
        );
        // Match image 0.25.8's best_entry: the last entry wins a tie with
        // itself; otherwise the first maximum among earlier entries wins.
        if selected.is_none_or(|(best, _)| score > best || (index == count - 1 && score == best)) {
            selected = Some((
                score,
                u32::from_le_bytes(entry[12..16].try_into().expect("four bytes")),
            ));
        }
    }
    let Some((_, offset)) = selected else {
        return Err(invalid("ICO directory contains no image"));
    };
    reader.seek(SeekFrom::Start(u64::from(offset)))?;
    let signature = read::<8>(reader)?;
    reader.seek(SeekFrom::Start(u64::from(offset)))?;
    if &signature == b"\x89PNG\r\n\x1a\n" {
        png(reader)
    } else {
        bmp(reader)?;
        Ok(Transfer::Srgb)
    }
}

fn tiff<R: Read + Seek>(reader: &mut R) -> Result<(), ImageError> {
    let header = read::<4>(reader)?;
    let little = match &header[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(invalid("invalid TIFF byte order")),
    };
    let number = |bytes: &[u8]| {
        if little {
            bytes
                .iter()
                .rev()
                .fold(0_u64, |n, b| (n << 8) | u64::from(*b))
        } else {
            bytes.iter().fold(0_u64, |n, b| (n << 8) | u64::from(*b))
        }
    };
    // The pinned format guesser recognizes classic TIFF, not BigTIFF.
    if number(&header[2..]) != 42 {
        return Err(invalid("invalid TIFF version"));
    }
    let offset = number(&read::<4>(reader)?);
    reader.seek(SeekFrom::Start(offset))?;
    let count = number(&read::<2>(reader)?);
    // TIFF 6.0 colorimetry tags and ICC tag 34675. Read tag identities, not
    // values: image's icc_profile() swallows TIFF value/limit errors as None.
    for _ in 0..count {
        let tag = number(&read::<2>(reader)?);
        if matches!(tag, 301 | 318 | 319 | 342 | 532 | 34675) {
            return Err(unsupported("TIFF colorimetry or ICC profiles"));
        }
        reader.seek(SeekFrom::Current(10))?;
    }
    Ok(())
}

fn gif<R: Read + Seek>(reader: &mut R) -> Result<(), ImageError> {
    let header = read::<13>(reader)?;
    if header[10] & 0x80 != 0 {
        reader.seek(SeekFrom::Current(3 * (2_i64 << (header[10] & 7))))?;
    }
    // ICC.1's GIF application extension must precede the first image.
    while read::<1>(reader)?[0] == 0x21 {
        let label = read::<1>(reader)?[0];
        let mut first = true;
        loop {
            let size = read::<1>(reader)?[0];
            if size == 0 {
                break;
            }
            if label == 0xff && first && size == 11 {
                if &read::<11>(reader)? == b"ICCRGBG1012" {
                    return Err(unsupported("GIF ICC profiles"));
                }
            } else {
                reader.seek(SeekFrom::Current(i64::from(size)))?;
            }
            first = false;
        }
    }
    Ok(())
}

fn read<const N: usize>(reader: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn unsupported(metadata: &'static str) -> ImageError {
    ImageError::Unsupported(UnsupportedError::from_format_and_kind(
        image::error::ImageFormatHint::Unknown,
        UnsupportedErrorKind::GenericFeature(metadata.to_owned()),
    ))
}

fn invalid(message: &'static str) -> ImageError {
    io::Error::new(io::ErrorKind::InvalidData, message).into()
}

fn limit() -> ImageError {
    ImageError::Limits(LimitError::from_kind(LimitErrorKind::InsufficientMemory))
}
