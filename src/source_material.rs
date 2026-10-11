//! Resolve declared source texture and material facts for the common static profile.
pub use crate::convert::AlphaMode;
use crate::static_scene::{StaticAlphaMode, StaticMaterial, StaticTexture};
use anyhow::Result;

pub use crate::convert::TextureDecodePolicy;

/// Declared source facts; this seam does not interpret shader names or geometry.
#[derive(Debug, Clone)]
pub struct SourceMaterial {
    pub name: String,
    pub base_color: [f32; 4],
    pub texture_index: Option<usize>,
    pub alpha_mode: AlphaMode,
    pub double_sided: bool,
    /// Presence requires a separate capability, including an empty frame list.
    pub texture_sequence: Option<(u32, Vec<String>)>,
}

/// Resolve supported alpha modes without modifying texture samples or RGB tint.
pub fn normalize_material(source: &SourceMaterial) -> Result<StaticMaterial> {
    anyhow::ensure!(
        source
            .base_color
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
        "material factor must be finite normalized RGBA"
    );
    anyhow::ensure!(
        source.texture_sequence.is_none(),
        "texture sequences require a separate capability"
    );
    let mut base_color = source.base_color;
    let alpha_mode = match source.alpha_mode {
        AlphaMode::Opaque => StaticAlphaMode::Opaque,
        AlphaMode::Masked => StaticAlphaMode::Mask { cutoff: 0.5 },
        AlphaMode::Cutout(cutoff) => StaticAlphaMode::Mask {
            cutoff: f32::from(cutoff) / 255.0,
        },
        AlphaMode::Blend(permille) => {
            anyhow::ensure!(
                permille <= 1000,
                "material opacity must be at most 1000 permille"
            );
            base_color[3] *= f32::from(permille) / 1000.0;
            StaticAlphaMode::Blend
        }
        AlphaMode::Additive => anyhow::bail!("additive blending requires a separate capability"),
    };
    Ok(StaticMaterial {
        name: source.name.clone(),
        base_color,
        texture_index: source.texture_index,
        alpha_mode,
        double_sided: source.double_sided,
    })
}

/// Decode bounded source samples to PNG, preserving RGBA and sample depth.
/// Material opacity belongs solely in the normalized material factor.
pub fn decode_static_texture(
    name: impl Into<String>,
    bytes: &[u8],
    policy: TextureDecodePolicy,
) -> Result<StaticTexture> {
    let name = name.into();
    let rgba = crate::convert::decode_texture_rgba(
        bytes,
        policy,
        &name,
        crate::convert::TextureDecodeProfile::Static,
    )?;
    let png_bytes = crate::convert::encode_texture_rgba_png(&rgba, &name)?;
    anyhow::ensure!(
        png_bytes.len() <= crate::static_scene::MAX_TEXTURE_BYTES,
        "texture {name} normalized PNG exceeds encoded byte limit"
    );
    Ok(StaticTexture { name, png_bytes })
}

#[cfg(test)]
mod tests;
