use super::{BinaryZoneScene, ZoneMesh, ZonePlacement};
use anyhow::{Context, Result, bail, ensure};
use glam::{Mat4, Vec3};
#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
use crate::convert::{
    AlphaMode, MaterialData, MeshData, NodeDef, PrimitiveData, TextureData, encode_texture_png,
    write_glb_instanced,
};
use libeq_eqg::mesh::MeshKind;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::Path,
};

#[derive(Debug, Serialize)]
pub struct SkippedPlacement {
    pub index: usize,
    pub reason: String,
}
#[derive(Debug, Serialize)]
pub struct ExportReport {
    pub meshes: usize,
    pub materials: usize,
    pub textures: usize,
    pub terrain_nodes: usize,
    pub instance_nodes: usize,
    /// Counted once per considered source mesh, not once per placement.
    pub omitted_triangles: usize,
    pub skipped_placements: Vec<SkippedPlacement>,
    pub untextured_materials: Vec<String>,
    pub omitted_meshes: Vec<String>,
    pub approximations: Vec<&'static str>,
}
fn convert(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[2], -v[1])
}
fn placement_matrix(p: &ZonePlacement) -> Result<Mat4> {
    ensure!(
        p.scale.is_finite() && p.scale > 0.,
        "zero, negative, or non-finite placement scale is unsupported"
    );
    ensure!(
        p.position.iter().chain(&p.rotation).all(|v| v.is_finite()),
        "non-finite placement transform"
    );
    let source = Mat4::from_translation(Vec3::from_array(p.position))
        * Mat4::from_rotation_z(p.rotation[0])
        * Mat4::from_rotation_y(p.rotation[1])
        * Mat4::from_rotation_x(p.rotation[2])
        * Mat4::from_scale(Vec3::splat(p.scale));
    // Exact signed permutation C(x,y,z)=(x,z,-y), including homogeneous axis.
    let a = source.to_cols_array_2d();
    let permutation = [0, 2, 1, 3];
    let signs = [1., 1., -1., 1.];
    let mut b = [[0.; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            b[col][row] = signs[col] * signs[row] * a[permutation[col]][permutation[row]];
        }
    }
    let m = Mat4::from_cols_array_2d(&b);
    ensure!(m.is_finite(), "placement matrix overflow");
    Ok(m)
}
fn validate_world_vertices(matrix: &Mat4, positions: &[[f32; 3]]) -> Result<()> {
    ensure!(
        positions
            .iter()
            .all(|p| matrix.transform_point3(Vec3::from_array(*p)).is_finite()),
        "non-finite or overflowing world-space position"
    );
    Ok(())
}
fn string(mesh: &ZoneMesh, offset: u32) -> Result<&str> {
    let bytes = mesh
        .string_table
        .get(offset as usize..)
        .context("invalid string offset")?;
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .context("unterminated mesh string")?;
    std::str::from_utf8(&bytes[..end]).context("mesh string is not UTF-8")
}
/// Write an isolated render preview. This does not publish baked assets or
/// establish collision, shader, lighting, or skeletal support.
pub fn export_preview(scene: &BinaryZoneScene, out: &Path) -> Result<ExportReport> {
    ensure!(
        matches!(scene.version, 1 | 2),
        "unsupported binary zone version"
    );
    let mut archive = libeq_pfs::PfsReader::open(File::open(&scene.archive_path)?)?;
    let mut members: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in archive.filenames()? {
        members
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push(name);
    }
    let terrains: Vec<_> = scene
        .meshes
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind == MeshKind::Terrain)
        .collect();
    ensure!(
        terrains.len() == 1,
        "preview requires exactly one distinct terrain mesh"
    );
    let (terrain_index, terrain) = terrains[0];
    let terrain_members: Vec<_> = members
        .iter()
        .filter(|(n, _)| n.ends_with(".ter"))
        .collect();
    ensure!(
        terrain_members.len() == 1
            && terrain_members[0].1.len() == 1
            && terrain_members[0].0 == &terrain.source_name.to_ascii_lowercase(),
        "preview requires exactly one matching terrain archive member"
    );
    for name in members.keys().filter(|n| n.ends_with(".ter")) {
        ensure!(
            !members.contains_key(&format!("{}.mod", &name[..name.len() - 4])),
            "terrain/model basename collision: {name}"
        );
    }
    let mut report = ExportReport {
        meshes: 0,
        materials: 0,
        textures: 0,
        terrain_nodes: 0,
        instance_nodes: 0,
        omitted_triangles: 0,
        skipped_placements: vec![],
        untextured_materials: vec![],
        omitted_meshes: vec![],
        approximations: vec![
            "Render preview only; no collision or manifest publication",
            "Vertex colors, secondary UVs, normal maps, triangle flags, and shader properties are omitted",
            "All materials are opaque; native alpha and blending are not reproduced",
            "Native lighting, regions, skeletal animation, and placement extension data are omitted",
        ],
    };
    let mut instances = Vec::new();
    let mut used = BTreeSet::from([terrain_index]);
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
        let slot = p.model_index.unwrap() as usize;
        let mesh_index = scene
            .model_slots
            .get(slot)
            .context("placement model slot out of range")?
            .context("placement references null model-table slot")?;
        ensure!(
            mesh_index < scene.meshes.len(),
            "placement mesh index out of range"
        );
        if mesh_index == terrain_index {
            report.skipped_placements.push(SkippedPlacement {
                index,
                reason: "terrain_emitted_once_at_identity".into(),
            });
            continue;
        }
        let matrix = placement_matrix(p).with_context(|| format!("placement {index}"))?;
        instances.push((index, mesh_index, matrix));
        used.insert(mesh_index);
    }
    let mut meshes = Vec::new();
    let mut materials = Vec::new();
    let mut textures = Vec::new();
    let mut texture_indices = BTreeMap::new();
    let mut mesh_indices = BTreeMap::new();
    for source_index in used {
        let source = &scene.meshes[source_index];
        let mut groups: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
        for tri in &source.triangles {
            if tri.material_index as usize >= source.materials.len() {
                report.omitted_triangles += 1;
                continue;
            }
            ensure!(
                tri.vertex_indices
                    .iter()
                    .all(|i| (*i as usize) < source.vertices.len()),
                "triangle vertex index out of range"
            );
            groups
                .entry(tri.material_index as usize)
                .or_default()
                .extend(tri.vertex_indices);
        }
        if groups.is_empty() {
            report.omitted_meshes.push(source.source_name.clone());
            continue;
        }
        let mut primitives = Vec::new();
        for (material_index, indices) in groups {
            let material = &source.materials[material_index];
            let name = format!(
                "{}:{}",
                source.source_name,
                string(source, material.name_offset)?
            );
            let mut diffuse = None;
            for property in &material.properties {
                if string(source, property.name_offset)?.eq_ignore_ascii_case("e_TextureDiffuse0") {
                    ensure!(
                        diffuse.is_none(),
                        "duplicate diffuse texture property in {name}"
                    );
                    diffuse = Some(property);
                }
            }
            if let Some(property) = diffuse {
                ensure!(
                    property.kind == 2,
                    "diffuse texture property is not a string in {name}"
                );
            }
            let texture_idx = if let Some(property) = diffuse {
                let requested = string(source, property.value)?;
                let exact = match members
                    .get(&requested.to_ascii_lowercase())
                    .map(Vec::as_slice)
                {
                    Some([exact]) => exact,
                    Some(_) => bail!("ambiguous diffuse texture {requested:?} in {name}"),
                    None => bail!("missing diffuse texture {requested:?} in {name}"),
                };
                let key = exact.to_ascii_lowercase();
                if let Some(&index) = texture_indices.get(&key) {
                    Some(index)
                } else {
                    let bytes = archive.get(exact)?.context("diffuse texture disappeared")?;
                    let png_bytes = encode_texture_png(&bytes, AlphaMode::Opaque, exact)
                        .with_context(|| format!("decode diffuse texture {exact:?} in {name}"))?;
                    let index = textures.len();
                    textures.push(TextureData {
                        name: key.clone(),
                        png_bytes,
                    });
                    texture_indices.insert(key, index);
                    Some(index)
                }
            } else {
                report.untextured_materials.push(name.clone());
                None
            };
            let material_idx = materials.len();
            materials.push(MaterialData {
                name,
                texture_idx,
                base_color: [1.; 4],
                alpha_mode: AlphaMode::Opaque,
                anim: None,
            });
            primitives.push(PrimitiveData {
                indices,
                material_idx,
                extras: None,
            });
        }
        let positions: Vec<_> = source
            .vertices
            .iter()
            .map(|v| convert(v.position).to_array())
            .collect();
        ensure!(
            source
                .vertices
                .iter()
                .all(|v| v.normal.iter().chain(&v.uv0).all(|f| f.is_finite())),
            "non-finite mesh normal or primary UV"
        );
        validate_world_vertices(&Mat4::IDENTITY, &positions)?;
        mesh_indices.insert(source_index, meshes.len());
        meshes.push(MeshData {
            name: source.source_name.clone(),
            positions,
            normals: source
                .vertices
                .iter()
                .map(|v| convert(v.normal).to_array())
                .collect(),
            uvs: source.vertices.iter().map(|v| v.uv0).collect(),
            primitives,
        });
    }
    ensure!(
        mesh_indices.contains_key(&terrain_index),
        "terrain contains no renderable triangles"
    );
    let mut nodes = Vec::new();
    if let Some(&mesh_idx) = mesh_indices.get(&terrain_index) {
        nodes.push(NodeDef {
            mesh_idx,
            matrix: None,
        });
        report.terrain_nodes = 1;
    }
    for (index, source_index, matrix) in instances {
        if let Some(&mesh_idx) = mesh_indices.get(&source_index) {
            validate_world_vertices(&matrix, &meshes[mesh_idx].positions)
                .with_context(|| format!("placement {index}"))?;
            nodes.push(NodeDef {
                mesh_idx,
                matrix: Some(matrix.to_cols_array_2d()),
            });
            report.instance_nodes += 1;
        } else {
            report.skipped_placements.push(SkippedPlacement {
                index,
                reason: "no_material_referenced_triangles".into(),
            });
        }
    }
    report.skipped_placements.sort_by_key(|p| p.index);
    report.meshes = meshes.len();
    report.materials = materials.len();
    report.textures = textures.len();
    ensure!(!nodes.is_empty(), "preview contains no renderable geometry");
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp =
        tempfile::NamedTempFile::new_in(parent).context("create sibling preview staging file")?;
    write_glb_instanced(temp.path(), &meshes, &materials, &textures, &nodes)?;
    temp.persist(out)
        .map_err(|e| e.error)
        .context("publish preview file")?;
    Ok(report)
}
