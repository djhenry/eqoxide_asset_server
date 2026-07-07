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
    /// Transparency mode from the EQ material's render method (foliage = Masked).
    pub alpha_mode: crate::convert::AlphaMode,
    /// EQ animated texture frames `(interval_ms, frame image names incl. frame 0)`;
    /// `None` for static textures. Captured from the libeq SimpleSpriteDef at load.
    pub anim: Option<(u32, Vec<String>)>,
}

/// EQ animated-texture frames for a material's base-color texture, if it is animated
/// (more than one frame). Returns `(interval_ms, lowercased frame image names)`.
/// libeq exposes the frame list via `iter_sources()` and the animated flag via `flags()`;
/// the per-frame interval (`SimpleSpriteDef.sleep`) isn't surfaced by the high-level API,
/// so we use EQ's classic ~100 ms/frame (≈10 fps) which matches fire/torch flicker.
fn texture_anim(tex: &libeq_wld::Texture<'_>) -> Option<(u32, Vec<String>)> {
    if !tex.flags().is_animated() {
        return None;
    }
    let frames: Vec<String> = tex.iter_sources().map(|s| s.to_lowercase()).collect();
    if frames.len() < 2 {
        return None;
    }
    Some((100, frames))
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
        alpha_mode: model.alpha_mode,
        anim: model.anim.clone(),
    }
}

/// Build the column-major 4x4 placement matrix that reproduces, for a
/// model-local vertex `v`, the exact world position that [`place_instance`]
/// computes by transforming vertices. With scaled vertex `(x,y,z) = scale*v`,
/// `place_instance` gives world `[x·cos+z·sin+px, y+py, -x·sin+z·cos+pz]`.
///
/// glTF node `matrix` is column-major: each inner `[f32;4]` is a *column*.
/// Columns are the images of the basis vectors:
///   col0 = M·x̂ = [ s·cos,  0, -s·sin, 0]
///   col1 = M·ŷ = [     0,  s,      0, 0]   (up = Y, libeq index 1)
///   col2 = M·ẑ = [ s·sin,  0,  s·cos, 0]
///   col3 = translation = [px, py, pz, 1]
/// so `M * [v,1]` = `[s·cos·vx + s·sin·vz + px,  s·vy + py,  -s·sin·vx + s·cos·vz + pz]`,
/// matching `place_instance` for x=s·vx, y=s·vy, z=s·vz.
pub fn placement_matrix(center: (f32, f32, f32), rot_z_deg: f32, scale: f32) -> [[f32; 4]; 4] {
    let (px, py, pz) = center;
    let (sin, cos) = rot_z_deg.to_radians().sin_cos();
    let s = scale;
    [
        [s * cos, 0.0, -s * sin, 0.0], // col0 (x basis)
        [0.0, s, 0.0, 0.0],            // col1 (y basis / up)
        [s * sin, 0.0, s * cos, 0.0],  // col2 (z basis)
        [px, py, pz, 1.0],             // col3 (translation)
    ]
}

/// Load object models from `obj_s3d` as welded, model-LOCAL meshes keyed by base
/// name. Vertices include `mesh.center()` (so the model is self-contained) but
/// carry no placement transform — placements are applied per-node via matrices.
///
/// `main_s3d`, when given, is consulted FIRST by the texture-container sniff (V
/// convention), matching [`bake_zone`]'s embed order `[main, obj]` — a texture
/// present only in the sibling main archive still gets the right V treatment.
/// Pass `None` when textures will be embedded from `obj_s3d` alone (e.g.
/// [`bake_object_models_glb`]).
pub fn load_object_models(
    obj_s3d: &Path,
    main_s3d: Option<&Path>,
) -> anyhow::Result<HashMap<String, Vec<ZoneMesh>>> {
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
            for zm in zone_meshes_from_mesh(&mesh) {
                models.entry(base.clone()).or_default().push(weld(&zm));
            }
        }
    }
    let mut pfs_list: Vec<libeq_pfs::PfsReader<std::fs::File>> = Vec::new();
    if let Some(main) = main_s3d {
        pfs_list.push(libeq_pfs::PfsReader::open(
            std::fs::File::open(main).with_context(|| format!("open {}", main.display()))?,
        )?);
    }
    pfs_list.push(obj_pfs);
    apply_gltf_v_convention(models.values_mut().flatten(), &mut pfs_list);
    Ok(models)
}

/// Read object placements from `main_s3d`'s WLD objects: `(base_name, matrix)`
/// where `matrix` is the column-major 4x4 from [`placement_matrix`] (same
/// scale/rotate-about-up/translate as [`place_instance`], built rather than
/// baked into vertices).
pub fn read_placements(main_s3d: &Path) -> anyhow::Result<Vec<(String, [[f32; 4]; 4])>> {
    let main_file = std::fs::File::open(main_s3d).with_context(|| format!("open {}", main_s3d.display()))?;
    let mut main_pfs = libeq_pfs::PfsReader::open(main_file)?;
    let main_names: Vec<String> = main_pfs.filenames()?;
    let mut placements = Vec::new();
    for wn in main_names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let bytes = match main_pfs.get(wn) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => { eprintln!("zone: failed to read {wn}: {e}"); continue; }
        };
        let wld = match libeq_wld::load(&bytes) { Ok(w) => w, Err(_) => continue };
        for obj in wld.objects() {
            let base = match obj.model_name() { Some(n) => object_base_name(n), None => continue };
            let (px, py, pz) = obj.center();
            let (_rx, rz, _ry) = obj.rotation();
            let (s_xz, s_y) = obj.scale();
            let scale = if s_y > 0.01 { s_y } else if s_xz > 0.01 { s_xz } else { 1.0 };
            placements.push((base, placement_matrix((px, py, pz), rz, scale)));
        }
    }
    Ok(placements)
}

