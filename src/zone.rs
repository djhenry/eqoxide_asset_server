//! Zone baking: terrain + object placements → world-space meshes, exported as glb.
use anyhow::Context;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct ZoneMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub texture_name: Option<String>,
}

/// Strip `_DMSPRITEDEF`/`_ACTORDEF`/`_DMSPRITE`/`_DEF` suffixes and uppercase the result.
pub fn object_base_name(n: &str) -> String {
    let u = n.to_uppercase();
    for suf in ["_DMSPRITEDEF", "_ACTORDEF", "_DMSPRITE", "_DEF"] {
        if let Some(s) = u.strip_suffix(suf) { return s.to_string(); }
    }
    u
}

/// Apply a world-space transform (translate + rotate about Y + uniform scale) to a model mesh.
pub fn place_instance(model: &ZoneMesh, center: (f32, f32, f32), rot_z_deg: f32, scale: f32) -> ZoneMesh {
    let (px, py, pz) = center;
    let (sin, cos) = rot_z_deg.to_radians().sin_cos();
    let positions = model.positions.iter().map(|v| {
        let (x, y, z) = (v[0] * scale, v[1] * scale, v[2] * scale);
        [x * cos + z * sin + px, y + py, -x * sin + z * cos + pz]
    }).collect();
    let normals = model.normals.iter()
        .map(|n| [n[0] * cos + n[2] * sin, n[1], -n[0] * sin + n[2] * cos])
        .collect();
    ZoneMesh {
        positions, normals,
        uvs: model.uvs.clone(),
        indices: model.indices.clone(),
        texture_name: model.texture_name.clone(),
    }
}

/// Read object models from `obj_s3d` and place each instance from `main_s3d`'s ActorInstances.
pub fn load_placed_objects(main_s3d: &Path, obj_s3d: &Path) -> anyhow::Result<Vec<ZoneMesh>> {
    // 1. Object models from _obj.s3d, keyed by base name (vertices include mesh.center).
    let obj_file = std::fs::File::open(obj_s3d).with_context(|| format!("open {}", obj_s3d.display()))?;
    let mut obj_pfs = libeq_pfs::PfsReader::open(obj_file)?;
    let obj_names: Vec<String> = obj_pfs.filenames()?;
    let mut models: HashMap<String, Vec<ZoneMesh>> = HashMap::new();
    for wn in obj_names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let bytes = match obj_pfs.get(wn) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => { eprintln!("zone: failed to read {wn}: {e}"); continue; }
        };
        let wld = match libeq_wld::load(&bytes) { Ok(w) => w, Err(_) => continue };
        for mesh in wld.meshes() {
            let base = match mesh.name() { Some(n) => object_base_name(n), None => continue };
            let all_pos = mesh.positions();
            if all_pos.is_empty() { continue; }
            let (cx, cy, cz) = mesh.center();
            let all_nrm = mesh.normals();
            let all_uv = mesh.texture_coordinates();
            for prim in mesh.primitives() {
                let idx: Vec<u32> = prim.indices();
                if idx.is_empty() { continue; }
                let positions = idx.iter().map(|&i| { let p = all_pos[i as usize]; [p[0]+cx, p[1]+cy, p[2]+cz] }).collect();
                let normals = idx.iter().map(|&i| all_nrm.get(i as usize).copied().unwrap_or([0.0,0.0,1.0])).collect();
                let uvs = idx.iter().map(|&i| all_uv.get(i as usize).copied().unwrap_or([0.0,0.0])).collect();
                let texture_name = prim.material().base_color_texture().and_then(|t| t.source());
                models.entry(base.clone()).or_default().push(ZoneMesh {
                    positions, normals, uvs, indices: (0..idx.len() as u32).collect(), texture_name,
                });
            }
        }
    }

    // 2. Placements from main .wld objects()
    let main_file = std::fs::File::open(main_s3d).with_context(|| format!("open {}", main_s3d.display()))?;
    let mut main_pfs = libeq_pfs::PfsReader::open(main_file)?;
    let main_names: Vec<String> = main_pfs.filenames()?;
    let mut placed = Vec::new();
    for wn in main_names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let bytes = match main_pfs.get(wn) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => { eprintln!("zone: failed to read {wn}: {e}"); continue; }
        };
        let wld = match libeq_wld::load(&bytes) { Ok(w) => w, Err(_) => continue };
        for obj in wld.objects() {
            let base = match obj.model_name() { Some(n) => object_base_name(n), None => continue };
            let Some(meshes) = models.get(&base) else { continue };
            let (px, py, pz) = obj.center();
            let (_rx, rz, _ry) = obj.rotation();
            let (s_xz, s_y) = obj.scale();
            let scale = if s_y > 0.01 { s_y } else if s_xz > 0.01 { s_xz } else { 1.0 };
            for m in meshes { placed.push(place_instance(m, (px, py, pz), rz, scale)); }
        }
    }
    Ok(placed)
}

