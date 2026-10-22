//! Synthetic raw records and explicit authored material bindings; no archive or material inference.
use anyhow::{Context, Result};
use eqoxide_asset_server::{
    eqg::{
        static_scene::{
            build_static_scene, BridgeLimits, MaterialBinding, ResolvedColors, ResolvedZoneVisual,
            SecondaryUvUse,
        },
        BinaryZoneScene, ZoneMesh, ZonePlacement,
    },
    static_scene::{write_static_visual, StaticAlphaMode, StaticMaterial},
};
use libeq_eqg::mesh::{Material, MeshKind, Triangle, Vertex};
use std::{collections::BTreeMap, path::Path};
fn geometry(
    name: &str,
    kind: MeshKind,
    positions: Vec<[f32; 3]>,
    normal: [f32; 3],
    indices: Vec<[u32; 3]>,
) -> ZoneMesh {
    ZoneMesh {
        source_name: name.into(),
        kind,
        version: 1,
        string_table: b"authored\0unresolved\0".to_vec(),
        materials: vec![Material {
            index: 42,
            name_offset: 0,
            shader_offset: Some(9),
            properties: vec![],
        }],
        vertices: positions
            .into_iter()
            .map(|position| Vertex {
                position,
                normal,
                color: None,
                uv0: [0.5, 0.5],
                uv1: None,
            })
            .collect(),
        triangles: indices
            .into_iter()
            .map(|vertex_indices| Triangle {
                vertex_indices,
                material_index: 0,
                flags: 3,
            })
            .collect(),
        bone_count: if kind == MeshKind::Model {
            Some(0)
        } else {
            None
        },
        uv_marker: None,
        trailing_data: vec![],
    }
}
fn main() -> Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .context("usage: eqg_geometry_fixture OUTPUT.glb")?;
    let terrain = geometry(
        "terrain.ter",
        MeshKind::Terrain,
        vec![
            [3., -6., -0.2],
            [3., 6., -0.2],
            [-3., 6., -0.2],
            [-3., -6., -0.2],
        ],
        [0., 0., 1.],
        vec![[0, 1, 2], [0, 2, 3]],
    );
    let object = geometry(
        "object.mod",
        MeshKind::Model,
        vec![[0., -1., 0.], [0., 1., 0.], [0., 0., 2.]],
        [-1., 0., 0.],
        vec![[0, 2, 1]],
    );
    let source = BinaryZoneScene {
        archive_path: "synthetic.eqg".into(),
        descriptor_name: "synthetic.zon".into(),
        descriptor_location: "synthetic",
        version: 1,
        string_table: b"object.mod\0terrain.ter\0instance\0".to_vec(),
        model_name_offsets: vec![Some(11), Some(0), None, Some(0)],
        model_slots: vec![Some(1), Some(0), None, Some(0)],
        meshes: vec![object, terrain],
        placements: vec![
            ZonePlacement {
                model_index: Some(3),
                name_offset: 23,
                position: [0., -3., 0.],
                rotation: [0.15, -0.2, 0.3],
                scale: 1.1,
                extension_data: vec![],
            },
            ZonePlacement {
                model_index: Some(1),
                name_offset: 23,
                position: [0., 0., 0.],
                rotation: [-0.25, 0.2, -0.15],
                scale: 0.8,
                extension_data: vec![],
            },
            ZonePlacement {
                model_index: Some(3),
                name_offset: 23,
                position: [0., 3., 0.],
                rotation: [0.2, 0.15, -0.3],
                scale: 1.3,
                extension_data: vec![],
            },
            ZonePlacement {
                model_index: Some(0),
                name_offset: 23,
                position: [99.; 3],
                rotation: [0.; 3],
                scale: 0.,
                extension_data: vec![],
            },
            ZonePlacement {
                model_index: None,
                name_offset: 23,
                position: [0.; 3],
                rotation: [0.; 3],
                scale: -1.,
                extension_data: vec![],
            },
        ],
        regions: vec![],
        lights: vec![],
        trailing_data: vec![],
    };
    let resolved = ResolvedZoneVisual {
        materials: vec![
            StaticMaterial {
                name: "authored object".into(),
                base_color: [1.; 4],
                texture_index: None,
                alpha_mode: StaticAlphaMode::Opaque,
                double_sided: false,
            },
            StaticMaterial {
                name: "authored ground".into(),
                base_color: [0.6, 0.3, 0.1, 1.],
                texture_index: None,
                alpha_mode: StaticAlphaMode::Opaque,
                double_sided: false,
            },
        ],
        textures: vec![],
        bindings: BTreeMap::from([
            (
                (0, 0),
                MaterialBinding {
                    material_index: 0,
                    colors: ResolvedColors::Rgba(vec![[0.2, 0.9, 0.4, 1.]; 3]),
                    secondary_uv: SecondaryUvUse::Unused,
                },
            ),
            (
                (1, 0),
                MaterialBinding {
                    material_index: 1,
                    colors: ResolvedColors::Unused,
                    secondary_uv: SecondaryUvUse::Unused,
                },
            ),
        ]),
    };
    let bridged = build_static_scene(&source, resolved, &BridgeLimits::default())?;
    let mut revision = blake3::Hasher::new();
    revision.update(b"synthetic-eqg-geometry-v1\0");
    revision.update(include_bytes!("eqg_geometry_fixture.rs"));
    let requirements = write_static_visual(
        &bridged.scene,
        &revision.finalize().to_hex().to_string(),
        Path::new(&output),
    )?;
    let (document, buffers, _) = gltf::import(&output)?;
    let object = document
        .meshes()
        .next()
        .unwrap()
        .primitives()
        .next()
        .unwrap();
    let reader = object.reader(|b| Some(&buffers[b.index()]));
    anyhow::ensure!(
        reader
            .read_indices()
            .unwrap()
            .into_u32()
            .collect::<Vec<_>>()
            == [0, 1, 2],
        "winding was converted more than once"
    );
    anyhow::ensure!(
        document.meshes().count() == 2 && document.nodes().count() == 4,
        "reusable geometry or terrain instances changed"
    );
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"requirements":requirements,"meshes":bridged.scene.meshes.len(),"instances":bridged.scene.instances.len(),"source_to_visual_mesh":bridged.report.source_to_visual_mesh,"sentinel_triangles":bridged.report.sentinel_triangles,"excluded_placements":bridged.report.excluded_placements.iter().map(|p|format!("{:?}",p.role)).collect::<Vec<_>>(),"transform_visits":bridged.report.transform_visits,"regions":bridged.report.region_count,"lights":bridged.report.light_count})
        )?
    );
    Ok(())
}
