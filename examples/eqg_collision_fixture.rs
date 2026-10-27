//! Authored raw binary-zone records through the common collision adapter and writer.
use anyhow::{ensure, Context, Result};
use eqoxide_asset_server::{
    eqg::{static_collision::*, BinaryZoneScene, ZoneMesh, ZonePlacement},
    static_collision::{
        write_static_collision_with_query_units, CollisionWriteLimits, InspectionQueryPolicy,
        QueryUnitFrames,
    },
};
use libeq_eqg::mesh::{MeshKind, Triangle, Vertex};
use std::path::PathBuf;
fn mesh(kind: MeshKind) -> ZoneMesh {
    ZoneMesh {
        source_name: "authored".into(),
        kind,
        version: 1,
        string_table: vec![],
        materials: vec![],
        vertices: [[1., 2., 3.], [4., 2., 4.], [1., 6., 5.]]
            .map(|position| Vertex {
                position,
                normal: [0.; 3],
                color: None,
                uv0: [0.; 2],
                uv1: None,
            })
            .into(),
        triangles: vec![Triangle {
            vertex_indices: [0, 1, 2],
            material_index: u32::MAX,
            flags: 2,
        }],
        bone_count: if kind == MeshKind::Model {
            Some(0)
        } else {
            None
        },
        uv_marker: None,
        trailing_data: vec![],
    }
}
fn source(empty: bool) -> BinaryZoneScene {
    let mut s = BinaryZoneScene {
        archive_path: "unused".into(),
        descriptor_name: "authored".into(),
        descriptor_location: "synthetic",
        version: 1,
        string_table: b"m\0p\0".to_vec(),
        model_name_offsets: vec![Some(0), Some(0), Some(0)],
        model_slots: vec![Some(1), Some(0), Some(1)],
        meshes: vec![mesh(MeshKind::Terrain), mesh(MeshKind::Model)],
        placements: vec![
            ZonePlacement {
                model_index: Some(2),
                name_offset: 2,
                position: [10., -20., -5.],
                rotation: [0.; 3],
                scale: 2.,
                extension_data: vec![],
            },
            ZonePlacement {
                model_index: Some(0),
                name_offset: 2,
                position: [0., 10., 1.],
                rotation: [0., 0., 0.],
                scale: 1.,
                extension_data: vec![],
            },
        ],
        regions: vec![],
        lights: vec![],
        trailing_data: vec![],
    };
    if empty {
        for m in &mut s.meshes {
            m.triangles[0].flags = 1;
        }
    }
    s
}
const SOURCE_FRAME_PROFILE: &str = "eqg-passed-record-query-frames-v1";
fn main() -> Result<()> {
    run(std::env::args_os().skip(1))
}
fn run(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let out = PathBuf::from(args.next().context(
        "usage: eqg_collision_fixture OUTPUT_DIRECTORY [--declare-preserved-source-query-frames]",
    )?);
    ensure!(
        !out.as_os_str().to_string_lossy().starts_with("--"),
        "output directory must precede the optional flag"
    );
    let query_unit_frames = match args.next() {
        None => QueryUnitFrames::Unspecified,
        Some(flag) if flag == "--declare-preserved-source-query-frames" => {
            QueryUnitFrames::DeclaredSourceQueryFramesPreserved
        }
        Some(_) => anyhow::bail!("unknown fixture option"),
    };
    ensure!(args.next().is_none(), "unexpected fixture argument");
    std::fs::create_dir_all(&out)?;
    let owners = CollisionOwnerBindings {
        terrain_owner_id: "authored-terrain".into(),
        placements: [
            (
                0,
                PlacementOwner::Include {
                    owner_id: "authored-underground".into(),
                },
            ),
            (
                1,
                PlacementOwner::Include {
                    owner_id: "authored-second".into(),
                },
            ),
        ]
        .into(),
    };
    let mut summaries = Vec::new();
    for (empty, name, policy) in [
        (false, "collision.glb", InspectionQueryPolicy::FrontFaces),
        (true, "empty.glb", InspectionQueryPolicy::BothFaces),
    ] {
        let b = build_static_collision(&source(empty), &owners, &CollisionBridgeLimits::default())?;
        let mut identity = blake3::Hasher::new();
        identity.update(SOURCE_FRAME_PROFILE.as_bytes());
        identity.update(include_bytes!("eqg_collision_fixture.rs"));
        identity.update(query_unit_frames.identifier().as_bytes());
        identity.update(&[u8::from(empty)]);
        let bake_revision = identity.finalize().to_hex().to_string();
        let requirements = write_static_collision_with_query_units(
            &b.scene,
            &bake_revision,
            policy,
            query_unit_frames,
            &out.join(name),
            &CollisionWriteLimits::default(),
        )?;
        summaries.push(serde_json::json!({"artifact":name,"requirements":requirements,"query_unit_frames":query_unit_frames.identifier(),"source_frame_profile":SOURCE_FRAME_PROFILE,"source_adapter_report":b.report,"component_decoded":false,"segment_queries":false,"package_binding":false,"gameplay_ready":false}));
    }
    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    const FILES: &[&str] = &["collision.glb", "empty.glb"];
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