/// Deduplicates identical `(position, normal, uv)` vertices (keyed by `f32::to_bits`,
/// exact/lossless) and rebuilds the index buffer, preserving `texture_name`.
pub fn weld(mesh: &ZoneMesh) -> ZoneMesh {
    use std::collections::HashMap;
    // key on the bit patterns of position+normal+uv (exact, lossless)
    let mut map: HashMap<[u32; 8], u32> = HashMap::new();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::with_capacity(mesh.indices.len());
    for &i in &mesh.indices {
        let i = i as usize;
        let p = mesh.positions[i];
        let n = mesh.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
        let u = mesh.uvs.get(i).copied().unwrap_or([0.0, 0.0]);
        let key = [
            p[0].to_bits(), p[1].to_bits(), p[2].to_bits(),
            n[0].to_bits(), n[1].to_bits(), n[2].to_bits(),
            u[0].to_bits(), u[1].to_bits(),
        ];
        let idx = *map.entry(key).or_insert_with(|| {
            positions.push(p); normals.push(n); uvs.push(u);
            (positions.len() - 1) as u32
        });
        indices.push(idx);
    }
    ZoneMesh { positions, normals, uvs, indices, texture_name: mesh.texture_name.clone() }
}

/// Extract terrain meshes from a zone's main `.s3d`, keeping texture names, in
/// raw libeq coordinates. Mirrors the non-skinned mesh loop in
/// `convert::convert_s3d_to_glb` (src/convert/mod.rs ~lines 126-207): walk
/// `wld.meshes()`, flatten per-primitive indices, pick the base-color texture
/// source. Zone terrain has no skin groups, so the bind-pose posing path used
/// by the character converter does not apply here.
fn load_terrain(main_s3d: &Path) -> anyhow::Result<Vec<ZoneMesh>> {
    let file = std::fs::File::open(main_s3d).with_context(|| format!("open {}", main_s3d.display()))?;
    let mut pfs = libeq_pfs::PfsReader::open(file)
        .with_context(|| format!("parse PFS {}", main_s3d.display()))?;
    let names: Vec<String> = pfs.filenames()?;
    let mut out = Vec::new();
    for wn in names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let bytes = match pfs.get(wn) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => { eprintln!("zone: failed to read {wn}: {e}"); continue; }
        };
        let wld = match libeq_wld::load(&bytes) { Ok(w) => w, Err(_) => continue };
        for mesh in wld.meshes() {
            let all_pos = mesh.positions();
            if all_pos.is_empty() { continue; }
            let all_nrm = mesh.normals();
            let all_uv = mesh.texture_coordinates();
            for prim in mesh.primitives() {
                let idx: Vec<u32> = prim.indices();
                if idx.is_empty() { continue; }
                let positions = idx.iter().map(|&i| all_pos[i as usize]).collect();
                let normals = idx.iter().map(|&i| all_nrm.get(i as usize).copied().unwrap_or([0.0, 0.0, 1.0])).collect();
                let uvs = idx.iter().map(|&i| all_uv.get(i as usize).copied().unwrap_or([0.0, 0.0])).collect();
                let texture_name = prim.material().base_color_texture().and_then(|t| t.source());
                out.push(ZoneMesh {
                    positions, normals, uvs,
                    indices: (0..idx.len() as u32).collect(),
                    texture_name,
                });
            }
        }
    }
    Ok(out)
}

