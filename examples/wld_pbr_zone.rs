//! Explicit offline WLD conversion; output is not published to the asset server.
use anyhow::{Context, Result};
use eqoxide_asset_server::{
    static_scene::{
        write_static_visual_with_surface_profile, ResolvedStaticMaterialPolicy,
        StaticMaterialPolicySource, StaticMaterialProfile,
    },
    wld_visual::{load_wld_visual_with_mode, WldVisualLimits, WldVisualMode},
};
use std::path::Path;
fn main() -> Result<()> {
    let mut inspect = false;
    let mut mode = WldVisualMode::StrictStatic;
    let mut a = Vec::new();
    for arg in std::env::args_os().skip(1) {
        if arg == "--inspect" {
            anyhow::ensure!(!inspect, "duplicate --inspect");
            inspect = true;
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
        "usage: wld_pbr_zone [--preview-snapshot] MAIN.s3d OUTPUT.glb [OBJECTS.s3d], or wld_pbr_zone [--preview-snapshot] --inspect MAIN.s3d [OBJECTS.s3d]");
    let object = a.get(if inspect { 1 } else { 2 }).map(Path::new);
    let resolved =
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
    if inspect {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "conversion":resolved.report,"recommended_material_choices":actual,
                "stage":"cpu-source-inspection","glb_written":false,"production_publication":false,"gameplay_ready":false
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
    )?;
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
        serde_json::to_string_pretty(&serde_json::json!({
            "conversion":resolved.report,"material_policy":written,"glb_bake_revision":revision,
            "writer_profile":profile_name,"output_role":"visual","production_publication":false,"gameplay_ready":false
        }))?
    );
    Ok(())
}
fn final_revision(conversion: &str, profile: &str, policy: &serde_json::Value) -> Result<String> {
    let mut h = blake3::Hasher::new();
    h.update(b"wld-standard-glb-bake-v1");
    h.update(conversion.as_bytes());
    h.update(profile.as_bytes());
    h.update(&serde_json::to_vec(policy)?);
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
        let a = final_revision(&source, "scalar-pbr-v1", &pbr).unwrap();
        assert_ne!(
            a,
            final_revision(&source, "scalar-pbr-hdr-blend-v1", &pbr).unwrap()
        );
        assert_ne!(a, final_revision(&source, "scalar-pbr-v1", &unlit).unwrap());
        assert_ne!(a, source);
    }
}