/// True when the named texture's on-disk file decodes to a standard, visually-upright
/// raster (row 0 = visual top): real BMPs (the `image` crate un-flips their bottom-up
/// rows) and PNGs. False for EQ's repacked DDS files, which keep the original
/// bottom-up (GL) row order and therefore decode vertically MIRRORED.
///
/// Resolution mirrors `convert::load_texture_from_archive`: try the name as-is, then
/// the stem with `.dds`/`.bmp`/`.png`, across the given archives in order, skipping
/// candidates that exist but don't decode (so the verdict matches the image the
/// embed step actually picks). `None` when nothing is found/decodable anywhere
/// (caller leaves UVs unchanged; the material decode will fail the same way).
fn texture_decodes_upright(
    pfs_list: &mut [libeq_pfs::PfsReader<std::fs::File>],
    name: &str,
) -> Option<bool> {
    let lower = name.to_lowercase();
    let stem = if lower.ends_with(".dds") || lower.ends_with(".bmp") || lower.ends_with(".png") {
        lower[..lower.len() - 4].to_string()
    } else {
        lower.clone()
    };
    let mut candidates = vec![lower];
    for ext in [".dds", ".bmp", ".png"] {
        let c = format!("{stem}{ext}");
        if !candidates.contains(&c) { candidates.push(c); }
    }
    for pfs in pfs_list.iter_mut() {
        for cand in &candidates {
            if let Ok(Some(data)) = pfs.get(cand) {
                // Present but undecodable files must fall through to the next
                // candidate, exactly like `convert::try_load_image` does, so the
                // verdict is for the image that actually gets embedded.
                if image::load_from_memory(&data).is_err() {
                    continue;
                }
                let is_dds = data.len() >= 4 && &data[0..4] == b"DDS ";
                return Some(!is_dds);
            }
        }
    }
    None
}

/// Convert raw WLD UVs to glTF convention, per texture container.
///
/// EQ WLD texture coordinates are bottom-origin (V increases with world height,
/// OpenGL-style), uniformly across eras — verified against fire sprite cards, whose
/// vertical orientation is unambiguous: in every zone the card's V increases from the
/// flame's geometric base to its tip. glTF V is top-origin, so V must be flipped
/// (`v -> 1 - v`) to sample a visually-upright image correctly.
///
/// EQ's DDS repacks (RoF2-era archives; the files keep their original `.bmp` names)
/// were converted WITHOUT reordering rows, so they decode vertically mirrored — which
/// exactly cancels the needed flip under REPEAT sampling (`frac(1 - (1 - v)) ==
/// frac(v)`). Hence: flip V for textures stored upright (real BMP/PNG), keep raw WLD
/// V for mirrored DDS. This replaces the old animated-sprites-only flip, which was
/// container-blind: right for BMP-era zones (highpasshold), upside-down for DDS zones
/// (issue eqoxide#160, the kaladimb Mining Guild campfire), and it left static
/// BMP-era textures latently flipped.
pub(crate) fn apply_gltf_v_convention<'a>(
    meshes: impl Iterator<Item = &'a mut ZoneMesh>,
    pfs_list: &mut [libeq_pfs::PfsReader<std::fs::File>],
) {
    let mut cache: HashMap<String, Option<bool>> = HashMap::new();
    for m in meshes {
        let Some(tex) = m.texture_name.clone() else { continue };
        let upright = *cache
            .entry(tex.to_lowercase())
            .or_insert_with(|| texture_decodes_upright(pfs_list, &tex));
        if upright == Some(true) {
            for uv in &mut m.uvs {
                uv[1] = 1.0 - uv[1];
            }
        }
    }
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
    ZoneMesh { positions, normals, uvs, indices, texture_name: mesh.texture_name.clone(), alpha_mode: mesh.alpha_mode, anim: mesh.anim.clone() }
}

