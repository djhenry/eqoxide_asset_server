//! Shared texture decoding; static-profile limits do not alter legacy output policy.
use anyhow::{ensure, Context, Result};
use image::{ColorType, DynamicImage, ImageDecoder};
use std::io::Cursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureDecodePolicy {
    /// Preserve source alpha; never infer a palette key from material opacity.
    PreserveRgba,
    /// Recover palette index zero in uncompressed 8bpp BMPs. Static-profile BMP
    /// forms that cannot provide that key fail; DDS/PNG retain their source alpha.
    PaletteIndexZero,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeProfile {
    Legacy,
    Static,
}

pub(super) fn check_dimensions(
    width: u32,
    height: u32,
    rgba_bytes: u64,
    limits: Option<&image::Limits>,
) -> Result<()> {
    let Some(limits) = limits else {
        return Ok(());
    };
    ensure!(width != 0 && height != 0, "empty texture dimensions");
    ensure!(
        limits.max_image_width.is_none_or(|max| width <= max)
            && limits.max_image_height.is_none_or(|max| height <= max),
        "texture dimensions exceed static limit"
    );
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(rgba_bytes))
        .context("texture decoded size overflow")?;
    ensure!(
        limits.max_alloc.is_none_or(|max| bytes <= max),
        "texture decoded RGBA allocation exceeds static limit"
    );
    Ok(())
}

pub(crate) fn decode_rgba(
    data: &[u8],
    policy: TextureDecodePolicy,
    name: &str,
    profile: DecodeProfile,
) -> Result<DynamicImage> {
    let limits = if profile == DecodeProfile::Static {
        ensure!(
            data.len() <= crate::static_scene::MAX_TEXTURE_BYTES,
            "texture {name} exceeds encoded byte limit"
        );
        if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            crate::static_scene::validate_png_stream(data)
                .with_context(|| format!("texture {name} incomplete or non-static PNG"))?;
        }
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(crate::static_scene::MAX_TEXTURE_DIMENSION);
        limits.max_image_height = Some(crate::static_scene::MAX_TEXTURE_DIMENSION);
        limits.max_alloc = Some(crate::static_scene::MAX_TEXTURE_BYTES as u64);
        Some(limits)
    } else {
        None
    };
    if let Some(rgba) = super::dds::decode_with_limits(data, limits.as_ref())
        .with_context(|| format!("decode texture {name}"))?
    {
        return Ok(DynamicImage::ImageRgba8(rgba));
    }
    if policy == TextureDecodePolicy::PaletteIndexZero {
        if let Some(rgba) = decode_bmp_keyed(data, limits.as_ref())? {
            return Ok(DynamicImage::ImageRgba8(rgba));
        }
        if profile == DecodeProfile::Static && data.starts_with(b"BM") {
            anyhow::bail!("texture {name} has an unsupported keyed BMP form");
        }
    }
    if profile == DecodeProfile::Legacy {
        let decoded = image::load_from_memory(data)
            .with_context(|| format!("decode texture {name}"))?
            .into_rgba8();
        ensure!(
            !decoded.is_empty(),
            "texture {name} decoded to a zero-pixel image"
        );
        return Ok(DynamicImage::ImageRgba8(decoded));
    }
    let mut reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .with_context(|| format!("identify texture {name}"))?;
    if let Some(limits) = &limits {
        reader.limits(limits.clone());
    }
    let decoder = reader
        .into_decoder()
        .with_context(|| format!("decode texture {name}"))?;
    let sixteen = matches!(
        decoder.color_type(),
        ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16
    );
    if profile == DecodeProfile::Static {
        ensure!(
            !matches!(decoder.color_type(), ColorType::Rgb32F | ColorType::Rgba32F),
            "texture float samples need a separate representation"
        );
    }
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height, if sixteen { 8 } else { 4 }, limits.as_ref())?;
    let decoded =
        DynamicImage::from_decoder(decoder).with_context(|| format!("decode texture {name}"))?;
    ensure!(
        decoded.width() != 0 && decoded.height() != 0,
        "texture {name} decoded to a zero-pixel image"
    );
    Ok(if sixteen {
        DynamicImage::ImageRgba16(decoded.into_rgba16())
    } else {
        DynamicImage::ImageRgba8(decoded.into_rgba8())
    })
}

