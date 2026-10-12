//! Bounded positions-only collision candidates for source-independent inspection.
//! This artifact supplies neither movement policy nor completeness of a live world.
use crate::{
    compatibility::{token, valid_hash, ReaderRequirements},
    static_scene::{convert_matrix, map_axes, publish_static, validate_matrix},
};
use anyhow::{ensure, Context, Result};
use glam::{DVec3, Mat4, Vec3};
use serde_json::json;
use std::{
    collections::BTreeSet,
    io::{self, Write},
    path::Path,
};

#[derive(Debug, Clone)]
pub enum StaticCollisionScene {
    Available {
        meshes: Vec<StaticCollisionMesh>,
        instances: Vec<StaticCollisionInstance>,
    },
    AvailableEmpty,
}
#[derive(Debug, Clone)]
pub struct StaticCollisionMesh {
    pub positions: Vec<[f32; 3]>,
    pub primitives: Vec<StaticCollisionPrimitive>,
}
#[derive(Debug, Clone)]
pub struct StaticCollisionPrimitive {
    pub indices: Vec<u32>,
}
#[derive(Debug, Clone)]
pub struct StaticCollisionInstance {
    pub owner_id: String,
    pub mesh_index: usize,
    /// Column-major matrix in server geometry axes.
    pub matrix: [[f32; 4]; 4],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionQueryPolicy {
    FrontFaces,
    BothFaces,
}
impl InspectionQueryPolicy {
    fn identifier(self) -> &'static str {
        match self {
            Self::FrontFaces => "inspection-segment-front-v1",
            Self::BothFaces => "inspection-segment-both-v1",
        }
    }
}
#[derive(Debug, Clone)]
pub struct CollisionWriteLimits {
    pub max_glb_bytes: usize,
    pub max_json_bytes: usize,
    pub max_geometry_bytes: usize,
    pub max_resources: usize,
    pub max_vertices: usize,
    pub max_indices: usize,
    pub max_expanded_triangles: usize,
    /// Three paired source/asset vertex evaluations per instanced triangle.
    pub max_world_vertices: usize,
}
impl Default for CollisionWriteLimits {
    fn default() -> Self {
        Self {
            max_glb_bytes: 134217728,
            max_json_bytes: 8388608,
            max_geometry_bytes: 134217728,
            max_resources: 100000,
            max_vertices: 5000000,
            max_indices: 15000000,
            max_expanded_triangles: 5000000,
            max_world_vertices: 50000000,
        }
    }
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).context("collision count overflow")
}
fn mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).context("collision size overflow")
}
fn aligned(n: usize) -> Result<usize> {
    Ok(add(n, 3)? & !3)
}
fn charge(total: &mut usize, n: usize, limit: usize, name: &str) -> Result<()> {
    *total = add(*total, n)?;
    ensure!(*total <= limit, "collision {name} limit exceeded");
    Ok(())
}
fn triangle(points: [[f32; 3]; 3]) -> Result<()> {
    ensure!(
        points.iter().flatten().all(|v| v.is_finite()),
        "collision transformed position nonfinite"
    );
    let [a, b, c] = points.map(|p| DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
    ensure!(
        (b - a).cross(c - a).length_squared() > 0.,
        "collision degenerate triangle"
    );
    Ok(())
}
/// Validate and atomically replace a collision GLB. The requirements describe only decoding.
pub fn write_static_collision(
    scene: &StaticCollisionScene,
    bake_revision: &str,
    policy: InspectionQueryPolicy,
    output: &Path,
    limits: &CollisionWriteLimits,
) -> Result<ReaderRequirements> {
    ensure!(
        valid_hash(bake_revision),
        "bake revision must be 64 lowercase hexadecimal digits"
    );
    let requirements = ReaderRequirements {
        reader_version: 2,
        capabilities: vec!["static-collision-triangles-v1".into()],
    };
    let (meshes, instances, availability) = match scene {
        StaticCollisionScene::Available { meshes, instances } => {
            ensure!(
                !meshes.is_empty() && !instances.is_empty(),
                "available collision requires meshes and instances"
            );
            (meshes.as_slice(), instances.as_slice(), "available")
        }
        StaticCollisionScene::AvailableEmpty => (&[][..], &[][..], "available-empty"),
    };
    ensure!(
        meshes.len() <= limits.max_resources
            && instances.len() <= limits.max_resources
            && limits.max_resources >= 1,
        "collision collection limit exceeded"
    );
    // Charge every declared resource and logical payload before scans, preparation or copies.
    let (mut vertices, mut indices, mut primitives, mut geometry) = (0, 0, 0, 0);
    for mesh in meshes {
        ensure!(
            !mesh.positions.is_empty() && !mesh.primitives.is_empty(),
            "empty collision mesh"
        );
        charge(
            &mut vertices,
            mesh.positions.len(),
            limits.max_vertices,
            "vertices",
        )?;
        charge(
            &mut primitives,
            mesh.primitives.len(),
            limits.max_resources,
            "primitives",
        )?;
        charge(
            &mut geometry,
            mul(mesh.positions.len(), 12)?,
            limits.max_geometry_bytes,
            "geometry bytes",
        )?;
        for primitive in &mesh.primitives {
            ensure!(
                !primitive.indices.is_empty() && primitive.indices.len() % 3 == 0,
                "collision primitive requires triangles"
            );
            charge(
                &mut indices,
                primitive.indices.len(),
                limits.max_indices,
                "indices",
            )?;
            charge(
                &mut geometry,
                mul(primitive.indices.len(), 4)?,
                limits.max_geometry_bytes,
                "geometry bytes",
            )?;
        }
    }
    let resources = add(meshes.len(), primitives)?;
    ensure!(
        resources <= limits.max_resources,
        "collision accessor/view limit exceeded"
    );
    // Even a larger caller budget cannot exceed the GLB 2 u32 container sizes.
    let minimum_glb = add(if meshes.is_empty() { 20 } else { 28 }, geometry)?;
    ensure!(
        minimum_glb <= limits.max_glb_bytes && minimum_glb <= u32::MAX as usize,
        "collision GLB byte limit exceeded"
    );
    let mut expanded = 0;
    let mut world = 0;
    for instance in instances {
        ensure!(
            token(&instance.owner_id),
            "invalid collision owner identity"
        );
        let mesh = meshes
            .get(instance.mesh_index)
            .context("collision instance mesh out of bounds")?;
        let count = mesh
            .primitives
            .iter()
            .try_fold(0, |n, p| add(n, p.indices.len() / 3))?;
        charge(
            &mut expanded,
            count,
            limits.max_expanded_triangles,
            "expanded triangles",
        )?;
        charge(
            &mut world,
            mul(count, 3)?,
            limits.max_world_vertices,
            "world vertices",
        )?;
    }
    let mut owners = BTreeSet::new();
    for instance in instances {
        ensure!(
            owners.insert(instance.owner_id.as_str()),
            "duplicate collision owner identity"
        );
    }
    for mesh in meshes {
        ensure!(
            mesh.positions.iter().flatten().all(|p| p.is_finite()),
            "nonfinite collision position"
        );
        for primitive in &mesh.primitives {
            for t in primitive.indices.chunks_exact(3) {
                let mut p = [[0.; 3]; 3];
                for (i, &index) in t.iter().enumerate() {
                    p[i] = *mesh
                        .positions
                        .get(index as usize)
                        .context("collision index out of bounds")?;
                }
                triangle(p)?;
            }
        }
    }
    for instance in instances {
        validate_matrix(instance.matrix).context("unsupported collision transform")?;
        let source = Mat4::from_cols_array_2d(&instance.matrix);
        let asset = Mat4::from_cols_array_2d(&convert_matrix(instance.matrix));
        ensure!(asset.is_finite(), "nonfinite converted collision matrix");
        for primitive in &meshes[instance.mesh_index].primitives {
            for t in primitive.indices.chunks_exact(3) {
                let mut source_points = [[0.; 3]; 3];
                let mut asset_points = [[0.; 3]; 3];
                for (i, &index) in t.iter().enumerate() {
                    let p = meshes[instance.mesh_index].positions[index as usize];
                    source_points[i] = source.transform_point3(Vec3::from_array(p)).to_array();
                    asset_points[i] = asset
                        .transform_point3(Vec3::from_array(map_axes(p)))
                        .to_array();
                }
                triangle(source_points)?;
                triangle(asset_points)?;
            }
        }
    }
    let mut document = json!({"asset":{"version":"2.0","generator":"eqoxide-static-collision-v1"},"scene":0,"scenes":[{}],"extras":{
        "eqoxideAsset":{"schemaVersion":1,"role":"collision","coordinateProfile":"eqoxide-static-y-up-v1","unitScale":1,"bakeRevision":bake_revision,"requirements":requirements},
        "eqoxideCollision":{"version":1,"scope":"declared-static-triangle-candidates-v1","availability":availability,"selectedNodes":[],"queryPolicy":policy.identifier()}
    }});
    let mut binary = Vec::with_capacity(geometry);
    if !meshes.is_empty() {
        let mut accessors = Vec::with_capacity(resources);
        let mut views = Vec::with_capacity(resources);
        let mut output_meshes = Vec::with_capacity(meshes.len());
        for mesh in meshes {
            let start = binary.len();
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for &p in &mesh.positions {
                let p = map_axes(p);
                for a in 0..3 {
                    min[a] = min[a].min(p[a]);
                    max[a] = max[a].max(p[a]);
                    binary.extend_from_slice(&p[a].to_le_bytes());
                }
            }
            let position = accessors.len();
            views.push(json!({"buffer":0,"byteOffset":start,"byteLength":binary.len()-start,"target":34962}));
            accessors.push(json!({"bufferView":views.len()-1,"componentType":5126,"count":mesh.positions.len(),"type":"VEC3","min":min,"max":max}));
            let mut output_primitives = Vec::with_capacity(mesh.primitives.len());
            for primitive in &mesh.primitives {
                let start = binary.len();
                for i in &primitive.indices {
                    binary.extend_from_slice(&i.to_le_bytes());
                }
                let index = accessors.len();
                views.push(json!({"buffer":0,"byteOffset":start,"byteLength":binary.len()-start,"target":34963}));
                accessors.push(json!({"bufferView":views.len()-1,"componentType":5125,"count":primitive.indices.len(),"type":"SCALAR"}));
                output_primitives
                    .push(json!({"attributes":{"POSITION":position},"indices":index,"mode":4}));
            }
            output_meshes.push(json!({"primitives":output_primitives}));
        }
        let nodes: Vec<_> = instances.iter().map(|i| json!({"mesh":i.mesh_index,"matrix":Mat4::from_cols_array_2d(&convert_matrix(i.matrix)).to_cols_array(),"extras":{"eqoxideCollisionInstance":{"ownerId":i.owner_id}}})).collect();
        let selected: Vec<_> = (0..nodes.len()).collect();
        document["scenes"] = json!([{"nodes":selected}]);
        document["extras"]["eqoxideCollision"]["selectedNodes"] = json!(selected);
        document["nodes"] = json!(nodes);
        document["meshes"] = json!(output_meshes);
        document["accessors"] = json!(accessors);
        document["bufferViews"] = json!(views);
        document["buffers"] = json!([{"byteLength":binary.len()}]);
    }
    let mut json_bytes = BoundedJson {
        bytes: Vec::new(),
        limit: limits.max_json_bytes,
    };
    serde_json::to_writer(&mut json_bytes, &document)
        .context("serialize bounded collision JSON")?;
    let json_length = aligned(json_bytes.bytes.len())?;
    ensure!(
        json_length <= limits.max_json_bytes && json_length <= u32::MAX as usize,
        "collision JSON byte limit exceeded"
    );
    let total = add(
        add(20, json_length)?,
        if binary.is_empty() {
            0
        } else {
            add(8, aligned(binary.len())?)?
        },
    )?;
    ensure!(
        total <= limits.max_glb_bytes && total <= u32::MAX as usize,
        "collision GLB byte limit exceeded"
    );
    json_bytes.bytes.resize(json_length, b' ');
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp =
        tempfile::NamedTempFile::new_in(parent).context("create temporary collision artifact")?;
    temp.write_all(b"glTF")?;
    for value in [2, total as u32, json_length as u32, 0x4e4f534a] {
        temp.write_all(&value.to_le_bytes())?;
    }
    temp.write_all(&json_bytes.bytes)?;
    if !binary.is_empty() {
        temp.write_all(&(binary.len() as u32).to_le_bytes())?;
        temp.write_all(&0x004e4942u32.to_le_bytes())?;
        temp.write_all(&binary)?;
    }
    publish_static(temp, output, "collision artifact")?;
    Ok(requirements)
}
struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| io::Error::other("collision JSON byte limit exceeded"))?;
        self.bytes.extend_from_slice(bytes);
        debug_assert_eq!(self.bytes.len(), end);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_size_helpers_refuse_overflow() {
        assert!(add(usize::MAX, 1).is_err());
        assert!(mul(usize::MAX, 12).is_err());
        assert!(aligned(usize::MAX).is_err());
        assert!(charge(&mut usize::MAX, 1, usize::MAX, "test").is_err());
    }
}
