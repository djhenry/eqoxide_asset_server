//! Explicit precision preparation for the current static base-color texture slots.
//!
//! Limits charge input RGBA storage (four bytes/pixel at PNG8, eight at PNG16).
//! Decoding can transiently retain native-channel and RGBA buffers; conversion also
//! retains RGBA16, RGBA8 and encoded output together. These logical quotas are not
//! a hard peak-memory or hostile-filesystem guarantee. Geometry is never copied.
use crate::{
    convert::{decode_texture_rgba, TextureDecodePolicy, TextureDecodeProfile},
    static_scene::{validate_png_stream, StaticAlphaMode, StaticScene, StaticTexture},
};
use anyhow::{ensure, Context, Result};
use image::ImageEncoder;
use serde::Serialize;
use std::io::{self, Write};

pub const PORTABLE_PNG8_POLICY: &str = "portable-png8-nearest-v1";
const NAME_SUFFIX: &str = ":png8-nearest-v1";

#[derive(Debug, Clone)]
pub struct PortableTextureLimits {
    pub max_resources: usize,
    pub max_single_encoded_bytes: usize,
    pub max_input_png_bytes: usize,
    pub max_output_png_bytes: usize,
    pub max_single_decoded_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_dimension: u32,
    /// Aggregate copied names, including the original/emitted report names.
    pub max_name_bytes: usize,
    pub max_report_bytes: usize,
}
impl Default for PortableTextureLimits {
    fn default() -> Self {
        Self {
            max_resources: 100_000,
            max_single_encoded_bytes: 64 << 20,
            max_input_png_bytes: 128 << 20,
            max_output_png_bytes: 128 << 20,
            max_single_decoded_bytes: 64 << 20,
            max_decoded_bytes: 256 << 20,
            max_dimension: 8192,
            max_name_bytes: 8 << 20,
            max_report_bytes: 16 << 20,
        }
    }
}
#[derive(Debug)]
pub struct PreparedPortableTextures {
    pub textures: Vec<StaticTexture>,
    pub report: PortableTextureReport,
}
#[derive(Debug, Serialize)]
pub struct PortableTextureReport {
    pub policy: &'static str,
    pub stage: &'static str,
    pub input_stage: &'static str,
    pub images: Vec<PortableTextureImageReport>,
}
#[derive(Debug, Serialize)]
pub struct PortableTextureImageReport {
    pub ordinal: usize,
    pub original_name: String,
    pub emitted_name: String,
    pub width: u32,
    pub height: u32,
    pub input_bit_depth: u8,
    pub output_bit_depth: u8,
    pub source_png_blake3: String,
    pub emitted_png_blake3: String,
    /// Absolute error in source 16-bit sample codes against emitted8 * 257.
    pub max_channel_error: [u16; 4],
    pub changed_samples: [u64; 4],
    pub nonzero_alpha_to_zero: u64,
}

fn charge(total: &mut usize, amount: usize, limit: usize, what: &str) -> Result<()> {
    let next = total
        .checked_add(amount)
        .with_context(|| format!("{what} size overflow"))?;
    ensure!(next <= limit, "{what} budget exceeded");
    *total = next;
    Ok(())
}
struct CappedWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for CappedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(buf.len())
            .is_none_or(|n| n > self.limit)
        {
            return Err(io::Error::other("portable encoded output budget exceeded"));
        }
        self.bytes
            .try_reserve_exact(buf.len())
            .map_err(io::Error::other)?;
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct CountWriter {
    count: usize,
    limit: usize,
}
impl Write for CountWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.count = self
            .count
            .checked_add(buf.len())
            .filter(|&n| n <= self.limit)
            .ok_or_else(|| io::Error::other("portable report budget exceeded"))?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn serialized_size(value: &impl Serialize, limit: usize) -> Result<usize> {
    let mut w = CountWriter { count: 0, limit };
    serde_json::to_writer(&mut w, value)?;
    Ok(w.count)
}