/// Extract one [`ZoneMesh`] per primitive from a WLD mesh, folding the mesh center
/// into positions. Both [`load_terrain`] and [`load_object_models`] use this helper
/// so the per-primitive extraction logic (center fold, bounds guard) lives in one
/// place. UVs are emitted RAW (WLD bottom-origin V); callers must run
/// [`apply_gltf_v_convention`] once per mesh set to fix up V per texture container.
pub(crate) fn zone_meshes_from_mesh(mesh: &libeq_wld::Mesh<'_>) -> Vec<ZoneMesh> {
    let all_pos = mesh.positions();
    if all_pos.is_empty() { return Vec::new(); }
    let (cx, cy, cz) = mesh.center();
    let all_nrm = mesh.normals();
    let all_uv = mesh.texture_coordinates();
    let mut out = Vec::new();
    for prim in mesh.primitives() {
        let idx: Vec<u32> = prim.indices();
        if idx.is_empty() { continue; }
        // Skip primitives whose indices exceed the position array (malformed WLD)
        // instead of panicking on the whole zone.
        if idx.iter().any(|&i| i as usize >= all_pos.len()) { continue; }
        // Invisible boundary geometry (zone sky-lids, underworld planes, invisible walls) is
        // never drawn by the native client — skip it from RENDER output. It remains SOLID in
        // `__collision__` via the separate flag-based `collision_indices()` path. Without this,
        // qeynos's boundary shell (809 prims, height −100..+100, render method 0) baked into
        // the render terrain as an opaque black box enclosing the whole city → the zone
        // appeared as a void from inside. (eqoxide#213)
        if crate::convert::is_invisible_render_method(prim.material().render_method()) {
            continue;
        }
        let positions = idx.iter()
            .map(|&i| { let p = all_pos[i as usize]; [p[0] + cx, p[1] + cy, p[2] + cz] })
            .collect();
        let normals = idx.iter()
            .map(|&i| all_nrm.get(i as usize).copied().unwrap_or([0.0, 0.0, 1.0]))
            .collect();
        let mat = prim.material();
        let tex = mat.base_color_texture();
        let texture_name = tex.as_ref().and_then(|t| t.source());
        let anim = tex.as_ref().and_then(texture_anim);
        let alpha_mode = crate::convert::alpha_mode_from_render(mat.render_method());
        let uvs = idx.iter()
            .map(|&i| all_uv.get(i as usize).copied().unwrap_or([0.0, 0.0]))
            .collect();
        out.push(ZoneMesh {
            positions, normals, uvs,
            indices: (0..idx.len() as u32).collect(),
            texture_name, alpha_mode, anim,
        });
    }
    out
}

/// Extract terrain meshes from a zone's main `.s3d`, keeping texture names, in
/// raw libeq coordinates. Mirrors the non-skinned mesh loop in
/// `convert::convert_s3d_to_glb` (src/convert/mod.rs ~lines 126-207): walk
/// `wld.meshes()`, flatten per-primitive indices, pick the base-color texture
/// source. Zone terrain has no skin groups, so the bind-pose posing path used
/// by the character converter does not apply here.
///
/// `obj_s3d`, when given, is consulted (after `main_s3d`) by the texture-container
/// sniff, matching [`bake_zone`]'s embed order `[main, obj]` — a terrain texture
/// stored only in the sibling object archive still gets the right V treatment.
fn load_terrain(main_s3d: &Path, obj_s3d: Option<&Path>) -> anyhow::Result<Vec<ZoneMesh>> {
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
            out.extend(zone_meshes_from_mesh(&mesh));
        }
    }
    let mut pfs_list: Vec<libeq_pfs::PfsReader<std::fs::File>> = vec![pfs];
    if let Some(obj) = obj_s3d {
        pfs_list.push(libeq_pfs::PfsReader::open(
            std::fs::File::open(obj).with_context(|| format!("open {}", obj.display()))?,
        )?);
    }
    apply_gltf_v_convention(out.iter_mut(), &mut pfs_list);
    // EQ zone WLDs split terrain into thousands of tiny primitives (qeynos: ~8500).
    // Emitting one glb mesh primitive per WLD primitive makes the client's from_glb
    // pathologically slow + memory-hungry (per-primitive glTF overhead). Merge all
    // terrain primitives that share a texture into one mesh — the renderer merges by
    // texture at upload anyway, so this is the same geometry in far fewer primitives.
    Ok(merge_by_texture(out))
}

/// Concatenate meshes that share a `texture_name` into one (offsetting indices).
/// Reduces a zone's terrain from thousands of tiny primitives to one-per-texture.
fn merge_by_texture(meshes: Vec<ZoneMesh>) -> Vec<ZoneMesh> {
    use std::collections::HashMap;
    // Group by (texture, alpha_mode): meshes sharing a texture but rendered with
    // different transparency must stay separate so each gets the right glTF material.
    let mut groups: HashMap<(Option<String>, crate::convert::AlphaMode), ZoneMesh> = HashMap::new();
    for m in meshes {
        let key = (m.texture_name.clone(), m.alpha_mode);
        let entry = groups.entry(key).or_insert_with(|| ZoneMesh {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            texture_name: m.texture_name.clone(),
            alpha_mode: m.alpha_mode,
            anim: m.anim.clone(),
        });
        let base = entry.positions.len() as u32;
        entry.positions.extend(m.positions);
        entry.normals.extend(m.normals);
        entry.uvs.extend(m.uvs);
        entry.indices.extend(m.indices.iter().map(|&i| i + base));
    }
    let mut result: Vec<ZoneMesh> = groups.into_values().collect();
    // Deterministic order: by texture name, then alpha mode.
    result.sort_by(|a, b| {
        a.texture_name
            .cmp(&b.texture_name)
            .then_with(|| format!("{:?}", a.alpha_mode).cmp(&format!("{:?}", b.alpha_mode)))
    });
    result
}

