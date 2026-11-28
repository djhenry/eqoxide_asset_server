//! Bounded RGB32 DDS support and static-only A4R4G4B4 decoding.
use anyhow::{ensure, Result};
use image::RgbaImage;

/// Static decoding adds only the exact A4R4G4B4 source format to RGB32 support.
pub(super) fn decode_static_with_limits(
    data: &[u8],
    limits: &image::Limits,
) -> Result<Option<RgbaImage>> {
    if !data.starts_with(b"DDS ") {
        return Ok(None);
    }
    ensure!(data.len() >= 128, "truncated DDS header");
    let word = |offset| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
    ensure!(word(4) == 124 && word(76) == 32, "invalid DDS header size");
    // RGBBitCount is unused by FourCC formats, even if its stored value is 16.
    if word(88) != 16 || word(80) & 4 != 0 || word(84) != 0 {
        return decode_with_limits(data, Some(limits));
    }
    validate_static_2d(data, word)?;
    ensure!(
        word(8) & 0x800000 == 0,
        "DDS depth declaration requires a separate representation"
    );
    ensure!(
        word(80) == 0x41
            && word(84) == 0
            && [word(92), word(96), word(100), word(104)] == [0xf00, 0xf0, 0xf, 0xf000],
        "unsupported A4R4G4B4 DDS pixel metadata"
    );
    ensure!(
        word(8) & 6 == 6,
        "A4R4G4B4 DDS requires width and height dimension flags"
    );
    let (width, height) = (word(16), word(12));
    // Apply axis limits without reserving the decoded allocation yet.
    let mut dimension_limits = limits.clone();
    dimension_limits.max_alloc = None;
    super::texture::check_dimensions(width, height, 4, Some(&dimension_limits))?;
    let count = word(28).max(1);
    let max_count = u32::BITS - width.max(height).leading_zeros();
    ensure!(count <= max_count, "invalid A4R4G4B4 DDS mip count");
    let row_bytes = usize::try_from(width)?
        .checked_mul(2)
        .ok_or_else(|| anyhow::anyhow!("DDS row size overflow"))?;
    let pitch = if word(8) & 8 != 0 {
        usize::try_from(word(20))?
    } else {
        row_bytes
    };
    ensure!(pitch >= row_bytes, "DDS pitch is smaller than a pixel row");
    ensure!(
        count == 1 || pitch == row_bytes,
        "padded A4R4G4B4 DDS mip chains are unsupported"
    );
    let base_size = pitch
        .checked_mul(usize::try_from(height)?)
        .ok_or_else(|| anyhow::anyhow!("DDS base payload size overflow"))?;
    let mut payload_size = base_size;
    let (mut mip_width, mut mip_height) = (width, height);
    for _ in 1..count {
        mip_width = (mip_width / 2).max(1);
        mip_height = (mip_height / 2).max(1);
        let size = usize::try_from(mip_width)?
            .checked_mul(usize::try_from(mip_height)?)
            .and_then(|n| n.checked_mul(2))
            .ok_or_else(|| anyhow::anyhow!("DDS mip payload size overflow"))?;
        payload_size = payload_size
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("DDS mip chain size overflow"))?;
    }
    let body = &data[128..];
    ensure!(
        body.len() == payload_size,
        "A4R4G4B4 DDS payload does not exactly match declared mip levels"
    );
    super::texture::check_dimensions(width, height, 4, Some(limits))?;
    let output_size = usize::try_from(width)?
        .checked_mul(usize::try_from(height)?)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| anyhow::anyhow!("DDS output size overflow"))?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(output_size)?;
    for row in body[..base_size].chunks_exact(pitch) {
        for bytes in row[..row_bytes].as_chunks::<2>().0 {
            let value = u16::from_le_bytes(*bytes);
            pixels.extend_from_slice(&[
                ((value >> 8) & 15) as u8 * 17,
                ((value >> 4) & 15) as u8 * 17,
                (value & 15) as u8 * 17,
                ((value >> 12) & 15) as u8 * 17,
            ]);
        }
    }
    Ok(Some(
        RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow::anyhow!("invalid DDS image dimensions"))?,
    ))
}

