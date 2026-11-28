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
    let dds = if let Some(limits) = &limits {
        super::dds::decode_static_with_limits(data, limits)
    } else {
        super::dds::decode_with_limits(data, None)
    };
    if let Some(rgba) = dds.with_context(|| format!("decode texture {name}"))? {
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
        let mut rgba = decoded.into_rgba8();
        super::dds::restore_static_bc1_alpha(data, &mut rgba)
            .with_context(|| format!("restore texture {name} BC1 alpha"))?;
        DynamicImage::ImageRgba8(rgba)
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

#[cfg(test)]
mod bc1_tests {
    use super::*;

    fn word(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture(width: u32, height: u32, dxgi: Option<u32>, blocks: &[(u16, u16, u32)]) -> Vec<u8> {
        let mut data = vec![0; if dxgi.is_some() { 148 } else { 128 }];
        data[..4].copy_from_slice(b"DDS ");
        for (offset, value) in [
            (4, 124),
            (8, 0x81007),
            (12, height),
            (16, width),
            (20, blocks.len() as u32 * 8),
            (76, 32),
            (80, 4),
            (108, 0x1000),
        ] {
            word(&mut data, offset, value);
        }
        data[84..88].copy_from_slice(if dxgi.is_some() { b"DX10" } else { b"DXT1" });
        if let Some(format) = dxgi {
            for (offset, value) in [(128, format), (132, 3), (140, 1)] {
                word(&mut data, offset, value);
            }
        }
        for &(c0, c1, selectors) in blocks {
            data.extend_from_slice(&c0.to_le_bytes());
            data.extend_from_slice(&c1.to_le_bytes());
            data.extend_from_slice(&selectors.to_le_bytes());
        }
        data
    }

    fn static_image(data: &[u8], policy: TextureDecodePolicy) -> image::RgbaImage {
        decode_rgba(data, policy, "bc1", DecodeProfile::Static)
            .unwrap()
            .into_rgba8()
    }

    #[test]
    fn bc1_classic_literal_cutout_retains_pinned_rgb_and_source_alpha() {
        // Every row is black, white, midpoint gray, transparent black.
        let data = fixture(4, 4, None, &[(0, 65535, 0xe4e4e4e4)]);
        let before = data.clone();
        for policy in [
            TextureDecodePolicy::PreserveRgba,
            TextureDecodePolicy::PaletteIndexZero,
        ] {
            let actual = static_image(&data, policy);
            assert_eq!(
                actual.as_raw(),
                &[0, 0, 0, 255, 255, 255, 255, 255, 128, 128, 128, 255, 0, 0, 0, 0].repeat(4)
            );
        }
        assert_eq!(data, before);
    }

    #[test]
    fn bc1_dx10_cutout_accepts_existing_formats_and_alpha_conventions() {
        for format in 70..=72 {
            for convention in [0, 1] {
                let mut data = fixture(4, 4, Some(format), &[(0, 65535, u32::MAX)]);
                word(&mut data, 144, convention);
                for policy in [
                    TextureDecodePolicy::PreserveRgba,
                    TextureDecodePolicy::PaletteIndexZero,
                ] {
                    assert!(
                        static_image(&data, policy).pixels().all(|p| p[3] == 0),
                        "format {format}, convention {convention}"
                    );
                }
            }
        }
    }

    #[test]
    fn bc1_every_selector_position_and_endpoint_order_preserves_rgb() {
        for (c0, c1, transparent) in [
            (0x1234, 0xabcd, true),
            (0x1234, 0x1234, true),
            (0xabcd, 0x1234, false),
        ] {
            for position in 0..16 {
                for selector in 0..4 {
                    let data = fixture(4, 4, None, &[(c0, c1, selector << (2 * position))]);
                    let baseline = image::load_from_memory(&data).unwrap().into_rgb8();
                    for policy in [
                        TextureDecodePolicy::PreserveRgba,
                        TextureDecodePolicy::PaletteIndexZero,
                    ] {
                        let actual = static_image(&data, policy);
                        for (i, (rgb, rgba)) in baseline.pixels().zip(actual.pixels()).enumerate() {
                            assert_eq!(&rgba.0[..3], &rgb.0, "RGB at {i}, position {position}");
                            let expected = if transparent && selector == 3 && i == position {
                                0
                            } else {
                                255
                            };
                            assert_eq!(rgba[3], expected, "alpha at {i}, position {position}, selector {selector}, endpoints {c0}/{c1}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn bc1_multiblock_rows_and_optional_flags_ignore_unused_rgb_fields() {
        for (width, blocks) in [
            (4, vec![(0, 65535, 0x03020100), (65535, 0, u32::MAX)]),
            (
                8,
                vec![
                    (0, 65535, 0x03020100),
                    (65535, 0, u32::MAX),
                    (77, 77, 0x01234567),
                    (0x1234, 0xabcd, 0xfedcba98),
                ],
            ),
        ] {
            for dxgi in [None, Some(71)] {
                for flags in [4, 5] {
                    let mut data = fixture(width, 8, dxgi, &blocks);
                    word(&mut data, 80, flags);
                    for (offset, value) in [
                        (88, 16),
                        (92, 123),
                        (96, 456),
                        (100, 789),
                        (104, u32::MAX),
                        (28, 3),
                    ] {
                        word(&mut data, offset, value);
                    }
                    // Existing compressed decoding consumes the base level, even with partial trailing mips.
                    data.extend_from_slice(&[23, 42]);
                    let baseline = image::load_from_memory(&data).unwrap().into_rgb8();
                    let actual = static_image(&data, TextureDecodePolicy::PreserveRgba);
                    for (x, y, rgba) in actual.enumerate_pixels() {
                        let &(a, b, bits) = &blocks[((y / 4) * (width / 4) + x / 4) as usize];
                        let index = (bits >> (2 * ((y % 4) * 4 + x % 4))) & 3;
                        assert_eq!(rgba[3], if a <= b && index == 3 { 0 } else { 255 });
                        assert_eq!(&rgba.0[..3], &baseline.get_pixel(x, y).0);
                    }
                }
            }
        }
    }

    #[test]
    fn bc1_legacy_projection_remains_opaque_and_other_compression_retains_alpha() {
        for dxgi in [None, Some(71)] {
            let data = fixture(4, 4, dxgi, &[(0, 65535, u32::MAX)]);
            let baseline = image::load_from_memory(&data).unwrap().into_rgba8();
            for policy in [
                TextureDecodePolicy::PreserveRgba,
                TextureDecodePolicy::PaletteIndexZero,
            ] {
                let actual = decode_rgba(&data, policy, "legacy", DecodeProfile::Legacy)
                    .unwrap()
                    .into_rgba8();
                assert_eq!(actual, baseline);
                assert!(actual.pixels().all(|p| p[3] == 255));
            }
        }
        for fourcc in [b"DXT3", b"DXT5"] {
            let mut data = fixture(4, 4, None, &[]);
            data[84..88].copy_from_slice(fourcc);
            // Nonzero fractional alpha followed by a BC1-shaped color block.
            data.extend_from_slice(if fourcc == b"DXT3" {
                &[0x77; 8]
            } else {
                &[77, 99, 0, 0, 0, 0, 0, 0]
            });
            data.extend_from_slice(&[0, 0, 255, 255, 255, 255, 255, 255]);
            let baseline = image::load_from_memory(&data).unwrap().into_rgba8();
            assert!(baseline.pixels().all(|p| p[3] > 0 && p[3] < 255));
            assert_eq!(
                static_image(&data, TextureDecodePolicy::PreserveRgba),
                baseline
            );
        }
    }

    #[test]
    fn bc1_invalid_shapes_metadata_and_truncation_still_refuse() {
        let good = fixture(4, 4, Some(71), &[(0, 65535, u32::MAX)]);
        for (offset, value) in [
            (112, 0x200),
            (112, 0x400),
            (112, 0x200000),
            (24, 2),
            (132, 4),
            (136, 4),
            (140, 2),
            (144, 2),
            (144, 3),
            (144, 4),
            (144, 8),
            (12, 0),
            (16, 16385),
            (4, 123),
            (76, 31),
        ] {
            let mut data = good.clone();
            word(&mut data, offset, value);
            assert!(
                decode_rgba(
                    &data,
                    TextureDecodePolicy::PreserveRgba,
                    "bad",
                    DecodeProfile::Static
                )
                .is_err(),
                "offset {offset}, value {value}"
            );
        }
        for length in [4, 80, 127, 128, 147, 148, 155] {
            assert!(
                decode_rgba(
                    &good[..length],
                    TextureDecodePolicy::PreserveRgba,
                    "bad",
                    DecodeProfile::Static
                )
                .is_err(),
                "length {length}"
            );
        }
    }
}
