//! Explicit offline WLD conversion; output is not published to the asset server.
use anyhow::{Context, Result};
use eqoxide_asset_server::source_material::portable::{
    prepare_portable_textures, PortableTextureLimits, PORTABLE_PNG8_POLICY,
};
use eqoxide_asset_server::static_scene::StaticTexture;
use eqoxide_asset_server::{
    static_scene::{
        write_static_visual_with_surface_profile, ResolvedStaticMaterialPolicy,
        StaticMaterialPolicySource, StaticMaterialProfile,
    },
    wld_visual::{load_wld_visual_with_mode, WldVisualLimits, WldVisualMode},
};
use std::path::Path;
fn main() -> Result<()> {
    run(std::env::args_os().skip(1))
}
pub(crate) fn run(args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let mut inspect = false;
    let mut portable_png8 = false;
    let mut mode = WldVisualMode::StrictStatic;
    let mut a = Vec::new();
    for arg in args {
        if arg == "--inspect" {
            anyhow::ensure!(!inspect, "duplicate --inspect");
            inspect = true;
        } else if arg == "--portable-png8" {
            anyhow::ensure!(!portable_png8, "duplicate --portable-png8");
            portable_png8 = true;
        } else if arg == "--preview-snapshot" {
            anyhow::ensure!(
                mode == WldVisualMode::StrictStatic,
                "duplicate --preview-snapshot"
            );
            mode = WldVisualMode::PreviewSnapshotV1;
        } else {
            anyhow::ensure!(!arg.to_string_lossy().starts_with("--"), "unknown option");
            a.push(arg);
        }
    }
    anyhow::ensure!(if inspect {(1..=2).contains(&a.len())} else {(2..=3).contains(&a.len())},
        "usage: wld_pbr_zone [--preview-snapshot] [--portable-png8] MAIN.s3d OUTPUT.glb [OBJECTS.s3d], or wld_pbr_zone [--preview-snapshot] [--portable-png8] --inspect MAIN.s3d [OBJECTS.s3d]");
    let object = a.get(if inspect { 1 } else { 2 }).map(Path::new);
    if !inspect {
        reject_source_output_alias(Path::new(&a[1]), Path::new(&a[0]), object)?;
    }
    let mut resolved =
        load_wld_visual_with_mode(Path::new(&a[0]), object, &WldVisualLimits::default(), mode)
            .context("resolve WLD static visual")?;
    let policy = &resolved.recommended_material_policy;
    let actual: Vec<_> = resolved
        .scene
        .materials
        .iter()
        .enumerate()
        .map(|(material_index, _)| {
            let (surface, source) = match policy.overrides.get(&material_index) {
                Some(&s) => (s, StaticMaterialPolicySource::Override),
                None => (policy.default, StaticMaterialPolicySource::Default),
            };
            ResolvedStaticMaterialPolicy {
                material_index,
                surface,
                source,
            }
        })
        .collect();
    let limits = PortableTextureLimits::default();
    let prepared = if portable_png8 {
        Some(
            prepare_portable_textures(&resolved.scene, &limits)
                .context("prepare portable PNG8 textures")?,
        )
    } else {
        None
    };
    // Serialize the new bounded diagnostics before committing any GLB output.
    let preparation_report = prepared
        .as_ref()
        .map(|p| serde_json::to_string(&p.report))
        .transpose()?;
    if let Some(report) = &preparation_report {
        anyhow::ensure!(
            report.len() <= limits.max_report_bytes,
            "portable report budget exceeded"
        );
    }
    let preparation_value = preparation_report
        .as_ref()
        .map(|s| serde_json::from_str::<serde_json::Value>(s))
        .transpose()?;
    let precision_policy = if portable_png8 {
        PORTABLE_PNG8_POLICY
    } else {
        "preserve-png-depth-v1"
    };
    if inspect {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "conversion":resolved.report,"conversion_stage":"source-import","texture_preparation":preparation_value,"texture_precision_policy":precision_policy,"recommended_material_choices":actual,
                "stage":if portable_png8 {"cpu-portable-texture-inspection"} else {"cpu-source-inspection"},"glb_written":false,"production_publication":false,"gameplay_ready":false
            }))?
        );
        return Ok(());
    }
    let (profile, profile_name) = if mode == WldVisualMode::PreviewSnapshotV1 {
        (
            StaticMaterialProfile::ScalarPbrHdrBlendV1,
            "scalar-pbr-hdr-blend-v1",
        )
    } else {
        (StaticMaterialProfile::ScalarPbrV1, "scalar-pbr-v1")
    };
    let revision = final_revision(
        &resolved.report.conversion_revision,
        profile_name,
        &serde_json::to_value(&actual)?,
        precision_policy,
        &resolved.scene.textures,
        prepared
            .as_ref()
            .map_or(&resolved.scene.textures, |p| &p.textures),
    )?;
    if let Some(prepared) = prepared {
        resolved.scene.textures = prepared.textures;
    }
    let written = write_static_visual_with_surface_profile(
        &resolved.scene,
        policy,
        profile,
        &revision,
        Path::new(&a[1]),
    )
    .context("write common PBR visual")?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "conversion":resolved.report,"conversion_stage":"source-import","texture_preparation":preparation_value,"texture_precision_policy":precision_policy,"material_policy":written,"glb_bake_revision":revision,
            "writer_profile":profile_name,"output_role":"visual","production_publication":false,"gameplay_ready":false
        }))?
    );
    Ok(())
}
/// Refuse aliases before parsing sources. This assumes a stable trusted local
/// filesystem; it does not protect against concurrent path substitution.
fn reject_source_output_alias(output: &Path, main: &Path, object: Option<&Path>) -> Result<()> {
    let output_metadata = match std::fs::metadata(output) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context("inspect output path"),
    };
    let Some(output_metadata) = output_metadata else {
        return Ok(());
    };
    let output_path = output.canonicalize().context("resolve output path")?;
    for source in std::iter::once(main).chain(object) {
        let source_path = source
            .canonicalize()
            .context("resolve source archive path")?;
        let mut same = output_path == source_path;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let source_metadata = std::fs::metadata(source).context("inspect source archive")?;
            same |= source_metadata.dev() == output_metadata.dev()
                && source_metadata.ino() == output_metadata.ino();
        }
        #[cfg(not(unix))]
        let _ = &output_metadata;
        anyhow::ensure!(!same, "output aliases a supplied source archive");
    }
    Ok(())
}
fn final_revision(
    conversion: &str,
    profile: &str,
    policy: &serde_json::Value,
    precision: &str,
    source: &[StaticTexture],
    emitted: &[StaticTexture],
) -> Result<String> {
    let mut h = blake3::Hasher::new();
    h.update(b"wld-standard-glb-bake-v2");
    h.update(conversion.as_bytes());
    h.update(profile.as_bytes());
    h.update(&serde_json::to_vec(policy)?);
    h.update(precision.as_bytes());
    for images in [source, emitted] {
        h.update(&(images.len() as u64).to_le_bytes());
        for image in images {
            h.update(&(image.name.len() as u64).to_le_bytes());
            h.update(image.name.as_bytes());
            h.update(blake3::hash(&image.png_bytes).as_bytes());
        }
    }
    h.update(blake3::hash(include_bytes!("../src/source_material/portable.rs")).as_bytes());
    h.update(blake3::hash(include_bytes!("../src/convert/texture.rs")).as_bytes());
    h.update(blake3::hash(include_bytes!("wld_pbr_zone.rs")).as_bytes());
    h.update(blake3::hash(include_bytes!("../src/static_scene.rs")).as_bytes());
    Ok(h.finalize().to_hex().to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glb_identity_binds_actual_surface_choices_and_writer_profile() {
        let source = "0".repeat(64);
        let pbr = serde_json::json!([{"material_index":0,"surface":{"Pbr":{"metallic":0.,"roughness":1.}},"source":"Default"}]);
        let unlit = serde_json::json!([{"material_index":0,"surface":"Unlit","source":"Override"}]);
        let a = final_revision(
            &source,
            "scalar-pbr-v1",
            &pbr,
            "preserve-png-depth-v1",
            &[],
            &[],
        )
        .unwrap();
        assert_ne!(
            a,
            final_revision(
                &source,
                "scalar-pbr-hdr-blend-v1",
                &pbr,
                "preserve-png-depth-v1",
                &[],
                &[]
            )
            .unwrap()
        );
        assert_ne!(
            a,
            final_revision(
                &source,
                "scalar-pbr-v1",
                &unlit,
                "preserve-png-depth-v1",
                &[],
                &[]
            )
            .unwrap()
        );
        assert_ne!(a, source);
    }
}