/// Collect the zone's SOLID collision geometry from its terrain WLD(s), in world-space libeq
/// coordinates (matching [`load_terrain`]). Uses libeq `Mesh::collision_indices()`, which keeps
/// every face whose flag bit 0x0010 is CLEAR — i.e. all SOLID faces, INCLUDING invisible-but-
/// solid ones (zone boundaries, invisible walls, doorframes) that have no render material, while
/// excluding PASSABLE faces (water surfaces, foliage; flag 0x0010 = "player can pass through",
/// per libeq `DmSpriteDef2FaceEntry`). Returns merged `(positions, indices)` referencing those
/// positions. The native client collides against this superset; eqoxide previously collided only
/// against the rendered terrain, so invisible walls were walk-through.
fn load_collision_geometry(main_s3d: &Path) -> anyhow::Result<(Vec<[f32; 3]>, Vec<u32>)> {
    let file = std::fs::File::open(main_s3d).with_context(|| format!("open {}", main_s3d.display()))?;
    let mut pfs = libeq_pfs::PfsReader::open(file)
        .with_context(|| format!("parse PFS {}", main_s3d.display()))?;
    let names: Vec<String> = pfs.filenames()?;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for wn in names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let bytes = match pfs.get(wn) {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let wld = match libeq_wld::load(&bytes) { Ok(w) => w, Err(_) => continue };
        for mesh in wld.meshes() {
            let all_pos = mesh.positions();
            if all_pos.is_empty() { continue; }
            let col = mesh.collision_indices();
            if col.is_empty() { continue; }
            let (cx, cy, cz) = mesh.center();
            let offset = positions.len() as u32;
            for p in &all_pos {
                positions.push([p[0] + cx, p[1] + cy, p[2] + cz]);
            }
            // Keep whole triangles only; drop any face referencing an out-of-range vertex.
            for tri in col.chunks_exact(3) {
                if tri.iter().any(|&i| i as usize >= all_pos.len()) { continue; }
                for &i in tri {
                    indices.push(i + offset);
                }
            }
        }
    }
    Ok((positions, indices))
}