/// Bake a zone into a single glb: terrain from `main_s3d` plus placed objects
/// from `obj_s3d` (when present). Positions stay in raw libeq space (no
/// re-orientation). Each distinct EQ texture name becomes one named glTF
/// material+image, decoded from whichever archive contains it. Reuses
/// `convert::write_glb` and `convert::load_texture_from_archive`.
pub fn bake_zone(main_s3d: &Path, obj_s3d: Option<&Path>, output_glb: &Path) -> anyhow::Result<()> {
    use crate::convert::{load_texture_from_archive, write_glb, MaterialData, MeshData, PrimitiveData, TextureData};

    // Gather every mesh: terrain + placed objects, all in raw libeq coords.
    let mut meshes = load_terrain(main_s3d)?;
    if let Some(obj) = obj_s3d {
        meshes.extend(load_placed_objects(main_s3d, obj)?);
    }
    if meshes.is_empty() {
        anyhow::bail!("no zone meshes found in {}", main_s3d.display());
    }

    // Open archives once for texture decode (textures may live in either).
    let mut pfs_list: Vec<libeq_pfs::PfsReader<std::fs::File>> = Vec::new();
    pfs_list.push(libeq_pfs::PfsReader::open(
        std::fs::File::open(main_s3d).with_context(|| format!("open {}", main_s3d.display()))?,
    )?);
    if let Some(obj) = obj_s3d {
        pfs_list.push(libeq_pfs::PfsReader::open(
            std::fs::File::open(obj).with_context(|| format!("open {}", obj.display()))?,
        )?);
    }

    // Merge all meshes into one glTF mesh with per-mesh primitives, one named
    // material per distinct texture name.
    let mut merged_positions: Vec<[f32; 3]> = Vec::new();
    let mut merged_normals: Vec<[f32; 3]> = Vec::new();
    let mut merged_uvs: Vec<[f32; 2]> = Vec::new();
    let mut primitives: Vec<PrimitiveData> = Vec::new();
    let mut materials: Vec<MaterialData> = Vec::new();
    let mut textures: Vec<TextureData> = Vec::new();
    let mut tex_map: HashMap<String, usize> = HashMap::new(); // tex name -> texture idx
    let mut mat_map: HashMap<String, usize> = HashMap::new(); // tex name -> material idx

    for m in &meshes {
        if m.positions.is_empty() || m.indices.is_empty() { continue; }
        let offset = merged_positions.len() as u32;
        merged_positions.extend_from_slice(&m.positions);
        merged_normals.extend_from_slice(&m.normals);
        merged_uvs.extend_from_slice(&m.uvs);
        let indices: Vec<u32> = m.indices.iter().map(|&i| i + offset).collect();

        let key = m.texture_name.clone().unwrap_or_else(|| "untextured".to_string());
        let material_idx = match mat_map.get(&key) {
            Some(&idx) => idx,
            None => {
                let texture_idx = if let Some(src) = &m.texture_name {
                    match tex_map.get(src) {
                        Some(&t) => Some(t),
                        None => {
                            let lower = src.to_lowercase();
                            let png = pfs_list.iter_mut()
                                .find_map(|pfs| load_texture_from_archive(pfs, &lower));
                            match png {
                                Some(png_bytes) => {
                                    let t = textures.len();
                                    textures.push(TextureData { name: lower, png_bytes });
                                    tex_map.insert(src.clone(), t);
                                    Some(t)
                                }
                                None => None,
                            }
                        }
                    }
                } else {
                    None
                };
                let idx = materials.len();
                materials.push(MaterialData {
                    name: m.texture_name.clone().unwrap_or_else(|| "untextured".to_string()),
                    texture_idx,
                    base_color: [1.0, 1.0, 1.0, 1.0],
                });
                mat_map.insert(key.clone(), idx);
                idx
            }
        };
        primitives.push(PrimitiveData { indices, material_idx });
    }

    if primitives.is_empty() {
        anyhow::bail!("no renderable primitives for {}", main_s3d.display());
    }

    let mesh = vec![MeshData {
        name: "zone".to_string(),
        positions: merged_positions,
        normals: merged_normals,
        uvs: merged_uvs,
        primitives,
    }];
    write_glb(output_glb, &mesh, &materials, &textures)
}

