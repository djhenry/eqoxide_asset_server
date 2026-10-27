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

const SOURCE_FRAME_PROFILE: &str = "wld-selected-passed-record-query-frames-v1";
fn package(
    output: &Path,
    empty: bool,
    disagree: bool,
    query_unit_frames: QueryUnitFrames,
) -> Result<()> {
    std::fs::create_dir_all(output)?;
    let mut source = MeshFixture {
        exponent: 2,
        ..Default::default()
    };
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
    identity.update(SOURCE_FRAME_PROFILE.as_bytes());
    identity.update(query_unit_frames.identifier().as_bytes());
    identity.update(&[u8::from(empty), u8::from(disagree)]);
    let bake = identity.finalize().to_hex().to_string();
    let requirements = write_static_collision_with_query_units(
        &normalized.scene,
        &bake,
        InspectionQueryPolicy::FrontFaces,
        query_unit_frames,
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
            "query_unit_frames":query_unit_frames.identifier(),"source_frame_profile":SOURCE_FRAME_PROFILE,
        }))?,
    )?;
    Ok(())
}
fn main() -> Result<()> {
    run(std::env::args_os().skip(1))
}
fn run(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let output = PathBuf::from(args.next().context(
        "usage: wld_collision_fixture OUTPUT_DIRECTORY [--declare-preserved-source-query-frames]",
    )?);
    anyhow::ensure!(
        !output.as_os_str().to_string_lossy().starts_with("--"),
        "output directory must precede the optional flag"
    );
    let query_unit_frames = match args.next() {
        None => QueryUnitFrames::Unspecified,
        Some(flag) if flag == "--declare-preserved-source-query-frames" => {
            QueryUnitFrames::DeclaredSourceQueryFramesPreserved
        }
        Some(_) => anyhow::bail!("unknown fixture option"),
    };
    anyhow::ensure!(args.next().is_none(), "unexpected fixture argument");
    package(&output.join("mixed"), false, false, query_unit_frames)?;
    package(&output.join("empty"), true, false, query_unit_frames)?;
    package(&output.join("disagreement"), false, true, query_unit_frames)?;
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    const FILES: &[&str] = &[
        "mixed/collision.glb",
        "empty/collision.glb",
        "disagreement/collision.glb",
    ];
    fn json(bytes: &[u8]) -> serde_json::Value {
        let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        serde_json::from_slice(&bytes[20..20 + length]).unwrap()
    }
    #[test]
    fn strict_arguments_refuse_before_writing() {
        let root = tempfile::tempdir().unwrap();
        assert!(run(Vec::<std::ffi::OsString>::new().into_iter()).is_err());
        let output = root.path().join("must-not-exist");
        for tail in [
            vec!["--canonical-query-units"], // Retired explicit flag is not an alias.
            vec!["--unknown"],
            vec![
                "--declare-preserved-source-query-frames",
                "--declare-preserved-source-query-frames",
            ],
            vec!["--declare-preserved-source-query-frames", "trailing"],
            vec!["trailing"],
        ] {
            let args = std::iter::once(output.as_os_str().to_owned())
                .chain(tail.into_iter().map(Into::into));
            assert!(run(args).is_err());
            assert!(!output.exists());
        }
        assert!(run([
            std::ffi::OsString::from("--declare-preserved-source-query-frames"),
            output.into_os_string()
        ]
        .into_iter())
        .is_err());
    }
    #[test]
    fn default_and_preserved_source_identity_capabilities_and_geometry_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        let ordinary = root.path().join("ordinary");
        let preserved = root.path().join("preserved");
        run([ordinary.as_os_str().to_owned()].into_iter()).unwrap();
        run([
            preserved.as_os_str().to_owned(),
            "--declare-preserved-source-query-frames".into(),
        ]
        .into_iter())
        .unwrap();
        for name in FILES {
            let before = std::fs::read(ordinary.join(name)).unwrap();
            let declared = std::fs::read(preserved.join(name)).unwrap();
            let a = json(&before);
            let b = json(&declared);
            assert_eq!(
                a["extras"]["eqoxideAsset"]["requirements"]["capabilities"],
                serde_json::json!(["static-collision-triangles-v1"])
            );
            assert_eq!(
                b["extras"]["eqoxideAsset"]["requirements"]["capabilities"],
                serde_json::json!([
                    "eqoxide-source-query-frames-preserved-v1",
                    "static-collision-triangles-v1"
                ])
            );
            assert_ne!(
                a["extras"]["eqoxideAsset"]["bakeRevision"],
                b["extras"]["eqoxideAsset"]["bakeRevision"]
            );
            assert_eq!(
                a["extras"]["eqoxideCollision"],
                b["extras"]["eqoxideCollision"]
            );
            for key in ["meshes", "nodes", "accessors", "bufferViews", "buffers"] {
                assert_eq!(a[key], b[key]);
            }
            let alen = u32::from_le_bytes(before[12..16].try_into().unwrap()) as usize;
            let blen = u32::from_le_bytes(declared[12..16].try_into().unwrap()) as usize;
            assert_eq!(&before[20 + alen..], &declared[20 + blen..]);
            if name.contains("empty") {
                assert_eq!(declared.len(), 20 + blen);
                assert!(b.get("nodes").is_none());
            }
        }
        let repeated = std::fs::read(preserved.join(FILES[0])).unwrap();
        run([
            preserved.as_os_str().to_owned(),
            "--declare-preserved-source-query-frames".into(),
        ]
        .into_iter())
        .unwrap();
        assert_eq!(std::fs::read(preserved.join(FILES[0])).unwrap(), repeated);
    }
}
