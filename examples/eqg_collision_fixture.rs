//! Authored raw binary-zone records through the common collision adapter and writer.
use anyhow::{ensure, Context, Result};
use eqoxide_asset_server::{
    eqg::{static_collision::*, BinaryZoneScene, ZoneMesh, ZonePlacement},
    static_collision::{write_static_collision, CollisionWriteLimits, InspectionQueryPolicy},
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
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let out = PathBuf::from(
        args.next()
            .context("usage: eqg_collision_fixture OUTPUT_DIRECTORY")?,
    );
    ensure!(args.next().is_none(), "unexpected argument");
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
        let requirements = write_static_collision(
            &b.scene,
            &"a".repeat(64),
            policy,
            &out.join(name),
            &CollisionWriteLimits::default(),
        )?;
        summaries.push(serde_json::json!({"artifact":name,"requirements":requirements,"source_adapter_report":b.report,"component_decoded":false,"segment_queries":false,"package_binding":false,"gameplay_ready":false}));
    }
    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
}
