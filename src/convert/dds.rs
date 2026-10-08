//! Bounded support for legacy, uncompressed 32-bit DDS textures.
use anyhow::{Result, ensure};
use image::RgbaImage;

/// Decode legacy RGB32 DDS data; leave other encodings to the general decoder.
pub(super) fn decode(data: &[u8]) -> Result<Option<RgbaImage>> {
    if !data.starts_with(b"DDS ") {
        return Ok(None);
    }
    ensure!(data.len() >= 128, "truncated DDS header");
    // Every header read below is within the checked, fixed-size legacy header.
    let word = |offset| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
    ensure!(word(4) == 124 && word(76) == 32, "invalid DDS header size");
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
        assert!(
            decode(&data)
                .unwrap()
                .unwrap()
                .pixels()
                .all(|p| p[3] == 255)
        );
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
