use super::*;
use crate::static_scene::{StaticAlphaMode, StaticMaterial, StaticSurfaceModel};

// Coverage is an explicit exact-name policy. No shader prefix is interpreted.
const D: (&str, u32) = ("e_TextureDiffuse0", 2);
const N: (&str, u32) = ("e_TextureNormal0", 2);
const C: (&str, u32) = ("e_TextureCoverage0", 2);
const S: (&str, u32) = ("e_fShininess0", 0);
const CS: (&str, u32) = ("e_fCoverageScale0", 0);
pub(super) fn schema(shader: &str) -> Result<&'static [(&'static str, u32)]> {
    Ok(match shader {
        "Opaque_MPLFull2UV.fx" => &[C, D, N, S],
        "Opaque_MPLBump.fx" => &[C, D, N, CS],
        "Opaque_MPLBasic.fx" | "Opaque_MaxC1.fx" | "Chroma_MaxC1.fx" => &[D],
        "Opaque_MPLSB.fx" => &[C, D, N, CS, S],
        "Opaque_MaxCB1.fx" | "Chroma_MaxCB1.fx" | "Opaque_OpaqueSModelCB1Max.fx" => &[D, N],
        "Opaque_OpaqueSModelCBGG1Max.fx" => &[D, N, ("e_fBumpiness0", 0), S],
        "Opaque_MaxCBSGE1.fx" => &[
            D,
            ("e_TextureEnvironment0", 2),
            N,
            ("e_fEnvMapStrength0", 0),
            S,
        ],
        "Opaque_MaxCBSG1.fx" | "Opaque_MaxCBS1.fx" => &[D, N, S],
        "Opaque_MaxWater.fx" => &[
            D,
            N,
            ("e_fFresnelBias", 0),
            ("e_fFresnelPower", 0),
            ("e_fReflectionAmount", 0),
            ("e_fReflectionColor", 3),
            ("e_fSlide1X", 0),
            ("e_fSlide1Y", 0),
            ("e_fSlide2X", 0),
            ("e_fSlide2Y", 0),
            ("e_fWaterColor1", 3),
            ("e_fWaterColor2", 3),
        ],
        _ => bail!("unsupported required shader {shader:?}"),
    })
}
pub(super) fn validate(shader: &str, props: &[report::PropertyReport]) -> Result<()> {
    let schema = schema(shader)?;
    ensure!(
        props.len() == schema.len(),
        "mandatory property set cardinality mismatch for {shader}"
    );
    let mut seen = BTreeSet::new();
    for p in props {
        ensure!(seen.insert(p.name.as_str()), "duplicate material property");
        ensure!(
            schema.contains(&(p.name.as_str(), p.kind)),
            "unknown or wrong-kind material property {:?} for {shader}",
            p.name
        );
    }
    Ok(())
}
pub(super) fn role(name: &str) -> Result<TextureRole> {
    Ok(match name {
        "e_TextureDiffuse0" => TextureRole::Diffuse,
        "e_TextureNormal0" => TextureRole::Normal,
        "e_TextureCoverage0" => TextureRole::Coverage,
        "e_TextureEnvironment0" => TextureRole::Environment,
        _ => bail!("unsupported texture property"),
    })
}
pub(super) fn material(name: String, shader: &str, texture_index: usize) -> StaticMaterial {
    let water = shader == "Opaque_MaxWater.fx";
    let alpha_mode = match shader {
        "Opaque_MaxWater.fx" => StaticAlphaMode::Blend,
        "Chroma_MaxC1.fx" | "Chroma_MaxCB1.fx" => StaticAlphaMode::Mask {
            cutoff: 192.0 / 255.0,
        },
        _ => StaticAlphaMode::Opaque,
    };
    StaticMaterial {
        name,
        base_color: [1., 1., 1., if water { 0.55 } else { 1. }],
        texture_index: Some(texture_index),
        alpha_mode,
        double_sided: water,
    }
}
pub(super) fn surface(shader: &str) -> StaticSurfaceModel {
    StaticSurfaceModel::Pbr {
        metallic: 0.,
        roughness: if shader == "Opaque_MaxWater.fx" {
            0.2
        } else {
            1.
        },
    }
}