/// Bake a zone into a single glb: terrain from `main_s3d` plus placed objects
/// from `obj_s3d` (when present). Positions stay in raw libeq space (no
/// re-orientation). Each distinct EQ texture name becomes one named glTF
/// material+image, decoded from whichever archive contains it. Reuses
/// `convert::write_glb` and `convert::load_texture_from_archive`.
pub fn bake_zone(main_s3d: &Path, obj_s3d: Option<&Path>, output_glb: &Path) -> anyhow::Result<()> {
    use crate::convert::{
        load_texture_from_archive, write_glb_instanced, MaterialData, MeshData, NodeDef,
        PrimitiveData, TextureData,
    };

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

    let mut materials: Vec<MaterialData> = Vec::new();
    let mut textures: Vec<TextureData> = Vec::new();
    let mut tex_map: HashMap<String, usize> = HashMap::new(); // tex name -> texture idx
    let mut mat_map: HashMap<String, usize> = HashMap::new(); // tex name -> material idx

    // Resolve a ZoneMesh's texture to a glTF material index (decoding+naming once).
    let mut material_for = |m: &ZoneMesh,
                            materials: &mut Vec<MaterialData>,
                            textures: &mut Vec<TextureData>,
                            tex_map: &mut HashMap<String, usize>,
                            mat_map: &mut HashMap<String, usize>,
                            pfs_list: &mut Vec<libeq_pfs::PfsReader<std::fs::File>>|
     -> usize {
        let tex_key = m.texture_name.clone().unwrap_or_else(|| "untextured".to_string());
        // Material + texture caches key on (texture, alpha_mode): same texture under
        // different transparency needs its own material (and its own decode: masked
        // keys out index 0, blend bakes opacity into alpha).
        let key = format!("{}\0{:?}", tex_key, m.alpha_mode);
        if let Some(&idx) = mat_map.get(&key) {
            return idx;
        }
        // Decode + cache one texture by name (keyed by alpha_mode so masked/blend
        // decodes don't collide with opaque). Returns its index in `textures`.
        let alpha_mode = m.alpha_mode;
        let decode = |name: &str,
                          textures: &mut Vec<TextureData>,
                          tex_map: &mut HashMap<String, usize>,
                          pfs_list: &mut Vec<libeq_pfs::PfsReader<std::fs::File>>| -> Option<usize> {
            let lower = name.to_lowercase();
            let cache_key = format!("{}\0{:?}", lower, alpha_mode);
            if let Some(&t) = tex_map.get(&cache_key) {
                return Some(t);
            }
            let png = pfs_list.iter_mut().find_map(|pfs| load_texture_from_archive(pfs, &lower, alpha_mode));
            png.map(|png_bytes| {
                let t = textures.len();
                textures.push(TextureData { name: lower, png_bytes });
                tex_map.insert(cache_key, t);
                t
            })
        };

        let texture_idx = m.texture_name.as_ref()
            .and_then(|src| decode(src, textures, tex_map, pfs_list));

        // Animated texture: decode every frame so all are present as glTF images, and
        // record the (interval, frame names) so the client can cycle them.
        let anim = m.anim.as_ref().map(|(ms, frames)| {
            for f in frames {
                decode(f, textures, tex_map, pfs_list);
            }
            (*ms, frames.clone())
        });

        let idx = materials.len();
        materials.push(MaterialData {
            name: tex_key,
            texture_idx,
            base_color: [1.0, 1.0, 1.0, 1.0],
            alpha_mode: m.alpha_mode,
            anim,
        });
        mat_map.insert(key, idx);
        idx
    };

    // Fold a group of ZoneMeshes (sharing a vertex pool) into one MeshData.
    let build_mesh = |name: String,
                      group: &[ZoneMesh],
                      materials: &mut Vec<MaterialData>,
                      textures: &mut Vec<TextureData>,
                      tex_map: &mut HashMap<String, usize>,
                      mat_map: &mut HashMap<String, usize>,
                      pfs_list: &mut Vec<libeq_pfs::PfsReader<std::fs::File>>,
                      material_for: &mut dyn FnMut(
        &ZoneMesh,
        &mut Vec<MaterialData>,
        &mut Vec<TextureData>,
        &mut HashMap<String, usize>,
        &mut HashMap<String, usize>,
        &mut Vec<libeq_pfs::PfsReader<std::fs::File>>,
    ) -> usize|
     -> Option<MeshData> {
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut uvs: Vec<[f32; 2]> = Vec::new();
        let mut primitives: Vec<PrimitiveData> = Vec::new();
        for m in group {
            if m.positions.is_empty() || m.indices.is_empty() { continue; }
            let offset = positions.len() as u32;
            positions.extend_from_slice(&m.positions);
            normals.extend_from_slice(&m.normals);
            uvs.extend_from_slice(&m.uvs);
            let indices: Vec<u32> = m.indices.iter().map(|&i| i + offset).collect();
            let material_idx = material_for(m, materials, textures, tex_map, mat_map, pfs_list);
            primitives.push(PrimitiveData { indices, material_idx, extras: None });
        }
        if primitives.is_empty() { return None; }
        Some(MeshData { name, positions, normals, uvs, primitives })
    };

    let mut meshes: Vec<MeshData> = Vec::new();
    let mut nodes: Vec<NodeDef> = Vec::new();

    // 1. Terrain: weld each mesh, one mesh per group, identity node.
    let terrain = load_terrain(main_s3d, obj_s3d)?;
    let welded_terrain: Vec<ZoneMesh> = terrain.iter().map(weld).collect();
    if let Some(md) = build_mesh(
        "terrain".to_string(), &welded_terrain,
        &mut materials, &mut textures, &mut tex_map, &mut mat_map, &mut pfs_list, &mut material_for,
    ) {
        let mesh_idx = meshes.len();
        meshes.push(md);
        nodes.push(NodeDef { mesh_idx, matrix: None });
    }

    // 1b. Collision geometry: a dedicated `__collision__` mesh holding every SOLID terrain face
    //     (incl. invisible-but-solid zone boundaries / invisible walls / doorframes; PASSABLE
    //     water+foliage excluded). The client skips drawing it and uses it for collision instead
    //     of the rendered terrain (back-compat: older zones without it fall back to the terrain).
    //     Positions-only; normals/uvs are placeholders. material_idx 0 is a valid reference but
    //     never rendered (the client routes this mesh to collision by its `__collision__` name).
    let (col_pos, col_idx) = load_collision_geometry(main_s3d)?;
    if !col_idx.is_empty() && !materials.is_empty() {
        let mesh_idx = meshes.len();
        meshes.push(MeshData {
            name: "__collision__".to_string(),
            normals: vec![[0.0, 0.0, 1.0]; col_pos.len()],
            uvs: vec![[0.0, 0.0]; col_pos.len()],
            primitives: vec![PrimitiveData { indices: col_idx, material_idx: 0, extras: None }],
            positions: col_pos,
        });
        nodes.push(NodeDef { mesh_idx, matrix: None });
    }

    // 2. Object models: one welded mesh per unique base that has a placement,
    //    plus one placement node per (base, matrix).
    if let Some(obj) = obj_s3d {
        let models = load_object_models(obj, Some(main_s3d))?; // already welded, model-local
        let placements = read_placements(main_s3d)?;
        let mut base_mesh_idx: HashMap<String, usize> = HashMap::new();
        for (base, matrix) in &placements {
            let mesh_idx = match base_mesh_idx.get(base) {
                Some(&i) => i,
                None => {
                    let Some(group) = models.get(base) else { continue };
                    let Some(md) = build_mesh(
                        base.clone(), group,
                        &mut materials, &mut textures, &mut tex_map, &mut mat_map, &mut pfs_list, &mut material_for,
                    ) else { continue };
                    let i = meshes.len();
                    meshes.push(md);
                    base_mesh_idx.insert(base.clone(), i);
                    i
                }
            };
            nodes.push(NodeDef { mesh_idx, matrix: Some(*matrix) });
        }
    }

    if meshes.is_empty() {
        anyhow::bail!("no zone meshes found in {}", main_s3d.display());
    }

    write_glb_instanced(output_glb, &meshes, &materials, &textures, &nodes)
}

