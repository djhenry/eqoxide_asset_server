//! Binary-zone default candidates normalized into the common collision geometry input.
//! Caller declarations and extraction diagnostics do not certify live-world ownership.
use super::{
    static_scene::{placement_matrix, swap},
    surface, BinaryZoneScene,
};
use crate::{
    compatibility::token,
    static_collision::{
        triangle, StaticCollisionInstance, StaticCollisionMesh, StaticCollisionPrimitive,
        StaticCollisionScene,
    },
    static_scene::{convert_matrix, map_axes, validate_matrix},
};
use anyhow::{ensure, Context, Result};
use glam::{Mat4, Vec3};
use libeq_eqg::mesh::MeshKind;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PlacementOwner {
    Include { owner_id: String },
    Exclude { reason: String },
}
#[derive(Debug, Clone)]
pub struct CollisionOwnerBindings {
    pub terrain_owner_id: String,
    pub placements: BTreeMap<usize, PlacementOwner>,
}
/// Logical payload/work caps for already assembled input; not archive ingestion or total RSS.
#[derive(Debug, Clone)]
pub struct CollisionBridgeLimits {
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_vertices: usize,
    pub max_indices: usize,
    pub max_resources: usize,
    pub max_expanded_triangles: usize,
    pub max_world_vertices: usize,
}
impl Default for CollisionBridgeLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 134217728,
            max_output_bytes: 134217728,
            max_vertices: 5000000,
            max_indices: 15000000,
            max_resources: 100000,
            max_expanded_triangles: 5000000,
            max_world_vertices: 50000000,
        }
    }
}
#[derive(Debug, Default, Serialize)]
pub struct CollisionBridgeWork {
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub resources: usize,
    pub vertices: usize,
    pub indices: usize,
    pub expanded_triangles: usize,
    pub world_vertices: usize,
    pub diagnostic_triangle_visits: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CollisionOwnerSource {
    Terrain,
    Placement(usize),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AutomaticExclusionRole {
    LightingMetadata,
    Unassigned,
    RepeatedTerrain,
}
#[derive(Debug, Serialize)]
pub struct AutomaticExclusion {
    pub placement_index: usize,
    pub role: AutomaticExclusionRole,
    pub extension_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct CollisionOwnerReport {
    pub source: CollisionOwnerSource,
    pub decision: PlacementOwner,
    pub extension_bytes: usize,
    pub zero_candidates: bool,
    pub instance_index: Option<usize>,
}
#[derive(Debug, Default, Serialize)]
pub struct CandidateFlagCounts {
    pub considered: usize,
    pub eligible: usize,
    pub excluded: usize,
}
#[derive(Debug, Serialize)]
pub struct CollisionBridgeReport {
    pub source_to_collision_mesh: Vec<Option<usize>>,
    pub owners: Vec<CollisionOwnerReport>,
    pub automatic_exclusions: Vec<AutomaticExclusion>,
    pub source_flags: BTreeMap<u32, CandidateFlagCounts>,
    pub expanded_flags: BTreeMap<u32, CandidateFlagCounts>,
    pub region_count: usize,
    pub light_count: usize,
    pub work: CollisionBridgeWork,
    /// Always false: supplied declarations are not independent ownership verification.
    pub ownership_verified: bool,
    /// Always false: static candidates do not certify regions, actors or world coverage.
    pub world_complete: bool,
}
#[derive(Debug)]
pub struct BridgedStaticCollision {
    pub scene: StaticCollisionScene,
    pub report: CollisionBridgeReport,
}
fn charge(total: &mut usize, count: usize, stride: usize, limit: usize, label: &str) -> Result<()> {
    let n = count
        .checked_mul(stride)
        .context("bridge count multiplication overflow")?;
    *total = total
        .checked_add(n)
        .context("bridge count addition overflow")?;
    ensure!(*total <= limit, "{label} budget exceeded");
    Ok(())
}
fn preflight(
    s: &BinaryZoneScene,
    o: &CollisionOwnerBindings,
    l: &CollisionBridgeLimits,
) -> Result<CollisionBridgeWork> {
    let mut w = CollisionBridgeWork::default();
    for count in [
        s.meshes.len(),
        s.model_slots.len(),
        s.model_name_offsets.len(),
        s.placements.len(),
        s.regions.len(),
        s.lights.len(),
        o.placements.len(),
        1,
    ] {
        charge(&mut w.resources, count, 1, l.max_resources, "resource")?;
    }
    for (count, stride) in [
        (s.model_slots.len(), 8),
        (s.model_name_offsets.len(), 4),
        (s.placements.len(), 36),
        (s.regions.len(), 40),
        (s.lights.len(), 32),
        (o.placements.len(), 16),
    ] {
        charge(
            &mut w.input_bytes,
            count,
            stride,
            l.max_input_bytes,
            "input",
        )?;
    }
    for bytes in [
        s.string_table.len(),
        s.descriptor_name.len(),
        s.trailing_data.len(),
        s.archive_path.as_os_str().len(),
        s.descriptor_location.len(),
        o.terrain_owner_id.len(),
    ] {
        charge(&mut w.input_bytes, bytes, 1, l.max_input_bytes, "input")?;
    }
    // Cumulative conservative logical bounds: per mesh, temporary histogram/list
    // containers, mapping/count/multiplicity/used arrays and final mesh/primitive.
    // Per owner, temporary selected/identity records plus final instance/report.
    // Strings retained both in an owner report and instance are charged twice.
    let owner_count = s
        .placements
        .len()
        .checked_add(1)
        .context("owner count overflow")?;
    for (count, stride) in [
        (s.meshes.len(), 256),
        (owner_count, 320),
        (s.placements.len(), 32),
    ] {
        charge(
            &mut w.output_bytes,
            count,
            stride,
            l.max_output_bytes,
            "output",
        )?;
    }
    charge(
        &mut w.resources,
        owner_count,
        2,
        l.max_resources,
        "resource",
    )?;
    charge(
        &mut w.output_bytes,
        o.terrain_owner_id.len(),
        2,
        l.max_output_bytes,
        "output",
    )?;
    for p in &s.placements {
        charge(
            &mut w.input_bytes,
            p.extension_data.len(),
            1,
            l.max_input_bytes,
            "input",
        )?;
    }
    for owner in o.placements.values() {
        let text = match owner {
            PlacementOwner::Include { owner_id } => owner_id,
            PlacementOwner::Exclude { reason } => reason,
        };
        charge(
            &mut w.input_bytes,
            text.len(),
            1,
            l.max_input_bytes,
            "input",
        )?;
        charge(
            &mut w.output_bytes,
            text.len(),
            2,
            l.max_output_bytes,
            "output",
        )?;
    }
    for m in &s.meshes {
        charge(
            &mut w.vertices,
            m.vertices.len(),
            1,
            l.max_vertices,
            "vertex",
        )?;
        charge(&mut w.indices, m.triangles.len(), 3, l.max_indices, "index")?;
        charge(
            &mut w.resources,
            m.materials.len(),
            1,
            l.max_resources,
            "resource",
        )?;
        // Fixed logical fields plus all retained optional vertex attribute payloads.
        for (count, stride) in [
            (1, 32),
            (m.vertices.len(), 44),
            (m.triangles.len(), 20),
            (m.materials.len(), 16),
        ] {
            charge(
                &mut w.input_bytes,
                count,
                stride,
                l.max_input_bytes,
                "input",
            )?;
        }
        for bytes in [
            m.source_name.len(),
            m.string_table.len(),
            m.trailing_data.len(),
        ] {
            charge(&mut w.input_bytes, bytes, 1, l.max_input_bytes, "input")?;
        }
        for material in &m.materials {
            charge(
                &mut w.resources,
                material.properties.len(),
                1,
                l.max_resources,
                "resource",
            )?;
            charge(
                &mut w.input_bytes,
                material.properties.len(),
                12,
                l.max_input_bytes,
                "input",
            )?;
        }
        charge(
            &mut w.output_bytes,
            m.vertices.len(),
            12,
            l.max_output_bytes,
            "output",
        )?;
        charge(
            &mut w.output_bytes,
            m.triangles.len(),
            20,
            l.max_output_bytes,
            "output",
        )?;
    }
    Ok(w)
}
fn offset(last: Option<usize>, n: u32) -> Result<()> {
    ensure!(
        last.is_some_and(|last| n as usize <= last),
        "invalid source string offset"
    );
    Ok(())
}
fn validate_source(s: &BinaryZoneScene) -> Result<usize> {
    ensure!(
        (1..=2).contains(&s.version),
        "unsupported descriptor version"
    );
    ensure!(s.trailing_data.is_empty(), "unresolved descriptor suffix");
    ensure!(
        s.model_slots.len() == s.model_name_offsets.len(),
        "model slot/name count mismatch"
    );
    let last = s.string_table.iter().rposition(|&b| b == 0);
    for (mi, name) in s.model_slots.iter().zip(&s.model_name_offsets) {
        ensure!(
            mi.is_some() == name.is_some(),
            "model slot/name null mismatch"
        );
        if let Some(mi) = mi {
            ensure!(*mi < s.meshes.len(), "model slot mesh out of range");
        }
        if let Some(name) = name {
            offset(last, *name)?;
        }
    }
    for p in &s.placements {
        offset(last, p.name_offset)?;
        ensure!(
            p.position
                .iter()
                .chain(&p.rotation)
                .chain(std::iter::once(&p.scale))
                .all(|v| v.is_finite()),
            "nonfinite placement fields"
        );
        ensure!(
            p.model_index
                .is_none_or(|slot| (slot as usize) < s.model_slots.len()),
            "placement slot out of range"
        );
        ensure!(
            s.version != 1 || p.extension_data.is_empty(),
            "v1 extension is unsupported"
        );
        ensure!(p.extension_data.len() % 4 == 0, "incomplete extension word");
    }
    for r in &s.regions {
        offset(last, r.name_offset)?;
    }
    for l in &s.lights {
        offset(last, l.name_offset)?;
    }
    let mut terrain = None;
    for (mi, m) in s.meshes.iter().enumerate() {
        ensure!((1..=3).contains(&m.version), "unsupported mesh version");
        ensure!(m.trailing_data.is_empty(), "unresolved mesh suffix");
        ensure!(
            match m.kind {
                MeshKind::Terrain => m.bone_count.is_none(),
                MeshKind::Model => m.bone_count == Some(0),
            },
            "unsupported skeletal geometry/header"
        );
        if m.kind == MeshKind::Terrain {
            ensure!(terrain.replace(mi).is_none(), "multiple terrain meshes");
        }
        ensure!(
            m.vertices
                .iter()
                .all(|v| v.position.iter().all(|x| x.is_finite())),
            "nonfinite source position"
        );
        for t in &m.triangles {
            ensure!(
                t.vertex_indices
                    .iter()
                    .all(|&i| (i as usize) < m.vertices.len()),
                "triangle index out of range"
            );
        }
    }
    terrain.context("missing terrain mesh")
}
fn role(s: &BinaryZoneScene, pi: usize, terrain: usize) -> Result<Option<AutomaticExclusionRole>> {
    let p = &s.placements[pi];
    if s.version == 2 && pi == 0 {
        return Ok(Some(AutomaticExclusionRole::LightingMetadata));
    }
    let Some(slot) = p.model_index else {
        return Ok(Some(AutomaticExclusionRole::Unassigned));
    };
    let mi = s.model_slots[slot as usize].context("actual placement references null slot")?;
    Ok((mi == terrain).then_some(AutomaticExclusionRole::RepeatedTerrain))
}
fn merge_flags(
    map: &mut BTreeMap<u32, CandidateFlagCounts>,
    flag: u32,
    count: usize,
    w: &mut CollisionBridgeWork,
    l: &CollisionBridgeLimits,
) -> Result<()> {
    if !map.contains_key(&flag) {
        charge(&mut w.resources, 1, 1, l.max_resources, "flag resource")?;
        charge(
            &mut w.output_bytes,
            1,
            32,
            l.max_output_bytes,
            "flag report output",
        )?;
    }
    let entry = map.entry(flag).or_default();
    entry.considered = entry
        .considered
        .checked_add(count)
        .context("flag count overflow")?;
    let target = if flag & 1 == 0 {
        &mut entry.eligible
    } else {
        &mut entry.excluded
    };
    *target = target.checked_add(count).context("flag count overflow")?;
    Ok(())
}
/// Compile only explicitly accounted-for default candidates. No material/normal/UV semantics
/// or native actor identity is inferred. Refusals preserve the borrowed source unchanged.
pub fn build_static_collision(
    s: &BinaryZoneScene,
    o: &CollisionOwnerBindings,
    l: &CollisionBridgeLimits,
) -> Result<BridgedStaticCollision> {
    let mut work = preflight(s, o, l)?;
    let terrain = validate_source(s)?;
    ensure!(token(&o.terrain_owner_id), "invalid terrain owner identity");
    let mut identities = BTreeSet::new();
    identities.insert(o.terrain_owner_id.as_str());
    let mut selected = vec![(
        CollisionOwnerSource::Terrain,
        terrain,
        Mat4::IDENTITY.to_cols_array_2d(),
        o.terrain_owner_id.as_str(),
    )];
    let mut automatic_exclusions = Vec::new();
    let mut caller_excluded = Vec::new();
    let mut actual_count = 0;
    for (pi, p) in s.placements.iter().enumerate() {
        if let Some(role) = role(s, pi, terrain)? {
            ensure!(
                !o.placements.contains_key(&pi),
                "extraneous owner binding for automatic exclusion"
            );
            automatic_exclusions.push(AutomaticExclusion {
                placement_index: pi,
                role,
                extension_bytes: p.extension_data.len(),
            });
            continue;
        }
        actual_count += 1;
        let owner = o
            .placements
            .get(&pi)
            .context("missing owner binding for actual placement")?;
        match owner {
            PlacementOwner::Exclude { reason } => {
                ensure!(token(reason), "invalid exclusion reason");
                caller_excluded.push(pi);
            }
            PlacementOwner::Include { owner_id } => {
                ensure!(token(owner_id), "invalid owner identity");
                ensure!(
                    identities.insert(owner_id.as_str()),
                    "duplicate owner identity"
                );
                ensure!(
                    p.extension_data.is_empty(),
                    "included placement extension is unresolved"
                );
                ensure!(p.scale > 0., "included placement requires positive scale");
                let matrix = placement_matrix(p);
                validate_matrix(matrix).context("unsupported included transform")?;
                let mi = s.model_slots[p.model_index.unwrap() as usize].unwrap();
                selected.push((CollisionOwnerSource::Placement(pi), mi, matrix, owner_id));
            }
        }
    }
    ensure!(
        o.placements.len() == actual_count,
        "extraneous owner binding"
    );
    let mut candidate_handles = vec![Vec::new(); s.meshes.len()];
    let mut histograms: Vec<BTreeMap<u32, CandidateFlagCounts>> =
        (0..s.meshes.len()).map(|_| BTreeMap::new()).collect();
    for (mi, m) in s.meshes.iter().enumerate() {
        for (ti, t) in m.triangles.iter().enumerate() {
            work.diagnostic_triangle_visits = work
                .diagnostic_triangle_visits
                .checked_add(1)
                .context("diagnostic visits overflow")?;
            merge_flags(&mut histograms[mi], t.flags, 1, &mut work, l)?;
            if surface::assess(t.material_index, m.materials.len(), t.flags)
                .default_collision_query_candidate
            {
                candidate_handles[mi].push(ti);
            }
        }
    }
    let mut multiplicities = vec![0usize; s.meshes.len()];
    for (_, mi, _, _) in &selected {
        multiplicities[*mi] = multiplicities[*mi]
            .checked_add(1)
            .context("owner multiplicity overflow")?;
    }
    let mut source_flags = BTreeMap::new();
    let mut expanded_flags = BTreeMap::new();
    for (mi, histogram) in histograms.iter().enumerate() {
        for (&flag, counts) in histogram {
            merge_flags(&mut source_flags, flag, counts.considered, &mut work, l)?;
            if multiplicities[mi] > 0 {
                let expanded = counts
                    .considered
                    .checked_mul(multiplicities[mi])
                    .context("expanded flag count overflow")?;
                merge_flags(&mut expanded_flags, flag, expanded, &mut work, l)?;
            }
        }
    }
    let mut eligible = vec![0usize; s.meshes.len()];
    let mut used = vec![false; s.meshes.len()];
    for mi in 0..s.meshes.len() {
        eligible[mi] = candidate_handles[mi].len();
        let expanded = eligible[mi]
            .checked_mul(multiplicities[mi])
            .context("expanded triangle count overflow")?;
        charge(
            &mut work.expanded_triangles,
            expanded,
            1,
            l.max_expanded_triangles,
            "expanded triangle",
        )?;
        charge(
            &mut work.world_vertices,
            expanded,
            3,
            l.max_world_vertices,
            "world vertex",
        )?;
        used[mi] = multiplicities[mi] > 0 && eligible[mi] > 0;
    }
    // Iterate only eligible triangle handles per owner. Excluded-face diagnostics were
    // aggregated above once per source group, independently of owner multiplicity.
    for (_, mi, matrix, _) in &selected {
        let server = Mat4::from_cols_array_2d(matrix);
        let asset = Mat4::from_cols_array_2d(&convert_matrix(*matrix));
        ensure!(asset.is_finite(), "nonfinite converted matrix");
        for &ti in &candidate_handles[*mi] {
            let t = &s.meshes[*mi].triangles[ti];
            let p = [
                t.vertex_indices[0],
                t.vertex_indices[2],
                t.vertex_indices[1],
            ]
            .map(|i| swap(s.meshes[*mi].vertices[i as usize].position));
            triangle(p).context("selected local candidate")?;
            triangle(p.map(|p| server.transform_point3(Vec3::from_array(p)).to_array()))
                .context("selected server world candidate")?;
            triangle(p.map(|p| {
                asset
                    .transform_point3(Vec3::from_array(map_axes(p)))
                    .to_array()
            }))
            .context("selected asset world candidate")?;
        }
    }
    let mut meshes = Vec::new();
    let mut mapping = vec![None; s.meshes.len()];
    for (mi, m) in s.meshes.iter().enumerate() {
        if !used[mi] {
            continue;
        }
        mapping[mi] = Some(meshes.len());
        let indices = m
            .triangles
            .iter()
            .filter(|t| {
                surface::assess(t.material_index, m.materials.len(), t.flags)
                    .default_collision_query_candidate
            })
            .flat_map(|t| {
                [
                    t.vertex_indices[0],
                    t.vertex_indices[2],
                    t.vertex_indices[1],
                ]
            })
            .collect();
        meshes.push(StaticCollisionMesh {
            positions: m.vertices.iter().map(|v| swap(v.position)).collect(),
            primitives: vec![StaticCollisionPrimitive { indices }],
        });
    }
    let mut instances = Vec::new();
    let mut owners = Vec::new();
    for (source, mi, matrix, owner_id) in selected {
        let instance_index = if let Some(mesh_index) = mapping[mi] {
            let index = instances.len();
            instances.push(StaticCollisionInstance {
                owner_id: owner_id.into(),
                mesh_index,
                matrix,
            });
            Some(index)
        } else {
            None
        };
        let extension_bytes = match source {
            CollisionOwnerSource::Terrain => 0,
            CollisionOwnerSource::Placement(pi) => s.placements[pi].extension_data.len(),
        };
        owners.push(CollisionOwnerReport {
            source,
            decision: PlacementOwner::Include {
                owner_id: owner_id.into(),
            },
            extension_bytes,
            zero_candidates: eligible[mi] == 0,
            instance_index,
        });
    }
    for pi in caller_excluded {
        owners.push(CollisionOwnerReport {
            source: CollisionOwnerSource::Placement(pi),
            decision: o.placements[&pi].clone(),
            extension_bytes: s.placements[pi].extension_data.len(),
            zero_candidates: false,
            instance_index: None,
        });
    }
    let scene = if instances.is_empty() {
        StaticCollisionScene::AvailableEmpty
    } else {
        StaticCollisionScene::Available { meshes, instances }
    };
    Ok(BridgedStaticCollision {
        scene,
        report: CollisionBridgeReport {
            source_to_collision_mesh: mapping,
            owners,
            automatic_exclusions,
            source_flags,
            expanded_flags,
            region_count: s.regions.len(),
            light_count: s.lights.len(),
            work,
            ownership_verified: false,
            world_complete: false,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counter_overflows_refuse_instead_of_wrapping() {
        let mut n = usize::MAX;
        assert!(charge(&mut n, 1, 1, usize::MAX, "test").is_err());
        let mut n = 0;
        assert!(charge(&mut n, usize::MAX, 2, usize::MAX, "test").is_err());
    }
}