pub(crate) fn encode_png(rgba: &DynamicImage, name: &str) -> Result<Vec<u8>> {
    let mut output = Cursor::new(Vec::new());
    rgba.write_to(&mut output, image::ImageFormat::Png)
        .with_context(|| format!("re-encode texture {name} as png"))?;
    Ok(output.into_inner())
}

#[cfg(test)]
pub(super) fn legacy_keyed_bmp(data: &[u8]) -> Option<image::RgbaImage> {
    decode_bmp_keyed(data, None).ok().flatten()
}

/// Decode an 8-bit paletted BMP, treating palette index 0 as fully transparent
/// (EQ's masked-texture convention). Returns `None` for any BMP that isn't the
/// uncompressed 8bpp BITMAPINFOHEADER form. The static caller rejects unsupported
/// BMP keying; the legacy caller retains its ordinary-decode fallback.
fn decode_bmp_keyed(
    data: &[u8],
    limits: Option<&image::Limits>,
) -> Result<Option<image::RgbaImage>> {
    if data.len() < 54 || &data[0..2] != b"BM" {
        return Ok(None);
    }
    let rd_u32 = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    let rd_i32 = |o: usize| i32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    let rd_u16 = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);

    let pixel_offset = rd_u32(10) as usize;
    let dib_size = rd_u32(14) as usize;
    if dib_size < 40 {
        return Ok(None); // only BITMAPINFOHEADER (40) or larger
    }
    let width = rd_i32(18);
    let height_raw = rd_i32(22);
    let bpp = rd_u16(28);
    let compression = rd_u32(30);
    if bpp != 8 || compression != 0 || width <= 0 || height_raw == 0 {
        return Ok(None);
    }
    let width = width as usize;
    let top_down = height_raw < 0;
    let height = height_raw.unsigned_abs() as usize;

    check_dimensions(width as u32, height as u32, 4, limits)?;

    // Palette: 4 bytes each (B,G,R,reserved), right after the DIB header.
    let palette_start = 14 + dib_size;
    let mut colors_used = rd_u32(46) as usize;
    if colors_used == 0 {
        colors_used = 256;
    }
    if palette_start + colors_used * 4 > data.len()
        || palette_start + colors_used * 4 > pixel_offset
    {
        return Ok(None);
    }
    if limits.is_some() {
        ensure!(colors_used <= 256, "invalid keyed BMP palette size");
    }
    let palette: Vec<[u8; 3]> = (0..colors_used)
        .map(|i| {
            let p = palette_start + i * 4;
            [data[p + 2], data[p + 1], data[p]] // R,G,B
        })
        .collect();

    // Rows are padded to a multiple of 4 bytes.
    let row_stride = (width + 3) & !3;
    if pixel_offset + row_stride * height > data.len() {
        return Ok(None);
    }

    let mut img = image::RgbaImage::new(width as u32, height as u32);
    for y in 0..height {
        // BMP is bottom-up unless height is negative.
        let src_row = if top_down { y } else { height - 1 - y };
        let row = pixel_offset + src_row * row_stride;
        for x in 0..width {
            let idx = data[row + x] as usize;
            let [r, g, b] = if limits.is_some() {
                palette
                    .get(idx)
                    .copied()
                    .context("keyed BMP index outside palette")?
            } else {
                palette.get(idx).copied().unwrap_or([0, 0, 0])
            };
            let a = if idx == 0 { 0 } else { 255 };
            img.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, a]));
        }
    }
    Ok(Some(img))
}