/// Build material/texture/mesh/node data from a `HashMap<base, Vec<ZoneMesh>>` and
/// write a GLB with one identity-node mesh per object base name (UPPERCASE mesh name),
/// textures embedded. Returns `Ok(false)` without writing anything when there are no
/// non-empty meshes. Shared by [`bake_object_models_glb`] (doors) and future callers
/// (weapons, armor).
pub(crate) fn write_object_models_glb(
    models: HashMap<String, Vec<ZoneMesh>>,
    pfs: &mut Vec<libeq_pfs::PfsReader<std::fs::File>>,
    out_glb: &Path,
) -> anyhow::Result<bool> {
    use crate::convert::{
        load_texture_from_archive, write_glb_instanced, MaterialData, MeshData, NodeDef,
        PrimitiveData, TextureData,
    };

    let mut materials: Vec<MaterialData> = Vec::new();
    let mut textures: Vec<TextureData> = Vec::new();
    let mut tex_map: HashMap<String, usize> = HashMap::new();
    let mut mat_map: HashMap<String, usize> = HashMap::new();
    let mut meshes: Vec<MeshData> = Vec::new();
    let mut nodes: Vec<NodeDef> = Vec::new();

    // Resolve a ZoneMesh to a glTF material index, decoding+caching textures once.
    let material_for = |m: &ZoneMesh,
                        materials: &mut Vec<MaterialData>,
                        textures: &mut Vec<TextureData>,
                        tex_map: &mut HashMap<String, usize>,
                        mat_map: &mut HashMap<String, usize>,
                        pfs: &mut Vec<libeq_pfs::PfsReader<std::fs::File>>|
     -> usize {
        let tex_key = m.texture_name.clone().unwrap_or_else(|| "untextured".to_string());
        let key = format!("{}\0{:?}", tex_key, m.alpha_mode);
        if let Some(&i) = mat_map.get(&key) { return i; }
        let texture_idx = m.texture_name.as_ref().and_then(|name| {
            let lower = name.to_lowercase();
            let ck = format!("{}\0{:?}", lower, m.alpha_mode);
            if let Some(&t) = tex_map.get(&ck) { return Some(t); }
            let png = pfs.iter_mut().find_map(|p| load_texture_from_archive(p, &lower, m.alpha_mode));
            png.map(|b| {
                let t = textures.len();
                textures.push(TextureData { name: lower, png_bytes: b });
                tex_map.insert(ck, t);
                t
            })
        });
        let idx = materials.len();
        materials.push(MaterialData {
            name: tex_key,
            texture_idx,
            base_color: [1.0, 1.0, 1.0, 1.0],
            alpha_mode: m.alpha_mode,
            anim: m.anim.clone(),
        });
        mat_map.insert(key, idx);
        idx
    };

    // Deterministic order for reproducible GLBs.
    let mut bases: Vec<&String> = models.keys().collect();
    bases.sort();
    for base in bases {
        let group = &models[base];
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut uvs: Vec<[f32; 2]> = Vec::new();
        let mut primitives: Vec<PrimitiveData> = Vec::new();
        for zm in group {
            if zm.positions.is_empty() || zm.indices.is_empty() { continue; }
            let offset = positions.len() as u32;
            positions.extend_from_slice(&zm.positions);
            normals.extend_from_slice(&zm.normals);
            uvs.extend_from_slice(&zm.uvs);
            let indices: Vec<u32> = zm.indices.iter().map(|&i| i + offset).collect();
            let material_idx = material_for(zm, &mut materials, &mut textures, &mut tex_map, &mut mat_map, pfs);
            primitives.push(PrimitiveData { indices, material_idx, extras: None });
        }
        if primitives.is_empty() { continue; }
        let mesh_idx = meshes.len();
        meshes.push(MeshData { name: base.to_uppercase(), positions, normals, uvs, primitives });
        nodes.push(NodeDef { mesh_idx, matrix: None });
    }
    if meshes.is_empty() { return Ok(false); }
    write_glb_instanced(out_glb, &meshes, &materials, &textures, &nodes)?;
    Ok(true)
}

/// Bake a zone's object archive (`<zone>_obj.s3d`) into a GLB with one identity-node
/// mesh per object base name (UPPERCASE), textures embedded. Door placement/animation
/// is applied client-side from live door state, so no instance transforms are emitted.
/// Returns `Ok(false)` (writing nothing) when the archive has no object models.
pub fn bake_object_models_glb(obj_s3d: &Path, output_glb: &Path) -> anyhow::Result<bool> {
    let models = load_object_models(obj_s3d, None)?;
    if models.is_empty() { return Ok(false); }
    let mut pfs = vec![libeq_pfs::PfsReader::open(
        std::fs::File::open(obj_s3d).with_context(|| format!("open {}", obj_s3d.display()))?,
    )?];
    write_object_models_glb(models, &mut pfs, output_glb)
}

#[cfg(test)]
mod doors_glb_tests {
    use super::*;
    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/qcat_obj.s3d"]
    fn bakes_named_object_meshes_with_textures() {
        let home = std::env::var("HOME").unwrap();
        let obj = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/qcat_obj.s3d"));
        if !obj.exists() { eprintln!("skip: archive missing"); return; }
        let out = std::env::temp_dir().join("qcat_doors_test.glb");
        let wrote = bake_object_models_glb(&obj, &out).unwrap();
        assert!(wrote, "qcat_obj has object models");
        // Re-import: meshes are named (UPPERCASE base) and at least one links a texture.
        let (doc, _b, _i) = gltf::import(&out).unwrap();
        let named = doc.meshes().filter(|m| m.name().is_some()).count();
        assert!(named > 0, "every door mesh must be named by base");
        assert!(doc.images().count() > 0, "door textures must be embedded");
        assert!(doc.meshes().all(|m| m.name().map_or(false, |n| n == n.to_uppercase())),
            "mesh names must be uppercase base names");
    }
}

#[cfg(test)]
mod fire_orientation_tests {
    use super::*;

