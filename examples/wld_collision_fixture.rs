//! Author synthetic selected WLD collision packages for CPU decoder inspection.
#[path = "../tests/common/wld_fixture.rs"]
mod fixture;
use anyhow::{Context, Result};
use eqoxide_asset_server::{
    cas::Cas, manifest::ManifestStore, static_collision::*, static_scene_descriptor::*,
    wld_collision::*,
};
use fixture::{document, MeshFixture};
use std::path::{Path, PathBuf};

fn package(output: &Path, empty: bool, disagree: bool) -> Result<()> {
    std::fs::create_dir_all(output)?;
    let mut source = MeshFixture::default();
    source.positions.push(source.positions[0]);
    source.faces = if empty {
        vec![(0, [0, 3, 2]), (0x10, [0, 1, 2])]
    } else {
        vec![
            (0, [0, 1, 2]),
            (0, [0, 3, 2]),
            (0x10, [0, 1, 2]),
            (0, [1, 2, 0]),
        ]
    };
    let doc = document(&[(0x9999, vec![]), (0x36, source.bytes(false))], false);
    let normalized = normalize_selected_wld_terrain(
        WldTerrainInput {
            document: &doc,
            source_context: "authored-wld-fixture",
            selections: &[WldTerrainSelection {
                fragment_slot: 1,
                owner_id: "authored-wld-owner",
            }],
        },
        &WldCollisionLimits::default(),
    )?;
    let mut identity = blake3::Hasher::new();
    identity.update(include_bytes!("wld_collision_fixture.rs"));
    identity.update(include_bytes!("../tests/common/wld_fixture.rs"));
    identity.update(NORMALIZATION_POLICY.as_bytes());
    identity.update(&[u8::from(empty)]);
    let bake = identity.finalize().to_hex().to_string();
    let requirements = write_static_collision(
        &normalized.scene,
        &bake,
        InspectionQueryPolicy::FrontFaces,
        &output.join("collision.glb"),
        &CollisionWriteLimits::default(),
    )?;
    let bytes = std::fs::read(output.join("collision.glb"))?;
    let limits = DescriptorLimits::default();
    let file = SceneFile::from_bytes("collision.glb", &bytes, &limits)?;
    let policy = if disagree {
        InspectionQueryPolicy::BothFaces
    } else {
        InspectionQueryPolicy::FrontFaces
    };
    let collision = if empty {
        CollisionComponent::AvailableEmpty {
            scope: "declared-static-triangle-candidates-v1".into(),
            file,
            requirements,
            query_policy: policy,
        }
    } else {
        CollisionComponent::Available {
            scope: "declared-static-triangle-candidates-v1".into(),
            file,
            requirements,
            query_policy: policy,
        }
    };
    let descriptor = SceneDescriptor::new(
        "authored",
        &bake,
        VisualComponent::NotExtracted {},
        collision,
    )?;
    let requirements = write_scene_descriptor(&descriptor, &output.join("scene.json"), &limits)?;
    let descriptor_bytes = std::fs::read(output.join("scene.json"))?;
    let manifest = ManifestStore::new(output).build_and_write(
        &Cas::new(output),
        &descriptor.set,
        &[
            ("scene.json".into(), descriptor_bytes.clone()),
            ("collision.glb".into(), bytes),
        ],
        requirements,
    )?;
    bind_scene_descriptor_to_manifest(&descriptor_bytes, &manifest, "authored", &limits)?;
    std::fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    std::fs::write(
        output.join("summary.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "manifest_revision":manifest.revision,"normalization_policy":normalized.report.normalization_policy,
            "source_faces":normalized.report.faces.source_faces,"source_flag_excluded":normalized.report.faces.source_flag_excluded,
            "coincident_source_positions_excluded":normalized.report.faces.coincident_source_positions_excluded,
            "retained_faces":normalized.report.faces.retained_faces,"known_empty":empty,
            "descriptor_query_policy_disagreement":disagree,"manifest_descriptor_binding_verified":true,
            "runtime_activation":false,"segment_queries":false,
        }))?,
    )?;
    Ok(())
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let output = PathBuf::from(
        args.next()
            .context("usage: wld_collision_fixture OUTPUT_DIRECTORY")?,
    );
    anyhow::ensure!(args.next().is_none(), "unexpected argument");
    package(&output.join("mixed"), false, false)?;
    package(&output.join("empty"), true, false)?;
    package(&output.join("disagreement"), false, true)?;
    Ok(())
}