/// Borrow a source scene and return same-order prepared textures and diagnostics.
/// PNG8 names and encoded bytes are exact; PNG16 uses independent nearest integer
/// RGBA quantization, without transfer-function conversion or premultiplication.
/// Every material is checked, including unused MASK records. This validates the
/// texture preparation domain only; the common writer still validates the scene.
pub fn prepare_portable_textures(
    scene: &StaticScene,
    limits: &PortableTextureLimits,
) -> Result<PreparedPortableTextures> {
    let mut resources = scene.textures.len();
    charge(
        &mut resources,
        scene.materials.len(),
        limits.max_resources,
        "texture/material resources",
    )?;
    let mut masked = vec![false; scene.textures.len()];
    for (i, m) in scene.materials.iter().enumerate() {
        ensure!(
            m.base_color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "material {i} invalid RGBA factor"
        );
        if let StaticAlphaMode::Mask { cutoff } = m.alpha_mode {
            ensure!(
                cutoff.is_finite() && (0.0..=1.0).contains(&cutoff),
                "material {i} invalid alpha cutoff"
            );
        }
        if let Some(t) = m.texture_index {
            let mask = masked
                .get_mut(t)
                .with_context(|| format!("material {i} texture out of bounds"))?;
            *mask |= matches!(m.alpha_mode, StaticAlphaMode::Mask { .. });
        }
    }
    // Preflight every image and aggregate work before the first pixel allocation.
    let mut inputs = 0;
    let mut decoded = 0;
    let mut names = 0;
    for t in &scene.textures {
        ensure!(
            t.png_bytes.len() <= limits.max_single_encoded_bytes,
            "single encoded PNG budget exceeded"
        );
        charge(
            &mut inputs,
            t.png_bytes.len(),
            limits.max_input_png_bytes,
            "input PNG",
        )?;
        validate_png_stream(&t.png_bytes)?;
        let depth = t.png_bytes[24];
        ensure!(
            matches!(depth, 8 | 16),
            "portable PNG supports only 8/16-bit source samples"
        );
        let width = u32::from_be_bytes(t.png_bytes[16..20].try_into()?);
        let height = u32::from_be_bytes(t.png_bytes[20..24].try_into()?);
        ensure!(
            width > 0
                && height > 0
                && width <= limits.max_dimension
                && height <= limits.max_dimension,
            "portable PNG dimensions exceed limit"
        );
        let bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|n| n.checked_mul(if depth == 16 { 8 } else { 4 }))
            .context("decoded PNG size overflow")?;
        let bytes = usize::try_from(bytes).context("decoded PNG size overflow")?;
        ensure!(
            bytes <= limits.max_single_decoded_bytes,
            "single decoded PNG budget exceeded"
        );
        charge(&mut decoded, bytes, limits.max_decoded_bytes, "decoded PNG")?;
        let emitted = t
            .name
            .len()
            .checked_add(if depth == 16 { NAME_SUFFIX.len() } else { 0 })
            .context("name size overflow")?;
        charge(
            &mut names,
            t.name.len(),
            limits.max_name_bytes,
            "copied texture names",
        )?;
        charge(
            &mut names,
            emitted,
            limits.max_name_bytes,
            "copied texture names",
        )?;
        charge(
            &mut names,
            emitted,
            limits.max_name_bytes,
            "copied texture names",
        )?;
    }
    let mut textures = Vec::new();
    let mut report = PortableTextureReport {
        policy: PORTABLE_PNG8_POLICY,
        stage: "emitted-texture-preparation",
        input_stage: "source-import-png",
        images: Vec::new(),
    };
    let mut report_size = serialized_size(&report, limits.max_report_bytes)?;
    let mut output_size = 0;
    for (ordinal, t) in scene.textures.iter().enumerate() {
        let input_bit_depth = t.png_bytes[24];
        let image = decode_texture_rgba(
            &t.png_bytes,
            TextureDecodePolicy::PreserveRgba,
            &t.name,
            TextureDecodeProfile::Static,
        )?;
        let (width, height) = (image.width(), image.height());
        let mut max_channel_error = [0; 4];
        let mut changed_samples = [0; 4];
        let mut nonzero_alpha_to_zero = 0;
        let (name, png_bytes) = if input_bit_depth == 16 {
            let source = image.into_rgba16();
            let mut pixels = Vec::new();
            pixels.try_reserve_exact(source.as_raw().len())?;
            for pixel in source.pixels() {
                ensure!(
                    !masked[ordinal] || pixel[3] % 257 == 0,
                    "texture {ordinal} MASK alpha is not exactly representable at PNG8"
                );
                for (c, &v) in pixel.0.iter().enumerate() {
                    let q = ((u32::from(v) + 128) / 257) as u8;
                    let error = v.abs_diff(u16::from(q) * 257);
                    max_channel_error[c] = max_channel_error[c].max(error);
                    changed_samples[c] += u64::from(error != 0);
                    if c == 3 && v != 0 && q == 0 {
                        nonzero_alpha_to_zero += 1;
                    }
                    pixels.push(q);
                }
            }
            let remaining = limits
                .max_output_png_bytes
                .checked_sub(output_size)
                .context("output PNG budget exceeded")?;
            let mut writer = CappedWriter {
                bytes: Vec::new(),
                limit: remaining.min(limits.max_single_encoded_bytes),
            };
            image::codecs::png::PngEncoder::new(&mut writer).write_image(
                &pixels,
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )?;
            (format!("{}{NAME_SUFFIX}", t.name), writer.bytes)
        } else {
            ensure!(
                t.png_bytes.len() <= limits.max_output_png_bytes.saturating_sub(output_size),
                "output PNG budget exceeded"
            );
            (t.name.clone(), t.png_bytes.clone())
        };
        charge(
            &mut output_size,
            png_bytes.len(),
            limits.max_output_png_bytes,
            "output PNG",
        )?;
        let record = PortableTextureImageReport {
            ordinal,
            original_name: t.name.clone(),
            emitted_name: name.clone(),
            width,
            height,
            input_bit_depth,
            output_bit_depth: 8,
            source_png_blake3: blake3::hash(&t.png_bytes).to_hex().to_string(),
            emitted_png_blake3: blake3::hash(&png_bytes).to_hex().to_string(),
            max_channel_error,
            changed_samples,
            nonzero_alpha_to_zero,
        };
        let size = serialized_size(&record, limits.max_report_bytes)?;
        charge(
            &mut report_size,
            size,
            limits.max_report_bytes,
            "preparation report",
        )?;
        if ordinal != 0 {
            charge(
                &mut report_size,
                1,
                limits.max_report_bytes,
                "preparation report",
            )?;
        }
        report.images.push(record);
        textures.push(StaticTexture { name, png_bytes });
    }
    serialized_size(&report, limits.max_report_bytes)?;
    Ok(PreparedPortableTextures { textures, report })
}