/// Decode legacy RGB32 DDS data; leave other encodings to the general decoder.
#[cfg(test)]
pub(super) fn decode(data: &[u8]) -> Result<Option<RgbaImage>> {
    decode_with_limits(data, None)
}
pub(super) fn decode_with_limits(
    data: &[u8],
    limits: Option<&image::Limits>,
) -> Result<Option<RgbaImage>> {
    if !data.starts_with(b"DDS ") {
        return Ok(None);
    }
    ensure!(data.len() >= 128, "truncated DDS header");
    // Every header read below is within the checked, fixed-size legacy header.
    let word = |offset| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
    ensure!(word(4) == 124 && word(76) == 32, "invalid DDS header size");
    if limits.is_some() {
        validate_static_2d(data, &word)?;
        super::texture::check_dimensions(word(16), word(12), 4, limits)?;
    }
    let pixel_flags = word(80);
    if pixel_flags & 0x40 == 0 || pixel_flags & 4 != 0 || word(84) != 0 || word(88) != 32 {
        return Ok(None);
    }
    // Cube faces and volume slices require a different image representation.
    if word(112) & (0xfe00 | 0x200000) != 0 || word(24) > 1 {
        return Ok(None);
    }
    let masks = [word(92), word(96), word(100), word(104)];
    let has_alpha = pixel_flags & 1 != 0;
    let mut channels = [0usize; 4];
    let mut used = 0u32;
    for index in 0..if has_alpha { 4 } else { 3 } {
        let mask = masks[index];
        ensure!(
            matches!(mask, 0xff | 0xff00 | 0xff0000 | 0xff000000),
            "invalid RGB32 DDS channel mask"
        );
        ensure!(used & mask == 0, "overlapping RGB32 DDS channel masks");
        used |= mask;
        channels[index] = (mask.trailing_zeros() / 8) as usize;
    }
    let width = word(16);
    let height = word(12);
    ensure!(width != 0 && height != 0, "empty DDS dimensions");
    let row_bytes = usize::try_from(width)?
        .checked_mul(4)
        .ok_or_else(|| anyhow::anyhow!("DDS row size overflow"))?;
    let rows = usize::try_from(height)?;
    let pitch = if word(8) & 8 != 0 {
        usize::try_from(word(20))?
    } else {
        row_bytes
    };
    ensure!(pitch >= row_bytes, "DDS pitch is smaller than a pixel row");
    let body_size = pitch
        .checked_mul(rows)
        .ok_or_else(|| anyhow::anyhow!("DDS pixel size overflow"))?;
    let body = data.get(128..).unwrap();
    ensure!(body.len() >= body_size, "truncated DDS pixels");
    let output_size = row_bytes
        .checked_mul(rows)
        .ok_or_else(|| anyhow::anyhow!("DDS output size overflow"))?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(output_size)?;
    for row in body[..body_size].chunks_exact(pitch) {
        for pixel in row[..row_bytes].as_chunks::<4>().0 {
            pixels.extend_from_slice(&[
                pixel[channels[0]],
                pixel[channels[1]],
                pixel[channels[2]],
                if has_alpha { pixel[channels[3]] } else { 255 },
            ]);
        }
    }
    Ok(Some(
        RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow::anyhow!("invalid DDS image dimensions"))?,
    ))
}