    /// Mean luminance of the embedded-texture row that glTF V coordinate `v`
    /// samples (REPEAT wrap), from the exact PNG bytes the bake embeds.
    fn row_luminance(img: &image::RgbaImage, v: f32) -> f64 {
        let (w, h) = img.dimensions();
        let row = ((v.rem_euclid(1.0) * h as f32) as u32).min(h - 1);
        let mut s = 0u64;
        for x in 0..w {
            let p = img.get_pixel(x, row);
            s += (p[0] as u64 + p[1] as u64 + p[2] as u64) / 3;
        }
        s as f64 / w as f64
    }

    /// A flame texture is bright/wide at its base and dim/sparse at its tip, so a
    /// correctly-oriented fire card must sample brighter texture rows near the
    /// geometric BASE (min world-up Y) than near the TIP (max Y). This pins the
    /// (UV, embedded image) pair end-to-end, independent of which container era
    /// the archive uses: kaladimb is a DDS-repack archive (issue eqoxide#160 —
    /// the Mining Guild campfire rendered upside down), highpasshold is an
    /// original real-BMP archive.
    fn assert_flame_upright(archive: &str, object_base: &str) {
        let home = std::env::var("HOME").unwrap();
        let path = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/{archive}"));
        let models = load_object_models(&path, None).unwrap();
        let group = models.get(object_base)
            .unwrap_or_else(|| panic!("{archive} has object {object_base}"));
        let fire = group.iter()
            .find(|m| m.anim.is_some()
                && m.texture_name.as_deref().is_some_and(|t| t.to_lowercase().starts_with("fire")))
            .unwrap_or_else(|| panic!("{object_base} has an animated fire primitive"));

        // Geometry up-axis is Y (index 1) in libeq mesh space; grab V at the
        // flame card's base (min y) and tip (max y).
        let (mut ymin, mut ymax) = (f32::MAX, f32::MIN);
        let (mut v_base, mut v_tip) = (0.0f32, 0.0f32);
        for (p, uv) in fire.positions.iter().zip(fire.uvs.iter()) {
            if p[1] < ymin { ymin = p[1]; v_base = uv[1]; }
            if p[1] > ymax { ymax = p[1]; v_tip = uv[1]; }
        }
        assert!(ymax - ymin > 1.0, "fire card spans height: {ymin}..{ymax}");

        // Decode frame 0 exactly as the bake embeds it.
        let mut pfs = libeq_pfs::PfsReader::open(std::fs::File::open(&path).unwrap()).unwrap();
        let tex = fire.texture_name.clone().unwrap();
        let png = crate::convert::load_texture_from_archive(&mut pfs, &tex, fire.alpha_mode)
            .unwrap_or_else(|| panic!("decode {tex} from {archive}"));
        let img = image::load_from_memory(&png).unwrap().to_rgba8();

        // Compare texture bands sampled just inside each end of the card (avoid
        // the exact edge rows, which are often black borders).
        let band = |t0: f32, t1: f32| -> f64 {
            let mut s = 0.0; let mut n = 0;
            let mut t = t0;
            while t <= t1 + 1e-6 {
                s += row_luminance(&img, v_base + (v_tip - v_base) * t);
                n += 1;
                t += 0.05;
            }
            s / n as f64
        };
        let base_lum = band(0.05, 0.30);
        let tip_lum = band(0.70, 0.95);
        eprintln!("{archive} {object_base}: v_base={v_base:.3} v_tip={v_tip:.3} base_lum={base_lum:.1} tip_lum={tip_lum:.1}");
        assert!(base_lum > tip_lum,
            "{archive} {object_base}: flame is upside down — texture near geometric base \
             (lum {base_lum:.1}) must be brighter than near tip (lum {tip_lum:.1})");
    }

    /// Issue eqoxide#160: kaladimb (DDS-repack archive) Mining Guild campfire.
    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/kaladimb_obj.s3d"]
    fn kaladimb_campfire_flame_is_upright() {
        assert_flame_upright("kaladimb_obj.s3d", "CAMPFIRE");
    }

    /// Regression guard for the original-era real-BMP archives, which the old
    /// animated-only V flip happened to render correctly.
    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/highpasshold_obj.s3d"]
    fn highpass_campfire_flame_is_upright() {
        assert_flame_upright("highpasshold_obj.s3d", "CAMPFIRE2");
    }
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
            ..Default::default()
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

    /// A baked zone GLB must carry a dedicated `__collision__` mesh whose face count is at least
    /// the rendered terrain's, since collision keeps every SOLID face including invisible-but-
    /// solid ones that the render path never emits (no material). Validates the asset-server half
    /// of Component B end-to-end against a real zone with known zone boundaries (gfaydark).
    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/gfaydark.s3d"]
    fn baked_zone_has_collision_mesh_with_invisible_faces() {
        let home = std::env::var("HOME").unwrap();
        let main = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/gfaydark.s3d"));
        let obj = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/gfaydark_obj.s3d"));
        if !main.exists() { eprintln!("skip: gfaydark.s3d missing"); return; }
        let out = std::env::temp_dir().join("eqoxide_test_gfaydark.glb");
        bake_zone(&main, obj.exists().then_some(obj.as_path()), &out).unwrap();

        // Parse the GLB JSON chunk and find the __collision__ mesh.
        let bytes = std::fs::read(&out).unwrap();
        let json_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let json: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
        let meshes = json["meshes"].as_array().unwrap();
        let col = meshes.iter().find(|m| m["name"] == "__collision__")
            .expect("baked zone must contain a __collision__ mesh");
        let col_idx_acc = col["primitives"][0]["indices"].as_u64().unwrap() as usize;
        let col_faces = json["accessors"][col_idx_acc]["count"].as_u64().unwrap() / 3;

        let terrain = meshes.iter().find(|m| m["name"] == "terrain").unwrap();
        let render_faces: u64 = terrain["primitives"].as_array().unwrap().iter().map(|p| {
            let a = p["indices"].as_u64().unwrap() as usize;
            json["accessors"][a]["count"].as_u64().unwrap() / 3
        }).sum();

        assert!(col_faces > 0, "collision mesh has faces");
        // Collision (solid, incl. invisible) is generally >= rendered solid faces.
        assert!(col_faces >= render_faces / 2,
            "collision faces {col_faces} unexpectedly small vs render {render_faces}");
        eprintln!("gfaydark: render_faces={render_faces} collision_faces={col_faces}");
    }

