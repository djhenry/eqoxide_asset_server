//! Bounded in-memory geometry bridge. Material, color and sampling decisions are supplied by callers.
use super::{
    surface::{self, RenderReference},
    BinaryZoneScene, ZoneMesh, ZonePlacement,
};
use crate::static_scene::{
    self, StaticAlphaMode, StaticInstance, StaticMaterial, StaticMesh, StaticPrimitive,
    StaticScene, StaticTexture,
};
use anyhow::{bail, ensure, Context, Result};
use glam::{DVec3, Mat4, Vec3};
use libeq_eqg::mesh::MeshKind;
use std::collections::BTreeMap;

/// Deduplicated mesh index and source material TABLE ordinal, never its declared ID.
pub type SourceMaterialKey = (usize, usize);
#[derive(Debug)]
pub enum ResolvedColors {
    Unused,
    Rgba(Vec<[f32; 4]>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondaryUvUse {
    Unused,
    Required,
}
#[derive(Debug)]
pub struct MaterialBinding {
    pub material_index: usize,
    pub colors: ResolvedColors,
    pub secondary_uv: SecondaryUvUse,
}
#[derive(Debug, Default)]
pub struct ResolvedZoneVisual {
    pub materials: Vec<StaticMaterial>,
    pub textures: Vec<StaticTexture>,
    pub bindings: BTreeMap<SourceMaterialKey, MaterialBinding>,
}
/// Limits apply to logical payloads and bridge work, not total process memory.
#[derive(Debug, Clone)]
pub struct BridgeLimits {
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_vertices: usize,
    pub max_indices: usize,
    pub max_resources: usize,
    pub max_transform_visits: usize,
}
impl Default for BridgeLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 128 * 1024 * 1024,
            max_output_bytes: 128 * 1024 * 1024,
            max_vertices: 5_000_000,
            max_indices: 15_000_000,
            max_resources: 100_000,
            max_transform_visits: 50_000_000,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludedPlacementRole {
    LightingMetadata,
    Unassigned,
    RepeatedTerrain,
}
#[derive(Debug)]
pub struct ExcludedPlacement {
    pub placement_index: usize,
    pub role: ExcludedPlacementRole,
    pub extension_bytes: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceSource {
    Terrain,
    Placement(usize),
}
#[derive(Debug, Default)]
pub struct BridgeReport {
    pub source_to_visual_mesh: Vec<Option<usize>>,
    pub instance_sources: Vec<InstanceSource>,
    pub sentinel_triangles: Vec<usize>,
    pub excluded_placements: Vec<ExcludedPlacement>,
    pub unused_bindings: Vec<SourceMaterialKey>,
    pub unused_materials: Vec<usize>,
    pub unused_textures: Vec<usize>,
    pub region_count: usize,
    pub light_count: usize,
    pub transform_visits: usize,
}
#[derive(Debug)]
pub struct BridgedStaticScene {
    pub scene: StaticScene,
    pub report: BridgeReport,
}
/// Convert raw source axes to server axes and emit terrain once at identity.
/// Resolved resources are consumed; the source records remain unchanged.
pub fn build_static_scene(
    source: &BinaryZoneScene,
    mut resolved: ResolvedZoneVisual,
    limits: &BridgeLimits,
) -> Result<BridgedStaticScene> {
    preflight(source, &resolved, limits)?;
    validate_source(source)?;
    validate_resources(source, &resolved)?;
    let mut terrain = source
        .meshes
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind == MeshKind::Terrain);
    let terrain_index = terrain.next().context("missing terrain mesh")?.0;
    ensure!(terrain.next().is_none(), "multiple distinct terrain meshes");
    // Estimate all repeated transformed-vertex work before allocating output/report arrays.
    let mut visits = source.meshes[terrain_index].vertices.len();
    for (pi, p) in source.placements.iter().enumerate() {
        if placement_role(source, pi, terrain_index)?.is_none() {
            let mi = source.model_slots[p.model_index.unwrap() as usize].unwrap();
            visits = visits
                .checked_add(source.meshes[mi].vertices.len())
                .context("transform visits overflow")?;
        }
    }
    ensure!(
        visits <= limits.max_transform_visits,
        "transform visit budget exceeded"
    );
    // No geometry or grouping arrays are allocated until aggregate input/output preflight passes.
    let mut used = vec![false; source.meshes.len()];
    used[terrain_index] = true;
    let mut placements = Vec::new();
    let mut report = BridgeReport {
        source_to_visual_mesh: vec![None; source.meshes.len()],
        sentinel_triangles: vec![0; source.meshes.len()],
        region_count: source.regions.len(),
        light_count: source.lights.len(),
        ..BridgeReport::default()
    };
    for (pi, p) in source.placements.iter().enumerate() {
        let role = placement_role(source, pi, terrain_index)?;
        if let Some(role) = role {
            report.excluded_placements.push(ExcludedPlacement {
                placement_index: pi,
                role,
                extension_bytes: p.extension_data.len(),
            });
            continue;
        }
        let mi = source.model_slots[p.model_index.unwrap() as usize].unwrap();
        ensure!(
            p.extension_data.is_empty(),
            "visual placement {pi} has unresolved extension data"
        );
        ensure!(
            p.scale > 0.,
            "visual placement {pi} must have positive scale"
        );
        let matrix = placement_matrix(p);
        static_scene::validate_matrix(matrix)
            .with_context(|| format!("placement {pi} unsupported transform"))?;
        used[mi] = true;
        placements.push((pi, mi, matrix));
    }
    report.transform_visits = visits;
    let mut scene = StaticScene {
        materials: resolved.materials,
        textures: resolved.textures,
        ..StaticScene::default()
    };
    for (mi, m) in source.meshes.iter().enumerate() {
        let mut groups: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
        for t in &m.triangles {
            match surface::assess(t.material_index, m.materials.len(), t.flags).render_reference {
                RenderReference::Sentinel => report.sentinel_triangles[mi] += 1,
                RenderReference::Material if used[mi] => {
                    let ordinal = t.material_index as usize;
                    ensure!(
                        resolved.bindings.contains_key(&(mi, ordinal)),
                        "mesh {mi} material table ordinal {ordinal} lacks a binding"
                    );
                    groups.entry(ordinal).or_default().extend([
                        t.vertex_indices[0],
                        t.vertex_indices[2],
                        t.vertex_indices[1],
                    ]);
                }
                _ => {}
            }
        }
        if !used[mi] {
            continue;
        }
        ensure!(
            !groups.is_empty(),
            "used mesh {mi} has no visual primitives"
        );
        let primitives = groups
            .into_iter()
            .map(|(ordinal, indices)| {
                let binding = resolved.bindings.remove(&(mi, ordinal)).unwrap();
                StaticPrimitive {
                    indices,
                    material_index: binding.material_index,
                    colors: match binding.colors {
                        ResolvedColors::Unused => None,
                        ResolvedColors::Rgba(c) => Some(c),
                    },
                }
            })
            .collect();
        report.source_to_visual_mesh[mi] = Some(scene.meshes.len());
        scene.meshes.push(StaticMesh {
            name: m.source_name.clone(),
            positions: m.vertices.iter().map(|v| swap(v.position)).collect(),
            normals: m.vertices.iter().map(|v| swap(v.normal)).collect(),
            uvs: m.vertices.iter().map(|v| v.uv0).collect(),
            primitives,
        });
    }
    scene.instances.push(StaticInstance {
        mesh_index: report.source_to_visual_mesh[terrain_index].unwrap(),
        matrix: Mat4::IDENTITY.to_cols_array_2d(),
    });
    report.instance_sources.push(InstanceSource::Terrain);
    for (pi, mi, matrix) in placements {
        let mesh_index = report.source_to_visual_mesh[mi].unwrap();
        let transform = Mat4::from_cols_array_2d(&matrix);
        ensure!(
            scene.meshes[mesh_index]
                .positions
                .iter()
                .all(|p| transform.transform_point3(Vec3::from_array(*p)).is_finite()),
            "placement {pi} transformed positions are nonfinite"
        );
        scene.instances.push(StaticInstance { mesh_index, matrix });
        report.instance_sources.push(InstanceSource::Placement(pi));
    }
    report.unused_bindings = resolved.bindings.into_keys().collect();
    let mut material_used = vec![false; scene.materials.len()];
    for primitive in scene.meshes.iter().flat_map(|m| &m.primitives) {
        material_used[primitive.material_index] = true;
    }
    let mut texture_used = vec![false; scene.textures.len()];
    for (i, m) in scene.materials.iter().enumerate() {
        if material_used[i] {
            if let Some(t) = m.texture_index {
                texture_used[t] = true;
            }
        }
    }
    report.unused_materials = material_used
        .iter()
        .enumerate()
        .filter_map(|(i, &used)| (!used).then_some(i))
        .collect();
    report.unused_textures = texture_used
        .iter()
        .enumerate()
        .filter_map(|(i, &used)| (!used).then_some(i))
        .collect();
    Ok(BridgedStaticScene { scene, report })
}

fn placement_role(
    source: &BinaryZoneScene,
    pi: usize,
    terrain_index: usize,
) -> Result<Option<ExcludedPlacementRole>> {
    let p = &source.placements[pi];
    if source.version == 2 && pi == 0 {
        return Ok(Some(ExcludedPlacementRole::LightingMetadata));
    }
    let Some(slot) = p.model_index else {
        return Ok(Some(ExcludedPlacementRole::Unassigned));
    };
    let mi = source.model_slots[slot as usize].context("placed model slot is null")?;
    Ok((mi == terrain_index).then_some(ExcludedPlacementRole::RepeatedTerrain))
}
fn swap([x, y, z]: [f32; 3]) -> [f32; 3] {
    [y, x, z]
}
fn placement_matrix(p: &ZonePlacement) -> [[f32; 4]; 4] {
    let raw = Mat4::from_translation(Vec3::from_array(p.position))
        * Mat4::from_rotation_z(p.rotation[0])
        * Mat4::from_rotation_y(p.rotation[1])
        * Mat4::from_rotation_x(p.rotation[2])
        * Mat4::from_scale(Vec3::splat(p.scale));
    let raw = raw.to_cols_array_2d();
    let order = [1, 0, 2, 3];
    std::array::from_fn(|col| std::array::from_fn(|row| raw[order[col]][order[row]]))
}
fn add(total: &mut usize, count: usize, stride: usize, limit: usize, label: &str) -> Result<()> {
    let bytes = count
        .checked_mul(stride)
        .context("bridge payload multiplication overflow")?;
    *total = total
        .checked_add(bytes)
        .context("bridge payload addition overflow")?;
    ensure!(*total <= limit, "{label} budget exceeded");
    Ok(())
}
fn preflight(s: &BinaryZoneScene, r: &ResolvedZoneVisual, l: &BridgeLimits) -> Result<()> {
    let mut input = 0;
    let mut output = 0;
    let mut resources = 0;
    let mut vertices = 0;
    let mut indices = 0;
    for count in [
        s.meshes.len(),
        s.model_slots.len(),
        s.model_name_offsets.len(),
        s.placements.len(),
        s.regions.len(),
        s.lights.len(),
        r.materials.len(),
        r.textures.len(),
        r.bindings.len(),
    ] {
        add(&mut resources, count, 1, l.max_resources, "resource count")?;
    }
    for bytes in [
        s.string_table.len(),
        s.trailing_data.len(),
        s.descriptor_name.len(),
    ] {
        add(&mut input, bytes, 1, l.max_input_bytes, "input payload")?;
    }
    add(
        &mut input,
        s.model_slots.len(),
        8,
        l.max_input_bytes,
        "input payload",
    )?;
    add(
        &mut input,
        s.placements.len(),
        36,
        l.max_input_bytes,
        "input payload",
    )?;
    add(
        &mut input,
        s.regions.len(),
        40,
        l.max_input_bytes,
        "input payload",
    )?;
    add(
        &mut input,
        s.lights.len(),
        32,
        l.max_input_bytes,
        "input payload",
    )?;
    let instance_count = s
        .placements
        .len()
        .checked_add(1)
        .context("instance count overflow")?;
    add(
        &mut output,
        instance_count,
        64,
        l.max_output_bytes,
        "output payload",
    )?;
    for p in &s.placements {
        add(
            &mut input,
            p.extension_data.len(),
            1,
            l.max_input_bytes,
            "input payload",
        )?;
    }
    for m in &s.meshes {
        add(
            &mut vertices,
            m.vertices.len(),
            1,
            l.max_vertices,
            "vertex count",
        )?;
        add(
            &mut indices,
            m.triangles.len(),
            3,
            l.max_indices,
            "index count",
        )?;
        add(
            &mut resources,
            m.materials.len(),
            1,
            l.max_resources,
            "resource count",
        )?;
        for n in [
            m.source_name.len(),
            m.string_table.len(),
            m.trailing_data.len(),
        ] {
            add(&mut input, n, 1, l.max_input_bytes, "input payload")?;
        }
        add(
            &mut input,
            m.vertices.len(),
            44,
            l.max_input_bytes,
            "input payload",
        )?;
        add(
            &mut input,
            m.triangles.len(),
            20,
            l.max_input_bytes,
            "input payload",
        )?;
        add(
            &mut input,
            m.materials.len(),
            16,
            l.max_input_bytes,
            "input payload",
        )?;
        add(
            &mut output,
            m.vertices.len(),
            32,
            l.max_output_bytes,
            "output payload",
        )?;
        add(
            &mut output,
            m.triangles.len(),
            12,
            l.max_output_bytes,
            "output payload",
        )?;
        add(
            &mut output,
            m.source_name.len(),
            1,
            l.max_output_bytes,
            "output payload",
        )?;
        for material in &m.materials {
            add(
                &mut resources,
                material.properties.len(),
                1,
                l.max_resources,
                "resource count",
            )?;
            add(
                &mut input,
                material.properties.len(),
                12,
                l.max_input_bytes,
                "input payload",
            )?;
        }
    }
    for binding in r.bindings.values() {
        if let ResolvedColors::Rgba(c) = &binding.colors {
            add(&mut input, c.len(), 16, l.max_input_bytes, "input payload")?;
            add(
                &mut output,
                c.len(),
                16,
                l.max_output_bytes,
                "output payload",
            )?;
        }
    }
    for m in &r.materials {
        add(&mut input, 32, 1, l.max_input_bytes, "input payload")?;
        add(
            &mut input,
            m.name.len(),
            1,
            l.max_input_bytes,
            "input payload",
        )?;
        add(&mut output, 32, 1, l.max_output_bytes, "output payload")?;
        add(
            &mut output,
            m.name.len(),
            1,
            l.max_output_bytes,
            "output payload",
        )?;
    }
    for t in &r.textures {
        add(
            &mut input,
            t.png_bytes.len(),
            1,
            l.max_input_bytes,
            "input payload",
        )?;
        add(
            &mut input,
            t.name.len(),
            1,
            l.max_input_bytes,
            "input payload",
        )?;
        ensure!(
            t.png_bytes.len() <= static_scene::MAX_TEXTURE_BYTES,
            "PNG encoded byte limit exceeded"
        );
        add(
            &mut output,
            t.png_bytes.len(),
            1,
            l.max_output_bytes,
            "output payload",
        )?;
        add(
            &mut output,
            t.name.len(),
            1,
            l.max_output_bytes,
            "output payload",
        )?;
    }
    Ok(())
}
// Every offset before the last NUL has a terminator, including valid substring offsets.
// Find it once per table, avoiding repeated suffix scans for many references.
fn string_offset(last_nul: Option<usize>, offset: u32) -> Result<()> {
    ensure!(
        last_nul.is_some_and(|last| offset as usize <= last),
        "invalid source string offset"
    );
    Ok(())
}
fn validate_source(s: &BinaryZoneScene) -> Result<()> {
    ensure!(
        (1..=2).contains(&s.version),
        "unsupported descriptor version"
    );
    ensure!(s.trailing_data.is_empty(), "unresolved descriptor suffix");
    ensure!(
        s.model_name_offsets.len() == s.model_slots.len(),
        "model slot/name counts differ"
    );
    let last = s.string_table.iter().rposition(|&b| b == 0);
    for (&name, &mesh) in s.model_name_offsets.iter().zip(&s.model_slots) {
        ensure!(
            name.is_some() == mesh.is_some(),
            "model slot/name null mismatch"
        );
        if let Some(name) = name {
            string_offset(last, name)?;
        }
        if let Some(mi) = mesh {
            ensure!(mi < s.meshes.len(), "model slot mesh out of range");
        }
    }
    for p in &s.placements {
        string_offset(last, p.name_offset)?;
        ensure!(
            p.position
                .iter()
                .chain(&p.rotation)
                .chain(std::iter::once(&p.scale))
                .all(|f| f.is_finite()),
            "nonfinite placement fields"
        );
        ensure!(
            p.model_index
                .is_none_or(|slot| (slot as usize) < s.model_slots.len()),
            "placement model slot out of range"
        );
        ensure!(
            s.version != 1 || p.extension_data.is_empty(),
            "descriptor v1 cannot have placement extensions"
        );
        ensure!(
            p.extension_data.len() % 4 == 0,
            "placement extension word is incomplete"
        );
    }
    for r in &s.regions {
        string_offset(last, r.name_offset)?;
    }
    for l in &s.lights {
        string_offset(last, l.name_offset)?;
    }
    for (mi, m) in s.meshes.iter().enumerate() {
        validate_mesh(m).with_context(|| format!("mesh {mi}"))?;
    }
    Ok(())
}
fn validate_mesh(m: &ZoneMesh) -> Result<()> {
    ensure!((1..=3).contains(&m.version), "unsupported mesh version");
    ensure!(
        match m.kind {
            MeshKind::Terrain => m.bone_count.is_none(),
            MeshKind::Model => m.bone_count == Some(0),
        },
        "unsupported bones or bone header context"
    );
    ensure!(m.trailing_data.is_empty(), "unresolved mesh suffix");
    let uv1 = match (m.version, m.uv_marker, m.kind) {
        (1, None, _) => false,
        (3, None, _) => true,
        (2, Some(0), _) => false,
        (2, Some(1), _) | (2, Some(2), MeshKind::Terrain) => true,
        _ => bail!("unsupported UV marker/header context"),
    };
    let last = m.string_table.iter().rposition(|&b| b == 0);
    for material in &m.materials {
        string_offset(last, material.name_offset)?;
        string_offset(last, material.shader_offset)?;
        for property in &material.properties {
            string_offset(last, property.name_offset)?;
            if property.kind == 2 {
                string_offset(last, property.value)?;
            }
        }
    }
    for v in &m.vertices {
        ensure!(
            v.color.is_some() == (m.version == 3) && v.uv1.is_some() == uv1,
            "vertex/header attribute context differs"
        );
        ensure!(
            v.position
                .iter()
                .chain(&v.normal)
                .chain(&v.uv0)
                .all(|f| f.is_finite()),
            "nonfinite source vertex attributes"
        );
        ensure!(
            (Vec3::from_array(v.normal).length_squared() - 1.).abs() <= 1e-4,
            "source normals must be unit length"
        );
    }
    for t in &m.triangles {
        ensure!(
            t.vertex_indices
                .iter()
                .all(|&v| (v as usize) < m.vertices.len()),
            "triangle vertex out of range"
        );
        ensure!(
            surface::assess(t.material_index, m.materials.len(), t.flags).render_reference
                != RenderReference::OutOfRange,
            "triangle material table ordinal out of range"
        );
        let [a, b, c] = t
            .vertex_indices
            .map(|i| DVec3::from_array(m.vertices[i as usize].position.map(f64::from)));
        ensure!(
            (b - a).cross(c - a).length_squared() > 0.,
            "degenerate triangle"
        );
    }
    Ok(())
}
fn normalized(f: f32) -> bool {
    f.is_finite() && (0. ..=1.).contains(&f)
}
fn validate_resources(s: &BinaryZoneScene, r: &ResolvedZoneVisual) -> Result<()> {
    for m in &r.materials {
        ensure!(
            m.base_color.into_iter().all(normalized),
            "invalid material RGBA factor"
        );
        ensure!(
            m.texture_index.is_none_or(|ti| ti < r.textures.len()),
            "material texture out of range"
        );
        if let StaticAlphaMode::Mask { cutoff } = m.alpha_mode {
            ensure!(normalized(cutoff), "invalid material alpha cutoff");
        }
    }
    for t in &r.textures {
        static_scene::validate_png_stream(&t.png_bytes)
            .context("resolved texture is not a complete static PNG")?;
    }
    for (&(mi, ordinal), binding) in &r.bindings {
        let mesh = s.meshes.get(mi).context("binding mesh out of range")?;
        ensure!(
            ordinal < mesh.materials.len(),
            "binding source material table ordinal out of range"
        );
        ensure!(
            binding.material_index < r.materials.len(),
            "binding global material out of range"
        );
        ensure!(
            binding.secondary_uv == SecondaryUvUse::Unused,
            "secondary UV sampling is unsupported"
        );
        if let ResolvedColors::Rgba(c) = &binding.colors {
            ensure!(
                c.len() == mesh.vertices.len() && c.iter().flatten().copied().all(normalized),
                "invalid binding full-pool RGBA colors"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
