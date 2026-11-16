//! Explicit offline WLD conversion; output is not published to the asset server.
use anyhow::{Context, Result};
use eqoxide_asset_server::{
    static_scene::{write_static_visual_with_material_policy, StaticMaterialPolicy},
    wld_visual::{load_wld_visual, WldVisualLimits},
};
use std::path::Path;
fn main() -> Result<()> {
    let mut a: Vec<_> = std::env::args_os().skip(1).collect();
    let inspect = a.first().is_some_and(|s| s == "--inspect");
    if inspect {
        a.remove(0);
    }
    anyhow::ensure!(if inspect{(1..=2).contains(&a.len())}else{(2..=3).contains(&a.len())},"usage: wld_pbr_zone MAIN.s3d OUTPUT.glb [OBJECTS.s3d], or wld_pbr_zone --inspect MAIN.s3d [OBJECTS.s3d]");
    let object = a.get(if inspect { 1 } else { 2 }).map(Path::new);
    let resolved = load_wld_visual(Path::new(&a[0]), object, &WldVisualLimits::default())
        .context("resolve WLD static visual")?;
    if inspect {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"conversion":resolved.report,"stage":"cpu-source-inspection","glb_written":false,"production_publication":false,"gameplay_ready":false})
            )?
        );
        return Ok(());
    }
    let policy = write_static_visual_with_material_policy(
        &resolved.scene,
        &StaticMaterialPolicy::default(),
        &resolved.report.bake_revision,
        Path::new(&a[1]),
    )
    .context("write common PBR visual")?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"conversion":resolved.report,"material_policy":policy,"output_role":"visual","production_publication":false,"gameplay_ready":false})
        )?
    );
    Ok(())
}
