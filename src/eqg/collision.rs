//! Explicit staging geometry for the default static triangle candidate policy.
use super::{
    BinaryZoneScene,
    export::{SkippedPlacement, placement_matrix},
};
use crate::convert::{
    AlphaMode, GlbMetadata, MaterialData, MeshData, NodeDef, PrimitiveData,
    write_glb_instanced_metadata,
};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use libeq_eqg::mesh::MeshKind;
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Default, Serialize)]
pub struct FlagCounts {
    pub considered: usize,
    pub excluded: usize,
    pub degenerate: usize,
    pub emitted: usize,
}
#[derive(Debug, Default, Serialize)]
pub struct CollisionReport {
    pub terrain_instances: usize,
    pub object_instances: usize,
    pub emitted_triangles: usize,
    pub degenerate_triangles: usize,
    /// Counts include each actual placement, plus terrain once.
    pub placement_expanded_flags: BTreeMap<u32, FlagCounts>,
    pub skipped_placements: Vec<SkippedPlacement>,
}

/// Export collision candidates without publishing assets or claiming gameplay readiness.
/// Material references are deliberately irrelevant to triangle eligibility.
pub fn export_collision(scene: &BinaryZoneScene, out: &Path) -> Result<CollisionReport> {
    ensure!(matches!(scene.version, 1 | 2), "unsupported binary zone version");
    let terrain: Vec<_> = scene.meshes.iter().enumerate()
        .filter(|(_, m)| m.kind == MeshKind::Terrain)
        .map(|(i, _)| i)
        .collect();
    ensure!(terrain.len() == 1, "collision requires exactly one distinct terrain mesh");
    let terrain = terrain[0];
    let mut report = CollisionReport {
        terrain_instances: 1,
        ..Default::default()
    };
    let mut instances = vec![(terrain, Mat4::IDENTITY)];
    for (index, p) in scene.placements.iter().enumerate() {
        let reason = if scene.version == 2 && index == 0 {
            Some("terrain_lighting_record")
        } else if p.model_index.is_none() {
            Some("unassigned")
        } else {
            None
        };
        if let Some(reason) = reason {
            report.skipped_placements.push(SkippedPlacement {
                index,
                reason: reason.into(),
            });
            continue;
        }
        let mesh_index = scene.model_slots.get(p.model_index.unwrap() as usize)
            .context("placement model slot out of range")?
            .context("placement references null model-table slot")?;
        ensure!(mesh_index < scene.meshes.len(), "placement mesh index out of range");
        if mesh_index == terrain {
            report.skipped_placements.push(SkippedPlacement {
                index,
                reason: "terrain_emitted_once_at_identity".into(),
            });
            continue;
        }
        instances.push((
            mesh_index,
            placement_matrix(p).with_context(|| format!("placement {index}"))?,
        ));
        report.object_instances += 1;
    }
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for (mesh_index, matrix) in instances {
        let source = &scene.meshes[mesh_index];
        ensure!(matches!(source.version, 1..=3), "unsupported mesh version");
        ensure!(source.bone_count.unwrap_or(0) == 0, "skeletal collision geometry is unsupported");
        let world: Vec<_> = source.vertices.iter().map(|v| {
            matrix.transform_point3(Vec3::new(v.position[0], v.position[2], -v.position[1]))
        }).collect();
        ensure!(world.iter().all(|p| p.is_finite()), "non-finite or overflowing world-space position");
        for tri in &source.triangles {
            ensure!(tri.vertex_indices.iter().all(|&i| (i as usize) < world.len()), "triangle vertex index out of range");
            let counts = report.placement_expanded_flags.entry(tri.flags).or_default();
            counts.considered += 1;
            if tri.flags & 1 != 0 {
                counts.excluded += 1;
                continue;
            }
            let [a, b, c] = [
                tri.vertex_indices[0],
                tri.vertex_indices[2],
                tri.vertex_indices[1],
            ].map(|i| world[i as usize]);
            // Test the emitted f32 positions, including collapse after translation.
            let cross = (b.as_dvec3() - a.as_dvec3()).cross(c.as_dvec3() - a.as_dvec3());
            if cross.length_squared() == 0. {
                counts.degenerate += 1;
                report.degenerate_triangles += 1;
                continue;
            }
            let base = u32::try_from(positions.len()).context("collision vertex index overflow")?;
            ensure!(base <= u32::MAX - 3, "collision vertex index overflow");
            positions.extend([a.to_array(), b.to_array(), c.to_array()]);
            normals.extend([cross.normalize().as_vec3().to_array(); 3]);
            indices.extend([base, base + 1, base + 2]);
            counts.emitted += 1;
            report.emitted_triangles += 1;
        }
    }
    ensure!(!indices.is_empty(), "collision contains no nondegenerate candidate triangles");
    let mesh = MeshData {
        name: "__collision__".into(),
        uvs: vec![[0.; 2]; positions.len()],
        positions,
        normals,
        primitives: vec![PrimitiveData { indices, material_idx: 0, extras: None }],
    };
    let material = MaterialData {
        name: "__collision__".into(),
        texture_idx: None,
        base_color: [1.; 4],
        alpha_mode: AlphaMode::Opaque,
        anim: None,
    };
    let metadata = GlbMetadata {
        extras: Some(serde_json::json!({"eqCollision": {
            "version": 1,
            "coordinates": "eqg_gltf_y_up",
            "scope": "default_static_triangle_candidates",
            "nodes": [0],
        }})),
        node_names: BTreeMap::from([(0, "__collision__".into())]),
    };
    let parent = out.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let temp = tempfile::NamedTempFile::new_in(parent).context("create sibling collision staging file")?;
    write_glb_instanced_metadata(
        temp.path(), &[mesh], &[material], &[],
        &[NodeDef { mesh_idx: 0, matrix: None }],
        &BTreeMap::new(), Some(&metadata),
    )?;
    temp.persist(out).map_err(|e| e.error)
        .context("publish collision staging file")?;
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ZoneMesh, ZonePlacement};
    use libeq_eqg::mesh::{MeshKind, Vertex, Triangle};
    fn mesh(kind: MeshKind) -> ZoneMesh {
        ZoneMesh {
            source_name: "test".into(),
            kind,
            version: 3,
            string_table: vec![0],
            materials: vec![libeq_eqg::mesh::Material {
                index: 0, name_offset: 0, shader_offset: 0, properties: vec![],
            }],
            vertices: [[1., 2., 3.], [2., 2., 3.], [1., 3., 3.]].into_iter()
                .map(|position| Vertex {
                    position, normal: [0., 0., 1.], color: None,
                    uv0: [0.; 2], uv1: None,
                }).collect(),
            triangles: vec![Triangle {
                vertex_indices: [0, 1, 2], material_index: u32::MAX, flags: 0,
            }],
            bone_count: None,
            uv_marker: None,
            trailing_data: vec![],
        }
    }
    fn placement(slot: Option<u32>) -> ZonePlacement {
        ZonePlacement {
            model_index: slot,
            name_offset: 0,
            position: [10., 20., 30.],
            rotation: [0.2, 0.4, 0.7],
            scale: 2.,
            extension_data: vec![],
        }
    }
    fn scene() -> BinaryZoneScene {
        BinaryZoneScene {
            archive_path: "unused".into(),
            descriptor_name: "test".into(),
            descriptor_location: "loose",
            version: 2,
            string_table: vec![],
            model_name_offsets: vec![],
            model_slots: vec![Some(0), Some(1)],
            meshes: vec![mesh(MeshKind::Terrain), mesh(MeshKind::Model)],
            placements: vec![
                placement(Some(1)), placement(None), placement(Some(0)),
                placement(Some(1)), placement(Some(1)),
            ],
            regions: vec![],
            lights: vec![],
            trailing_data: vec![],
        }
    }
    #[test]
    fn collision_policy_transforms_winding_and_authoritative_contract() {
        let mut s = scene();
        s.placements[4].position = [20.,40.,60.];
        s.meshes[0].triangles.extend([Triangle{vertex_indices:[0,1,2],material_index:0,flags:1},Triangle{vertex_indices:[0,1,2],material_index:u32::MAX,flags:2}]);
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("collision.glb");
        export_collision(&s,&out).unwrap();
        let (doc, buffers, _) = gltf::import(&out).unwrap();
        let extras:serde_json::Value=serde_json::from_str(doc.as_json().extras.as_ref().unwrap().get()).unwrap();
        assert_eq!(extras["eqCollision"],serde_json::json!({"version":1,"coordinates":"eqg_gltf_y_up","scope":"default_static_triangle_candidates","nodes":[0]}));
        assert_eq!(doc.nodes().count(),1);
        assert_eq!(doc.nodes().next().unwrap().name(),Some("__collision__"));
        let primitive=doc.meshes().next().unwrap().primitives().next().unwrap();
        let reader=primitive.reader(|b|Some(&buffers[b.index()]));
        let p:Vec<_>=reader.read_positions().unwrap().collect();
        let i:Vec<_>=reader.read_indices().unwrap().into_u32().collect();
        assert_eq!(i.len(),12); // terrain 0 and 2; two actual instances
        assert_eq!(p[i[0] as usize],[1.,3.,-2.]);
        assert_eq!(p[i[1] as usize],[1.,3.,-3.]); // reversed source winding
        let world = p[i[6] as usize];
        for (a, b) in world.into_iter().zip([14.700434, 35.821416, -20.130496]) {
            assert!((a - b).abs() < 1e-5, "{world:?}");
        }
        let second = p[i[9] as usize];
        for ((a, b), delta) in second.into_iter().zip(world).zip([10.,30.,-20.]) {
            assert!((a - b - delta).abs() < 1e-5);
        }
    }
    #[test]
    fn reports_world_space_degeneracy_and_preserves_unknown_flags() {
        let mut s = scene();
        s.meshes[0].triangles[0].flags = 0x80;
        s.meshes[0].triangles.push(Triangle {vertex_indices:[0,0,1], material_index:0, flags:0});
        for p in &mut s.placements[3..] {
            p.position = [1e20;3];
        }
        let dir = tempfile::tempdir().unwrap();
        let report = export_collision(&s, &dir.path().join("collision.glb")).unwrap();
        assert_eq!(report.emitted_triangles, 1);
        assert_eq!(report.degenerate_triangles, 3);
        assert_eq!(report.placement_expanded_flags[&0x80].emitted, 1);
        for counts in report.placement_expanded_flags.values() {
            assert_eq!(counts.considered, counts.excluded + counts.emitted + counts.degenerate);
        }
    }

    #[test]
    fn invalid_and_empty_exports_preserve_last_good() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("collision.glb");
        std::fs::write(&out,b"last good").unwrap();
        for case in 0..10 {
            let mut s = scene();
            match case {
                0=>for m in &mut s.meshes {m.triangles[0].flags=1},
                1=>s.meshes[0].triangles[0].vertex_indices[0]=99,
                2=>s.meshes[1].bone_count=Some(1),
                3=>s.meshes[0].vertices[0].position[0]=f32::NAN,
                4=>for m in &mut s.meshes {m.triangles[0].vertex_indices=[0,0,0]},
                5=>s.placements[3].model_index=Some(100),
                6=>s.placements[3].scale=-1.,
                7=>s.placements[3].position[0]=f32::INFINITY,
                8=>s.version=3,
                _=>s.placements[3].scale=f32::MAX,
            }
            assert!(export_collision(&s,&out).is_err(),"case {case}");
            assert_eq!(std::fs::read(&out).unwrap(),b"last good");
        }
    }
}
