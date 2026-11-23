//! Bounded static PNG container and compressed scanline admission.
use crate::static_scene::{MAX_TEXTURE_BYTES, MAX_TEXTURE_DIMENSION};
use anyhow::{ensure, Context, Result};
use flate2::{Decompress, FlushDecompress, Status};

struct InflatedScanlines {
    decoder: Decompress,
    expected: u64,
    budget: u64,
    finished: bool,
}
impl InflatedScanlines {
    fn new(expected: u64) -> Result<Self> {
        Ok(Self {
            decoder: Decompress::new(true),
            expected,
            budget: expected
                .checked_add(1)
                .context("PNG scanline budget overflow")?,
            finished: false,
        })
    }
    fn consume(&mut self, mut input: &[u8]) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        let mut output = [0u8; 8192];
        loop {
            let capacity =
                usize::try_from((self.budget - self.decoder.total_out()).min(output.len() as u64))?;
            ensure!(capacity > 0, "PNG has excess inflated image data");
            let before_in = self.decoder.total_in();
            let before_out = self.decoder.total_out();
            let status = self
                .decoder
                .decompress(input, &mut output[..capacity], FlushDecompress::None)
                .context("invalid PNG zlib stream")?;
            let consumed = usize::try_from(self.decoder.total_in() - before_in)?;
            let produced = self.decoder.total_out() - before_out;
            ensure!(
                self.decoder.total_out() <= self.expected,
                "PNG has excess inflated image data"
            );
            input = input
                .get(consumed..)
                .context("PNG inflater input accounting")?;
            if status == Status::StreamEnd {
                self.finished = true;
                return Ok(());
            }
            if consumed == 0 && produced == 0 {
                ensure!(input.is_empty(), "PNG zlib stream made no progress");
                return Ok(());
            }
        }
    }
    fn finish(&self) -> Result<()> {
        ensure!(self.finished, "PNG zlib stream is incomplete");
        ensure!(
            self.decoder.total_out() == self.expected,
            "PNG inflated scanline length differs from IHDR"
        );
        Ok(())
    }
}

fn expected_scanlines(header: &[u8]) -> Result<u64> {
    ensure!(header.len() == 13, "invalid PNG IHDR length");
    let width = u32::from_be_bytes(header[..4].try_into()?);
    let height = u32::from_be_bytes(header[4..8].try_into()?);
    ensure!(
        width > 0
            && height > 0
            && width <= MAX_TEXTURE_DIMENSION
            && height <= MAX_TEXTURE_DIMENSION,
        "PNG dimensions exceed static limit"
    );
    let depth = header[8];
    let samples = match header[9] {
        0 => {
            ensure!(
                [1, 2, 4, 8, 16].contains(&depth),
                "invalid grayscale PNG depth"
            );
            1u64
        }
        2 => {
            ensure!([8, 16].contains(&depth), "invalid RGB PNG depth");
            3
        }
        3 => {
            ensure!([1, 2, 4, 8].contains(&depth), "invalid indexed PNG depth");
            1
        }
        4 => {
            ensure!(
                [8, 16].contains(&depth),
                "invalid grayscale-alpha PNG depth"
            );
            2
        }
        6 => {
            ensure!([8, 16].contains(&depth), "invalid RGBA PNG depth");
            4
        }
        _ => anyhow::bail!("invalid PNG color type"),
    };
    ensure!(
        header[10] == 0 && header[11] == 0 && header[12] <= 1,
        "unsupported PNG compression/filter/interlace method"
    );
    let noninterlaced = [(0u32, 0u32, 1u32, 1u32)];
    let adam7 = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    let passes = if header[12] == 0 {
        &noninterlaced[..]
    } else {
        &adam7[..]
    };
    let (mut pixels, mut rows) = (0u64, 0u64);
    for &(x, y, dx, dy) in passes {
        let pass_width = width.saturating_sub(x).div_ceil(dx);
        let pass_height = height.saturating_sub(y).div_ceil(dy);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        let bits = u64::from(pass_width)
            .checked_mul(samples)
            .and_then(|v| v.checked_mul(u64::from(depth)))
            .context("PNG row size overflow")?;
        let row_bytes = bits.checked_add(7).context("PNG row rounding overflow")? / 8;
        pixels = row_bytes
            .checked_mul(u64::from(pass_height))
            .and_then(|v| pixels.checked_add(v))
            .context("PNG scanline size overflow")?;
        rows = rows
            .checked_add(u64::from(pass_height))
            .context("PNG row count overflow")?;
    }
    // Packed native samples are a lower bound on the caller's decoded allocation.
    // Keep grayscale/RGB native admission; callers separately apply RGBA/palette limits.
    ensure!(
        pixels <= MAX_TEXTURE_BYTES as u64,
        "PNG native scanline bytes exceed static limit"
    );
    pixels
        .checked_add(rows)
        .context("PNG filtered scanline size overflow")
}