#[cfg(test)]
mod weld_tests {
    use super::*;
    #[test]
    fn welds_duplicate_vertices() {
        // A de-indexed quad: 2 triangles, 6 vertices, but only 4 are distinct.
        let v = |x: f32, z: f32| [x, 0.0, z];
        let m = ZoneMesh {
            positions: vec![v(0.,0.), v(1.,0.), v(1.,1.),   v(0.,0.), v(1.,1.), v(0.,1.)],
            normals:   vec![[0.,1.,0.]; 6],
            uvs:       vec![[0.,0.]; 6],
            indices:   vec![0,1,2,3,4,5],
            texture_name: Some("floor.bmp".into()),
        };
        let w = weld(&m);
        assert_eq!(w.positions.len(), 4, "4 distinct corners");
        assert_eq!(w.indices.len(), 6, "still 6 indices (2 triangles)");
        // reconstructed triangles equal the original positions
        let recon: Vec<[f32;3]> = w.indices.iter().map(|&i| w.positions[i as usize]).collect();
        assert_eq!(recon, m.positions);
        assert_eq!(w.texture_name.as_deref(), Some("floor.bmp"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_instance_translates_and_rotates() {
        let model = ZoneMesh {
            positions: vec![[1.0, 0.0, 0.0]],
            normals: vec![[1.0, 0.0, 0.0]],
            uvs: vec![[0.0, 0.0]],
            indices: vec![0],
            texture_name: Some("wall.bmp".into()),
        };
        // 90° about up at origin offset (10, 5, 20): x=1 → [cos90*1 + 0, 0+5, -sin90*1 + 20]
        let out = place_instance(&model, (10.0, 5.0, 20.0), 90.0, 1.0);
        let p = out.positions[0];
        assert!((p[0] - 10.0).abs() < 1e-4, "x={}", p[0]);   // cos90≈0 → 0 + 10
        assert!((p[1] - 5.0).abs() < 1e-4, "y={}", p[1]);    // y + py
        assert!((p[2] - 19.0).abs() < 1e-4, "z={}", p[2]);   // -sin90*1 + 20 = -1 + 20
        assert_eq!(out.texture_name.as_deref(), Some("wall.bmp"));
    }

    #[test]
    #[ignore = "requires ~/eq_assets/EQ_Files/qcat.s3d + qcat_obj.s3d"]
    fn load_placed_objects_places_off_origin() {
        let home = std::env::var("HOME").unwrap();
        let main = std::path::PathBuf::from(format!("{home}/eq_assets/EQ_Files/qcat.s3d"));
        let obj = std::path::PathBuf::from(format!("{home}/eq_assets/EQ_Files/qcat_obj.s3d"));
        if !main.exists() || !obj.exists() { eprintln!("skip: archives missing"); return; }
        let placed = load_placed_objects(&main, &obj).unwrap();
        assert!(!placed.is_empty(), "expected placed object meshes");
        // Not all vertices clustered at the origin (the "piled at 0,0,0" regression).
        let off_origin = placed.iter().flat_map(|m| &m.positions)
            .any(|p| p[0].abs() > 1.0 || p[2].abs() > 1.0);
        assert!(off_origin, "placed objects should not all be at the origin");
    }
}