    /// eqoxide#213: invisible boundary primitives (render method 0 / Boundary materials) must be
    /// EXCLUDED from render meshes while remaining available to collision. qeynos is the known
    /// worst case: its boundary shell (~809 prims spanning height −100..+100) rendered as an
    /// opaque black box enclosing the whole city — the zone looked like a void from inside.
    /// Asserts on the real archive: qeynos HAS invisible prims, and zone_meshes_from_mesh emits
    /// exactly the valid-minus-invisible primitive count (i.e. the skip removed them all).
    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/qeynos.s3d"]
    fn zone_meshes_skip_invisible_boundary_prims() {
        let home = std::env::var("HOME").unwrap();
        let main = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/qeynos.s3d"));
        if !main.exists() { eprintln!("skip: qeynos.s3d missing"); return; }
        let file = std::fs::File::open(&main).unwrap();
        let mut pfs = libeq_pfs::PfsReader::open(file).unwrap();
        let names: Vec<String> = pfs.filenames().unwrap();
        let (mut total_valid, mut invisible, mut emitted) = (0usize, 0usize, 0usize);
        for wn in names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
            let Ok(Some(bytes)) = pfs.get(wn) else { continue };
            let Ok(wld) = libeq_wld::load(&bytes) else { continue };
            for mesh in wld.meshes() {
                let n_pos = mesh.positions().len();
                for prim in mesh.primitives() {
                    let idx = prim.indices();
                    if idx.is_empty() || idx.iter().any(|&i| i as usize >= n_pos) { continue; }
                    total_valid += 1;
                    if crate::convert::is_invisible_render_method(prim.material().render_method()) {
                        invisible += 1;
                    }
                }
                emitted += zone_meshes_from_mesh(&mesh).len();
            }
        }
        eprintln!("qeynos: valid prims={total_valid} invisible={invisible} emitted={emitted}");
        assert!(invisible > 0, "qeynos must contain invisible boundary prims (had none?)");
        assert_eq!(emitted, total_valid - invisible,
            "render meshes must exclude exactly the invisible boundary prims");
    }

    #[test]
    fn place_instance_translates_and_rotates() {
        let model = ZoneMesh {
            positions: vec![[1.0, 0.0, 0.0]],
            normals: vec![[1.0, 0.0, 0.0]],
            uvs: vec![[0.0, 0.0]],
            indices: vec![0],
            texture_name: Some("wall.bmp".into()),
            ..Default::default()
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
    fn placement_matrix_matches_place_instance() {
        // For arbitrary model-local vertices, M*[v,1] must equal the world
        // position place_instance bakes into vertices.
        let model = ZoneMesh {
            positions: vec![[1.0, 2.0, 3.0], [-4.0, 0.5, 7.0], [0.0, 0.0, 0.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            uvs: vec![[0.0, 0.0]; 3],
            indices: vec![0, 1, 2],
            texture_name: None,
            ..Default::default()
        };
        let center = (10.0, 5.0, -20.0);
        let rot = 37.0;
        let scale = 1.5;
        let placed = place_instance(&model, center, rot, scale);
        let m = placement_matrix(center, rot, scale);
        for (v, expected) in model.positions.iter().zip(placed.positions.iter()) {
            // column-major: world = sum over j of col[j] * [v.x,v.y,v.z,1][j]
            let h = [v[0], v[1], v[2], 1.0];
            let mut w = [0.0f32; 3];
            for r in 0..3 {
                for j in 0..4 {
                    w[r] += m[j][r] * h[j];
                }
            }
            for k in 0..3 {
                assert!((w[k] - expected[k]).abs() < 1e-3, "axis {k}: {} vs {}", w[k], expected[k]);
            }
        }
    }

    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/qcat.s3d + qcat_obj.s3d"]
    fn placements_are_off_origin() {
        let home = std::env::var("HOME").unwrap();
        let main = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/qcat.s3d"));
        let obj = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/qcat_obj.s3d"));
        if !main.exists() || !obj.exists() { eprintln!("skip: archives missing"); return; }
        let models = load_object_models(&obj, None).unwrap();
        assert!(!models.is_empty(), "expected object models");
        let placements = read_placements(&main).unwrap();
        assert!(!placements.is_empty(), "expected placements");
        // Some placement translates off the origin (the "piled at 0,0,0" regression).
        let off_origin = placements.iter()
            .any(|(_, m)| m[3][0].abs() > 1.0 || m[3][2].abs() > 1.0);
        assert!(off_origin, "placements should not all be at the origin");
    }
}