pub(crate) fn validate(png: &[u8]) -> Result<()> {
    ensure!(
        png.len() <= MAX_TEXTURE_BYTES,
        "PNG encoded bytes exceed static limit"
    );
    ensure!(
        png.starts_with(b"\x89PNG\r\n\x1a\n"),
        "invalid PNG signature"
    );
    let mut offset = 8usize;
    let mut chunks = 0usize;
    let mut ended = false;
    let mut saw_idat = false;
    let mut idat_closed = false;
    let mut inflater = None;
    while offset < png.len() {
        chunks = chunks.checked_add(1).context("PNG chunk count overflow")?;
        ensure!(chunks <= 100_000, "excessive PNG chunk count");
        let header = offset.checked_add(8).context("PNG chunk offset overflow")?;
        let bytes = png
            .get(offset..header)
            .context("truncated PNG chunk header")?;
        let len = u32::from_be_bytes(bytes[..4].try_into()?) as usize;
        let data_end = header.checked_add(len).context("PNG chunk size overflow")?;
        let end = data_end.checked_add(4).context("PNG CRC offset overflow")?;
        ensure!(end <= png.len(), "truncated PNG chunk payload");
        let kind = &bytes[4..];
        ensure!(
            kind.iter().all(u8::is_ascii_alphabetic),
            "invalid PNG chunk type"
        );
        ensure!(
            kind != b"acTL" && kind != b"fcTL" && kind != b"fdAT",
            "animated PNG requires a separate capability"
        );
        let crc = u32::from_be_bytes(png[data_end..end].try_into()?);
        ensure!(
            crc32fast::hash(&png[offset + 4..data_end]) == crc,
            "invalid PNG chunk CRC"
        );
        if chunks == 1 {
            ensure!(kind == b"IHDR" && len == 13, "PNG must start with IHDR");
            inflater = Some(InflatedScanlines::new(expected_scanlines(
                &png[header..data_end],
            )?)?);
        } else {
            ensure!(kind != b"IHDR", "duplicate PNG IHDR");
        }
        if kind == b"IDAT" {
            ensure!(!idat_closed, "PNG IDAT chunks must be consecutive");
            saw_idat = true;
            inflater
                .as_mut()
                .context("PNG IHDR missing")?
                .consume(&png[header..data_end])?;
        } else if saw_idat {
            idat_closed = true;
        }
        if kind == b"IEND" {
            ensure!(
                len == 0 && end == png.len(),
                "invalid PNG end or trailing data"
            );
            ensure!(saw_idat, "PNG image data missing");
            inflater.as_ref().context("PNG IHDR missing")?.finish()?;
            ended = true;
        }
        offset = end;
    }
    ensure!(ended, "PNG missing IEND");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(dead_code)]
    mod fixture {
        include!("../tests/support/png_stream.rs");
    }
    fn header(width: u32, height: u32, depth: u8, color: u8, interlace: u8) -> Vec<u8> {
        let mut bytes = width.to_be_bytes().to_vec();
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[depth, color, 0, 0, interlace]);
        bytes
    }
    #[test]
    fn checked_layout_preserves_native_gray_and_rgb16_boundaries() {
        assert_eq!(
            expected_scanlines(&header(8192, 8192, 8, 0, 0)).unwrap(),
            67_117_056
        );
        assert_eq!(
            expected_scanlines(&header(8192, 1365, 16, 2, 0)).unwrap(),
            67_093_845
        );
        assert!(expected_scanlines(&header(8192, 1366, 16, 2, 0)).is_err());
        assert!(expected_scanlines(&header(u32::MAX, u32::MAX, 16, 6, 1)).is_err());
        assert!(expected_scanlines(&header(0, 1, 8, 6, 0)).is_err());
    }
    #[test]
    fn split_idat_and_legal_compressed_padding_are_accepted() {
        let mut compressed = fixture::stored_zlib(&[0; 18]);
        compressed.extend_from_slice(&[0; 4]);
        for split in 0..=compressed.len() {
            let png = fixture::png_from_zlib(
                2,
                2,
                8,
                6,
                0,
                &[&compressed[..split], &[], &compressed[split..]],
            );
            validate(&png).unwrap();
        }
    }
    #[test]
    fn extra_inflated_bytes_checksum_and_truncation_refuse() {
        let mut raw = vec![0; 18];
        raw.extend_from_slice(&[0; 4096]);
        let mut bounded = InflatedScanlines::new(18).unwrap();
        assert!(bounded.consume(&fixture::stored_zlib(&raw)).is_err());
        assert_eq!(bounded.decoder.total_out(), 19);
        assert!(validate(&fixture::png(2, 2, 8, 6, 0, &raw, &[])).is_err());
        let mut compressed = fixture::stored_zlib(&[0; 18]);
        let n = compressed.len();
        compressed[n - 1] ^= 1;
        assert!(validate(&fixture::png_from_zlib(2, 2, 8, 6, 0, &[&compressed])).is_err());
        let compressed = fixture::stored_zlib(&[0; 18]);
        assert!(validate(&fixture::png_from_zlib(
            2,
            2,
            8,
            6,
            0,
            &[&compressed[..compressed.len() - 1]]
        ))
        .is_err());
    }
    #[test]
    fn adam7_lowbit_and_sixteen_bit_scanline_counts_remain_valid() {
        for (w, h, d, c, i, n) in [
            (2, 2, 8, 6, 1, 19),
            (3, 5, 16, 2, 1, 100),
            (2, 2, 1, 0, 0, 4),
            (1, 17, 1, 0, 1, 34),
            (2, 2, 1, 3, 0, 4),
        ] {
            validate(&fixture::png(w, h, d, c, i, &vec![0; n], &[])).unwrap();
        }
    }
}
