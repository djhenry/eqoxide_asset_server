use super::*;
use serde::Serialize;
#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
pub enum DescriptorIdentity {
    Loose {
        content_hash: String,
    },
    ArchiveMember {
        exact_name: String,
        content_hash: String,
    },
}
#[derive(Debug, Serialize)]
pub struct SourceModelIdentity {
    pub mesh_index: usize,
    pub exact_name: String,
    pub blake3: String,
}
#[derive(Debug, Serialize)]
pub struct PropertyReport {
    pub raw_name_offset: u32,
    pub name: String,
    pub kind: u32,
    pub value_word: u32,
    pub resource_index: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct MaterialReport {
    pub source_mesh_index: usize,
    pub source_material_ordinal: usize,
    pub source_declared_id: u32,
    pub raw_name_offset: u32,
    pub raw_shader_offset: Option<u32>,
    pub name: String,
    pub shader: String,
    pub used: bool,
    pub output_material_index: Option<usize>,
    pub output_texture_index: Option<usize>,
    pub properties: Vec<PropertyReport>,
    pub coverage: String,
    pub cutoff: Option<f32>,
    pub decisions: Vec<String>,
    pub base_color_factor: Option<[f32; 4]>,
    pub surface: Option<crate::static_scene::StaticSurfaceModel>,
    pub double_sided: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct ResourceReport {
    pub exact_name: String,
    pub blake3: String,
    pub raw_bytes: usize,
    pub uses: Vec<ResourceUse>,
    pub output_texture_index: Option<usize>,
    pub width: u32,
    pub height: u32,
    pub normalized_png_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct LightingReport {
    pub record_ordinal: Option<usize>,
    pub raw_byte_count: usize,
    pub word_count: usize,
    pub blake3: String,
    pub targetkind: String,
    pub outcome: String,
    pub consumer_scope: String,
    pub placement_mesh_index: Option<usize>,
    pub target_mesh_index: Option<usize>,
    pub current_pool_count: Option<usize>,
    pub applied: bool,
}
#[derive(Debug, Serialize)]
pub struct MeshReport {
    pub source_mesh_index: usize,
    pub source_vertices: usize,
    pub source_triangles: usize,
    pub source_materials: usize,
    pub sentinel_triangles: usize,
    pub output_mesh_index: Option<usize>,
    pub output_primitives: usize,
    pub output_indices: usize,
}
#[derive(Debug, Serialize)]
pub struct ExcludedPlacement {
    pub placement_index: usize,
    pub role: String,
    pub extension_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct WorldBounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}
pub(super) fn indexed_bounds(scene: &StaticScene) -> Result<WorldBounds> {
    let referenced: Vec<Vec<usize>> = scene
        .meshes
        .iter()
        .map(|m| {
            let mut used = vec![false; m.positions.len()];
            for p in &m.primitives {
                for &i in &p.indices {
                    used[i as usize] = true;
                }
            }
            used.into_iter()
                .enumerate()
                .filter_map(|(i, b)| b.then_some(i))
                .collect()
        })
        .collect();
    let mut min = glam::DVec3::splat(f64::INFINITY);
    let mut max = glam::DVec3::splat(f64::NEG_INFINITY);
    for instance in &scene.instances {
        let matrix =
            glam::DMat4::from_cols_array_2d(&instance.matrix.map(|col| col.map(f64::from)));
        for &i in &referenced[instance.mesh_index] {
            let p = matrix.transform_point3(glam::DVec3::from_array(
                scene.meshes[instance.mesh_index].positions[i].map(f64::from),
            ));
            ensure!(p.is_finite(), "nonfinite report world bounds");
            min = min.min(p);
            max = max.max(p);
        }
    }
    ensure!(
        min.is_finite() && max.is_finite(),
        "empty report world bounds"
    );
    Ok(WorldBounds {
        min: min.to_array(),
        max: max.to_array(),
    })
}
#[derive(Debug, Serialize)]
pub struct GeometryReport {
    pub world_bounds_server_geometry: WorldBounds,
    pub source_meshes: usize,
    pub source_model_slots: usize,
    pub source_placements: usize,
    pub source_materials: usize,
    pub used_materials: usize,
    pub output_meshes: usize,
    pub output_instances: usize,
    pub output_primitives: usize,
    pub output_indices: usize,
    pub deduplicated_model_slots: usize,
    pub meshes: Vec<MeshReport>,
    pub excluded_placements: Vec<ExcludedPlacement>,
    pub instance_sources: Vec<String>,
    pub region_count: usize,
    pub light_count: usize,
    pub transform_visits: usize,
}
#[derive(Debug, Default, Serialize)]
pub struct BudgetReport {
    pub archive_bytes: u64,
    pub archive_advertised_bytes: usize,
    pub descriptor_bytes: usize,
    pub preserved_resource_bytes: usize,
    pub normalized_png_bytes: usize,
    pub decoded_pixel_bytes: usize,
    pub source_records: usize,
    pub report_records: usize,
    pub report_string_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct EqgPbrReport {
    pub schema: &'static str,
    pub profile: &'static str,
    pub scope: &'static str,
    pub conversion_revision: String,
    pub implementation_revision: String,
    pub archive_blake3: String,
    pub descriptor: DescriptorIdentity,
    pub models: Vec<SourceModelIdentity>,
    pub materials: Vec<MaterialReport>,
    pub resources: Vec<ResourceReport>,
    pub lighting: Vec<LightingReport>,
    pub geometry: GeometryReport,
    pub budget: BudgetReport,
    pub limitations: Vec<&'static str>,
}

/// Serialize through a capped sink, including formatting overhead.
pub fn bounded_json(value: &impl Serialize, cap: usize) -> Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        cap: usize,
    }
    impl std::io::Write for Bounded {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if b.len() > self.cap.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("report JSON byte limit exceeded"));
            }
            self.bytes.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = Bounded {
        bytes: Vec::new(),
        cap,
    };
    serde_json::to_writer_pretty(&mut sink, value)?;
    Ok(sink.bytes)
}
pub(super) fn lighting(d: &super::super::lighting::LightingDiagnostics<'_>) -> Vec<LightingReport> {
    d.targets
        .iter()
        .map(|t| {
            let bytes = match &t.candidate {
                super::super::lighting::Candidate::Inline { packed_bytes, .. } => *packed_bytes,
                _ => &[],
            };
            LightingReport {
                record_ordinal: t.placement_ordinal,
                raw_byte_count: bytes.len(),
                word_count: bytes.len() / 4,
                blake3: hash(bytes),
                targetkind: format!("{:?}", t.kind),
                outcome: format!("{:?}", t.outcome),
                consumer_scope: format!("{:?}", t.consumer_scope),
                placement_mesh_index: t.placement_mesh_index,
                target_mesh_index: t.target_mesh_index,
                current_pool_count: t.current_pool_count,
                applied: false,
            }
        })
        .collect()
}
pub(super) fn geometry(
    s: &BinaryZoneScene,
    b: &super::super::static_scene::BridgedStaticScene,
    used: usize,
) -> Result<GeometryReport> {
    let r = &b.report;
    let scene = &b.scene;
    Ok(GeometryReport {
        world_bounds_server_geometry: indexed_bounds(scene)?,
        source_meshes: s.meshes.len(),
        source_model_slots: s.model_slots.len(),
        source_placements: s.placements.len(),
        source_materials: s.meshes.iter().map(|m| m.materials.len()).sum(),
        used_materials: used,
        output_meshes: scene.meshes.len(),
        output_instances: scene.instances.len(),
        output_primitives: scene.meshes.iter().map(|m| m.primitives.len()).sum(),
        output_indices: scene
            .meshes
            .iter()
            .flat_map(|m| &m.primitives)
            .map(|p| p.indices.len())
            .sum(),
        deduplicated_model_slots: s
            .model_slots
            .iter()
            .filter(|m| m.is_some())
            .count()
            .saturating_sub(s.meshes.len()),
        meshes: s
            .meshes
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let output = r.source_to_visual_mesh[i].map(|j| &scene.meshes[j]);
                MeshReport {
                    source_mesh_index: i,
                    source_vertices: m.vertices.len(),
                    source_triangles: m.triangles.len(),
                    source_materials: m.materials.len(),
                    sentinel_triangles: r.sentinel_triangles[i],
                    output_mesh_index: r.source_to_visual_mesh[i],
                    output_primitives: output.map_or(0, |m| m.primitives.len()),
                    output_indices: output
                        .map_or(0, |m| m.primitives.iter().map(|p| p.indices.len()).sum()),
                }
            })
            .collect(),
        excluded_placements: r
            .excluded_placements
            .iter()
            .map(|p| ExcludedPlacement {
                placement_index: p.placement_index,
                role: format!("{:?}", p.role),
                extension_bytes: p.extension_bytes,
            })
            .collect(),
        instance_sources: r
            .instance_sources
            .iter()
            .map(|s| format!("{s:?}"))
            .collect(),
        region_count: r.region_count,
        light_count: r.light_count,
        transform_visits: r.transform_visits,
    })
}