// A single straight-alpha 2D PNG cannot represent these declared DDS semantics.
fn validate_static_2d(data: &[u8], word: impl Fn(usize) -> u32) -> Result<()> {
    ensure!(
        word(112) & (0xfe00 | 0x200000) == 0 && word(24) <= 1,
        "DDS cube or volume requires a separate representation"
    );
    if word(84) == u32::from_le_bytes(*b"DX10") {
        let extension: &[u8; 20] = data
            .get(128..148)
            .ok_or_else(|| anyhow::anyhow!("truncated DDS DX10 header"))?
            .try_into()?;
        let extended =
            |offset| u32::from_le_bytes(extension[offset..offset + 4].try_into().unwrap());
        ensure!(
            extended(4) == 3 && extended(8) == 0 && extended(12) == 1,
            "DDS DX10 resource requires a separate representation"
        );
        ensure!(
            matches!(extended(16), 0 | 1),
            "DDS DX10 alpha convention requires a separate representation"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture() -> Vec<u8> {
        let mut data = vec![0; 128];
        data[..4].copy_from_slice(b"DDS ");
        for (offset, value) in [
            (4, 124),
            (8, 0x81007),
            (12, 2),
            (16, 2),
            (20, 16),
            (76, 32),
            (80, 0x41),
            (88, 32),
            (92, 0xff0000),
            (96, 0xff00),
            (100, 0xff),
            (104, 0xff000000),
            (108, 0x1000),
        ] {
            word(&mut data, offset, value);
        }
        data.extend_from_slice(&[3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]);
        data
    }

    #[test]
    fn static_packed_budget_boundaries_and_payload_preflight() {
        let mut data = fixture();
        for (offset, value) in [(88, 16), (92, 0xf00), (96, 0xf0), (100, 0xf), (104, 0xf000)] {
            word(&mut data, offset, value);
        }
        data.truncate(128);
        data.extend_from_slice(&[0x34, 0x12, 0x78, 0x56, 0xbc, 0x9a, 0xf0, 0xde]);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(2);
        limits.max_image_height = Some(2);
        for budget in [16, 17] {
            limits.max_alloc = Some(budget);
            assert!(decode_static_with_limits(&data, &limits).unwrap().is_some());
        }
        limits.max_alloc = Some(15);
        let error = decode_static_with_limits(&data, &limits).unwrap_err();
        assert!(error.to_string().contains("allocation"));
        let error = decode_static_with_limits(&data[..135], &limits).unwrap_err();
        assert!(error.to_string().contains("payload"));
        limits.max_image_width = Some(1);
        assert!(decode_static_with_limits(&data, &limits)
            .unwrap_err()
            .to_string()
            .contains("dimensions"));
        limits.max_image_width = Some(2);
        word(&mut data, 8, 0x100f);
        word(&mut data, 20, u32::MAX);
        assert!(decode_static_with_limits(&data, &limits)
            .unwrap_err()
            .to_string()
            .contains("payload"));
    }

    #[test]
    fn tight_bgra_ignores_linear_size_as_pitch_and_preserves_alpha_and_rows() {
        let result = decode(&fixture()).unwrap().unwrap();
        assert_eq!(result.dimensions(), (2, 2));
        assert_eq!(result.as_raw(), &(1..=16).collect::<Vec<_>>());
    }

    #[test]
    fn rgba_padded_pitch_and_xrgb() {
        let mut data = fixture();
        word(&mut data, 8, 0x100f);
        word(&mut data, 20, 12);
        word(&mut data, 92, 0xff);
        word(&mut data, 100, 0xff0000);
        data.splice(136..136, [99; 4]);
        data.extend_from_slice(&[99; 4]);
        let result = decode(&data).unwrap().unwrap();
        assert_eq!(
            result.as_raw(),
            &[3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]
        );
        word(&mut data, 80, 0x40);
        word(&mut data, 104, 0);
        assert!(decode(&data)
            .unwrap()
            .unwrap()
            .pixels()
            .all(|p| p[3] == 255));
    }

    #[test]
    fn malformed_supported_files_are_errors_without_panics() {
        for length in [4, 16, 80, 127, 128, 143] {
            assert!(decode(&fixture()[..length]).is_err(), "length {length}");
        }
        for (offset, value) in [
            (4, 123),
            (76, 31),
            (12, 0),
            (16, 0),
            (12, u32::MAX),
            (16, u32::MAX),
            (92, 0x1f),
            (96, 0xff0000),
            (104, 0xff),
        ] {
            let mut data = fixture();
            word(&mut data, offset, value);
            assert!(decode(&data).is_err(), "offset {offset}, value {value}");
        }
        let mut data = fixture();
        word(&mut data, 12, u32::MAX);
        word(&mut data, 16, u32::MAX);
        assert!(decode(&data).is_err());
        let mut data = fixture();
        word(&mut data, 8, 0x100f);
        word(&mut data, 20, 7);
        assert!(decode(&data).is_err());
    }

    #[test]
    fn other_formats_are_delegated() {
        assert!(decode(b"not a DDS").unwrap().is_none());
        assert!(decode(b"DD").unwrap().is_none());
        for (offset, value) in [
            (80, 4),
            (84, 0x31545844),
            (88, 24),
            (24, 2),
            (112, 0x200),
            (112, 0x200000),
        ] {
            let mut data = fixture();
            word(&mut data, offset, value);
            assert!(decode(&data).unwrap().is_none());
        }
    }
}
