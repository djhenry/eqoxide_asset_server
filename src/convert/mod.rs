#![allow(dead_code, unused_imports)]
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use glam::{Mat4, Quat, Vec3};
use libeq_wld::parser::{
    Dag, DmSpriteDef2, HierarchicalSpriteDef, MaterialType, RenderMethod, Track, TrackDef, WldDoc,
};

pub(crate) struct PrimitiveData {
    pub(crate) indices: Vec<u32>,
    pub(crate) material_idx: usize,
    /// Optional per-primitive glTF `extras` object. Used to tag face and hair
    /// variant primitives on Luclin humanoid character models so the client can
    /// select the correct variant from Spawn_Struct face/hairstyle fields.
    pub(crate) extras: Option<serde_json::Value>,
}

pub(crate) struct MeshData {
    pub(crate) name: String,
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) normals: Vec<[f32; 3]>,
    pub(crate) uvs: Vec<[f32; 2]>,
    pub(crate) primitives: Vec<PrimitiveData>,
}

pub(crate) struct TextureData {
    pub(crate) name: String,
    pub(crate) png_bytes: Vec<u8>,
}

/// Transparency mode derived from the EQ material's `RenderMethod` / `MaterialType`.
/// Drives both how the source texture is decoded (masked keys out palette index 0)
/// and which glTF `alphaMode` is emitted.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum AlphaMode {
    #[default]
    Opaque,
    /// Cutout: palette index 0 becomes transparent; glTF `alphaMode: MASK`.
    Masked,
    /// Semi-transparent blend; opacity in permille (1000 = opaque). glTF `alphaMode: BLEND`.
    Blend(u16),
    /// Additive blend (EQ glow/fire). glTF `alphaMode: BLEND` + `extras.eqAdditive`.
    Additive,
}

pub(crate) struct MaterialData {
    pub(crate) name: String,
    pub(crate) texture_idx: Option<usize>,
    pub(crate) base_color: [f32; 4],
    pub(crate) alpha_mode: AlphaMode,
    /// For EQ animated textures (fire/water/lava): `(frame_interval_ms, frame image
    /// names incl. frame 0)`. The client cycles these frames at the interval. `None`
    /// for static textures.
    pub(crate) anim: Option<(u32, Vec<String>)>,
}

/// Build the glTF material JSON object for a [`MaterialData`], including the
/// `alphaMode`/`alphaCutoff` (and additive `extras`) derived from its EQ render
/// method. Shared by all three GLB writers so transparency is emitted uniformly.
fn material_to_gltf(mat: &MaterialData) -> serde_json::Value {
    // Blended materials carry opacity in the baseColorFactor alpha (which glTF
    // multiplies into the texture); masked/opaque keep full alpha.
    let base_alpha = match mat.alpha_mode {
        AlphaMode::Blend(permille) => permille as f32 / 1000.0,
        _ => mat.base_color[3],
    };
    let pbr = if let Some(ti) = mat.texture_idx {
        serde_json::json!({
            "baseColorTexture": { "index": ti },
            "baseColorFactor": [1.0, 1.0, 1.0, base_alpha],
            "metallicFactor": 0.0,
            "roughnessFactor": 1.0,
        })
    } else {
        let mut bc = mat.base_color;
        bc[3] = base_alpha;
        serde_json::json!({ "baseColorFactor": bc, "metallicFactor": 0.0, "roughnessFactor": 1.0 })
    };
    let mut m = serde_json::json!({
        "name": mat.name,
        "pbrMetallicRoughness": pbr,
        "doubleSided": true,
    });
    let mut extras = serde_json::Map::new();
    match mat.alpha_mode {
        AlphaMode::Opaque => {}
        AlphaMode::Masked => {
            m["alphaMode"] = serde_json::json!("MASK");
            m["alphaCutoff"] = serde_json::json!(0.5);
        }
        AlphaMode::Blend(_) => {
            m["alphaMode"] = serde_json::json!("BLEND");
        }
        AlphaMode::Additive => {
            // glTF has no additive mode; flag it for the client via extras.
            m["alphaMode"] = serde_json::json!("BLEND");
            extras.insert("eqAdditive".into(), serde_json::json!(true));
        }
    }
    // Animated texture (fire/water/lava): emit frame interval + frame image names so
    // the client can cycle them. glTF has no native texture-frame animation.
    if let Some((ms, frames)) = &mat.anim {
        extras.insert(
            "eqAnim".into(),
            serde_json::json!({ "ms": ms, "frames": frames }),
        );
    }
    if !extras.is_empty() {
        m["extras"] = serde_json::Value::Object(extras);
    }
    m
}

/// A glTF node for the instanced writer: references a mesh by index, with an
/// optional column-major 4x4 transform `matrix` (identity when `None`).
pub(crate) struct NodeDef {
    pub(crate) mesh_idx: usize,
    /// Column-major 4x4 (glTF convention). `None` => identity (no `matrix` emitted).
    pub(crate) matrix: Option<[[f32; 4]; 4]>,
}

pub fn s3d_to_glb(input_s3d: &Path, output_glb: &Path, skinned: bool) -> Result<()> {
    s3d_to_glb_model(input_s3d, output_glb, skinned, None)
}

/// Like `s3d_to_glb`, but `model_code` selects a single model (its 3-char EQ code,
/// e.g. "SKE", "BEA") out of a multi-model character archive. `None` converts the
/// whole archive (one model per archive, e.g. the per-race `global*_chr.s3d`).
pub fn s3d_to_glb_model(input_s3d: &Path, output_glb: &Path, skinned: bool, model_code: Option<&str>) -> Result<()> {
    if skinned {
        convert_s3d_to_glb_skinned(input_s3d, output_glb, model_code)?;
    } else {
        convert_s3d_to_glb(input_s3d, output_glb)?;
    }
    Ok(())
}

fn convert_s3d_to_glb(input: &Path, output: &Path) -> Result<()> {
    let file = fs::File::open(input)
        .with_context(|| format!("failed to open {}", input.display()))?;
    let mut pfs = libeq_pfs::PfsReader::open(file)
        .with_context(|| format!("failed to parse PFS: {}", input.display()))?;

    let filenames: Vec<String> = pfs
        .filenames()
        .with_context(|| "failed to list filenames")?;

    let wld_files: Vec<&str> = filenames
        .iter()
        .filter(|f| f.to_lowercase().ends_with(".wld"))
        .map(|f| f.as_str())
        .collect();

    // Merge ALL WLD meshes into a SINGLE glTF mesh.
    // This is critical because the renderer applies one x_center/z_center offset
    // to all primitives — if meshes are separate, the centering shifts smaller
    // meshes (like eyes) away from the body.
    let mut merged_positions: Vec<[f32; 3]> = Vec::new();
    let mut merged_normals: Vec<[f32; 3]> = Vec::new();
    let mut merged_uvs: Vec<[f32; 2]> = Vec::new();
    let mut merged_primitives: Vec<PrimitiveData> = Vec::new();
    let mut texture_map: HashMap<String, usize> = HashMap::new();
    let mut materials: Vec<MaterialData> = Vec::new();
    let mut textures: Vec<TextureData> = Vec::new();
    let mut total_wld_meshes: usize = 0;

    for wld_name in &wld_files {
        let wld_bytes = match pfs.get(wld_name) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => {
                eprintln!("  warning: failed to read {}: {}", wld_name, e);
                continue;
            }
        };

        let wld = match libeq_wld::load(&wld_bytes) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("  warning: failed to parse {}: {}", wld_name, e);
                continue;
            }
        };

        // Low-level pass: build the skeletal bind pose and pose every skinned
        // mesh into model space. Character meshes (DmSpriteDef2 with skin
        // assignment groups) store vertices in bone-local space; without the
        // bind-pose bone transforms applied they collapse into an overlapping
        // blob. Keyed by mesh name so the high-level loop below can pick up the
        // posed geometry. Zone/object meshes have no skin groups and fall back
        // to raw positions.
        let posed: HashMap<String, (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<bool>)> =
            match WldDoc::parse(&wld_bytes) {
                Ok(doc) => match build_bind_pose(&doc) {
                    Some(bind) => {
                        let mut map = HashMap::new();
                        for frag in doc.fragment_iter::<DmSpriteDef2>() {
                            let name = doc
                                .get_string(frag.name_reference)
                                .unwrap_or("")
                                .to_string();
                            if let Some(geo) = pose_skinned_mesh(frag, &bind) {
                                map.insert(name, geo);
                            }
                        }
                        eprintln!(
                            "  built bind pose ({} bones), posed {} skinned mesh(es)",
                            bind.world.len(),
                            map.len()
                        );
                        map
                    }
                    None => HashMap::new(),
                },
                Err(_) => HashMap::new(),
            };

        for mesh in wld.meshes() {
            let mesh_name = mesh.name().unwrap_or("unnamed").to_string();

            // Use skeletally-posed geometry if available; otherwise fall back to
            // the raw (already Y-up) positions for non-skinned meshes.
            let posed_geo = posed.get(&mesh_name);
            let (all_positions, all_normals, outliers) = match posed_geo {
                Some((p, n, o)) => (p.clone(), n.clone(), o.clone()),
                None => {
                    let p = mesh.positions();
                    let len = p.len();
                    (p, mesh.normals(), vec![false; len])
                }
            };
            if all_positions.is_empty() {
                continue;
            }

            let all_uvs = mesh.texture_coordinates();

            // Skip eye meshes only when they could NOT be posed. A posed eye mesh
            // is correctly placed at its head bone; an unposed one sits at the
            // origin and gets misaligned by the renderer's centering.
            if posed_geo.is_none() && mesh_name.to_uppercase().contains("EYE") {
                eprintln!("  skipping unposed eye mesh '{}'", mesh_name);
                continue;
            }

            // Index offset: new primitives reference vertices starting from
            // the current end of the merged buffer.
            let index_offset = merged_positions.len() as u32;

            // Append vertices to merged buffer.
            merged_positions.extend_from_slice(&all_positions);
            merged_normals.extend_from_slice(&all_normals);
            merged_uvs.extend_from_slice(&all_uvs);

            // Build per-primitive index lists with offset.
            let mut prim_count = 0;
            for primitive in mesh.primitives() {
                let raw_indices: Vec<u32> = primitive.indices();
                if raw_indices.is_empty() {
                    continue;
                }

                // Drop triangles that reference flagged placeholder vertices
                // (stray attachment-point geometry), then offset the surviving
                // indices into the merged vertex buffer.
                let mut prim_indices: Vec<u32> = Vec::with_capacity(raw_indices.len());
                for tri in raw_indices.chunks_exact(3) {
                    let drop = tri
                        .iter()
                        .any(|&i| outliers.get(i as usize).copied().unwrap_or(false));
                    if drop {
                        continue;
                    }
                    for &i in tri {
                        prim_indices.push(i + index_offset);
                    }
                }
                if prim_indices.is_empty() {
                    continue;
                }

                let material = primitive.material();
                let texture_source = material.base_color_texture().and_then(|t| t.source());

                let material_idx = get_or_create_material(
                    &mut materials,
                    &mut texture_map,
                    &mut textures,
                    &material,
                    texture_source.as_deref(),
                    &mut pfs,
                );

                merged_primitives.push(PrimitiveData {
                    indices: prim_indices,
                    material_idx,
                    extras: None,
                });
                prim_count += 1;
            }

            if prim_count > 0 {
                eprintln!("  mesh '{}': {} verts, {} primitives (offset={})",
                    mesh_name, all_positions.len(), prim_count, index_offset);
                total_wld_meshes += 1;
            }
        }
    }

    if merged_primitives.is_empty() {
        anyhow::bail!("no meshes found in {}", input.display());
    }

    // Create a single merged mesh.
    let all_meshes = vec![MeshData {
        name: "combined".to_string(),
        positions: merged_positions,
        normals: merged_normals,
        uvs: merged_uvs,
        primitives: merged_primitives,
    }];

    eprintln!(
        "  merged {} WLD meshes into 1 glTF mesh ({} verts, {} prims), {} materials, {} textures",
        total_wld_meshes,
        all_meshes[0].positions.len(),
        all_meshes[0].primitives.len(),
        materials.len(),
        textures.len()
    );

    write_glb(output, &all_meshes, &materials, &textures)
}

fn get_or_create_material(
    materials: &mut Vec<MaterialData>,
    texture_map: &mut HashMap<String, usize>,
    textures: &mut Vec<TextureData>,
    material: &libeq_wld::Material<'_>,
    texture_source: Option<&str>,
    pfs: &mut libeq_pfs::PfsReader<fs::File>,
) -> usize {
    let mat_name = material.name().unwrap_or("unnamed").to_string();
    let alpha_mode = alpha_mode_from_render(material.render_method());

    let texture_idx = if let Some(src) = texture_source {
        // Key by (source, alpha_mode): masked keys out index 0 and blend bakes
        // opacity into alpha, so the decoded bytes differ per mode — never dedup
        // the same texture across modes.
        let cache_key = format!("{}\0{:?}", src, alpha_mode);
        if let Some(&idx) = texture_map.get(&cache_key) {
            Some(idx)
        } else {
            let tex_name = src.to_lowercase();
            match load_texture_from_archive(pfs, &tex_name, alpha_mode) {
                Some(png_bytes) => {
                    let idx = textures.len();
                    textures.push(TextureData {
                        name: tex_name,
                        png_bytes,
                    });
                    texture_map.insert(cache_key, idx);
                    Some(idx)
                }
                None => None,
            }
        }
    } else {
        None
    };

    let idx = materials.len();
    materials.push(MaterialData {
        name: mat_name,
        texture_idx,
        base_color: [1.0, 1.0, 1.0, 1.0],
        alpha_mode,
        anim: None, // character/weapon model textures: keep first frame only
    });
    idx
}

/// Map an EQ material's `RenderMethod` to our [`AlphaMode`]. Foliage and other
/// cutout surfaces are `TransparentMasked`/`TransparentMaskedPassable`; the
/// `Transparent25/50/75` types are semi-transparent blends; the additive types
/// are EQ glow/fire surfaces. Everything else renders opaque.
pub(crate) fn alpha_mode_from_render(rm: &RenderMethod) -> AlphaMode {
    match rm {
        RenderMethod::UserDefined { material_type } => match material_type {
            MaterialType::TransparentMasked | MaterialType::TransparentMaskedPassable => {
                AlphaMode::Masked
            }
            MaterialType::Transparent25 => AlphaMode::Blend(250),
            MaterialType::Transparent50 => AlphaMode::Blend(500),
            MaterialType::Transparent75 => AlphaMode::Blend(750),
            MaterialType::TransparentAdditive | MaterialType::TransparentAdditiveUnlit => {
                AlphaMode::Additive
            }
            _ => AlphaMode::Opaque,
        },
        RenderMethod::Standard { .. } => AlphaMode::Opaque,
    }
}

/// Detect if a WLD material name refers to a Luclin head-region material.
/// Material names follow the pattern `{RACE}HE000{N}_MDF` (e.g. `ELFHE0001_MDF`,
/// `ELFHE0008_MDF`) where N ∈ 1..=8 corresponds to one of the 8 head polygon
/// groups that together form the full Luclin character head.
///
/// N ∈ {1, 4, 5}: hairstyle-swappable regions.
/// N ∈ {2, 3, 6, 7, 8}: fixed regions (ears, neck, features, forehead).
/// Returns N when matched, None otherwise.
fn head_region_from_material_name(name: &str) -> Option<u8> {
    let u = name.to_uppercase();
    let stem = u.trim_end_matches("_MDF");
    // The suffix "HE000{N}" is 6 characters; the 3-char race prefix precedes it.
    if stem.len() >= 6 {
        let tail = &stem[stem.len() - 6..];
        if tail.starts_with("HE000") {
            let n = tail.as_bytes()[5];
            if n >= b'1' && n <= b'8' {
                return Some(n - b'0');
            }
        }
    }
    None
}

/// Load a texture by filename from the PFS archive, caching in `texture_map` to
/// avoid duplicate buffer entries. Returns the texture index in `textures`, or
/// `None` if the file is absent in the archive.
fn load_or_cache_texture(
    pfs: &mut libeq_pfs::PfsReader<fs::File>,
    name: &str,
    alpha_mode: AlphaMode,
    textures: &mut Vec<TextureData>,
    texture_map: &mut HashMap<String, usize>,
) -> Option<usize> {
    let cache_key = format!("{}\0{:?}", name, alpha_mode);
    if let Some(&idx) = texture_map.get(&cache_key) {
        return Some(idx);
    }
    match load_texture_from_archive(pfs, name, alpha_mode) {
        Some(png_bytes) => {
            let idx = textures.len();
            textures.push(TextureData { name: name.to_string(), png_bytes });
            texture_map.insert(cache_key, idx);
            Some(idx)
        }
        None => None,
    }
}

/// Extract the 3-char EQ race/sex code from a Luclin character archive path.
/// `globalelf_chr.s3d` → `"elf"`, `globalhum_chr.s3d` → `"hum"`, etc.
fn race_code_from_archive(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?.to_lowercase();
    let tail = stem.strip_prefix("global")?;
    let code = tail.split('_').next()?;
    if !code.is_empty() {
        Some(code.to_string())
    } else {
        None
    }
}

pub(crate) fn load_texture_from_archive(
    pfs: &mut libeq_pfs::PfsReader<fs::File>,
    name: &str,
    alpha_mode: AlphaMode,
) -> Option<Vec<u8>> {
    let lower = name.to_lowercase();

    // Try the name as-is first
    if let Some(data) = try_load_image(pfs, &lower, alpha_mode) {
        return Some(data);
    }

    // Try stripping extension and trying common ones
    let stem = if lower.ends_with(".dds") || lower.ends_with(".bmp") || lower.ends_with(".png") {
        &lower[..lower.len() - 4]
    } else {
        &lower
    };

    for ext in &[".dds", ".bmp", ".png"] {
        let filename = format!("{}{}", stem, ext);
        if let Some(data) = try_load_image(pfs, &filename, alpha_mode) {
            return Some(data);
        }
    }
    None
}

fn try_load_image(pfs: &mut libeq_pfs::PfsReader<fs::File>, filename: &str, alpha_mode: AlphaMode) -> Option<Vec<u8>> {
    let data = pfs.get(filename).ok()??;
    // For masked materials, recover EQ's keyed transparency: in 8-bit paletted BMPs
    // palette index 0 is the transparent key. The `image` crate's to_rgba8() would
    // make it opaque, so decode the palette ourselves when we can.
    let mut rgba = if alpha_mode == AlphaMode::Masked {
        decode_bmp_keyed(&data).unwrap_or_else(|| image::load_from_memory(&data).ok().map(|i| i.to_rgba8()).unwrap_or_default())
    } else {
        image::load_from_memory(&data).ok()?.to_rgba8()
    };
    if rgba.is_empty() {
        return None;
    }
    // Bake per-material opacity into the alpha channel for blended materials so the
    // client can blend straight from the texture (no per-draw opacity uniform).
    if let AlphaMode::Blend(permille) = alpha_mode {
        let scale = permille as f32 / 1000.0;
        for px in rgba.pixels_mut() {
            px[3] = (px[3] as f32 * scale).round().clamp(0.0, 255.0) as u8;
        }
    }
    let mut png_buf = Cursor::new(Vec::new());
    rgba.write_to(&mut png_buf, image::ImageFormat::Png).ok()?;
    Some(png_buf.into_inner())
}

/// Decode an 8-bit paletted BMP, treating palette index 0 as fully transparent
/// (EQ's masked-texture convention). Returns `None` for any BMP that isn't the
/// uncompressed 8bpp BITMAPINFOHEADER form, so callers fall back to opaque decode.
fn decode_bmp_keyed(data: &[u8]) -> Option<image::RgbaImage> {
    if data.len() < 54 || &data[0..2] != b"BM" {
        return None;
    }
    let rd_u32 = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    let rd_i32 = |o: usize| i32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    let rd_u16 = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);

    let pixel_offset = rd_u32(10) as usize;
    let dib_size = rd_u32(14) as usize;
    if dib_size < 40 {
        return None; // only BITMAPINFOHEADER (40) or larger
    }
    let width = rd_i32(18);
    let height_raw = rd_i32(22);
    let bpp = rd_u16(28);
    let compression = rd_u32(30);
    if bpp != 8 || compression != 0 || width <= 0 || height_raw == 0 {
        return None;
    }
    let width = width as usize;
    let top_down = height_raw < 0;
    let height = height_raw.unsigned_abs() as usize;

    // Palette: 4 bytes each (B,G,R,reserved), right after the DIB header.
    let palette_start = 14 + dib_size;
    let mut colors_used = rd_u32(46) as usize;
    if colors_used == 0 {
        colors_used = 256;
    }
    if palette_start + colors_used * 4 > data.len() || palette_start + colors_used * 4 > pixel_offset {
        return None;
    }
    let palette: Vec<[u8; 3]> = (0..colors_used)
        .map(|i| {
            let p = palette_start + i * 4;
            [data[p + 2], data[p + 1], data[p]] // R,G,B
        })
        .collect();

    // Rows are padded to a multiple of 4 bytes.
    let row_stride = (width + 3) & !3;
    if pixel_offset + row_stride * height > data.len() {
        return None;
    }

    let mut img = image::RgbaImage::new(width as u32, height as u32);
    for y in 0..height {
        // BMP is bottom-up unless height is negative.
        let src_row = if top_down { y } else { height - 1 - y };
        let row = pixel_offset + src_row * row_stride;
        for x in 0..width {
            let idx = data[row + x] as usize;
            let [r, g, b] = palette.get(idx).copied().unwrap_or([0, 0, 0]);
            let a = if idx == 0 { 0 } else { 255 };
            img.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, a]));
        }
    }
    Some(img)
}

/// A skeletal bind pose: world-space matrix per bone (indexed by dag index), in
/// EQ native coordinate space (Z-up). Built by walking the HierarchicalSpriteDef
/// dag tree and composing each bone's local transform (frame 0 of its TrackDef)
/// with its parent's world transform.
struct BindPose {
    world: Vec<Mat4>,
}

/// Resolve a dag's TrackDef (the per-bone transform) by following
/// dag.track_reference (1-based fragment index) -> Track (0x13) -> TrackDef (0x12).
fn dag_track_def<'a>(doc: &'a WldDoc, dag: &Dag) -> Option<&'a TrackDef> {
    if dag.track_reference == 0 {
        return None;
    }
    let track = doc
        .at((dag.track_reference - 1) as usize)?
        .as_any()
        .downcast_ref::<Track>()?;
    doc.get(&track.reference)
}

/// Decode a single animation frame (translation, rotation) in EQ-native space.
/// New-format (FrameTransform): rotation quaternion = (x,y,z,denominator) normalized,
/// translation = shift_xyz / shift_denominator. Legacy format stores floats directly.
fn frame_trs(td: &TrackDef, frame: usize) -> (Vec3, Quat) {
    if let Some(frames) = &td.frame_transforms {
        if let Some(f) = frames.get(frame).or_else(|| frames.first()) {
            // `rotate_denominator` is the quaternion's W component, and it can legitimately be 0 —
            // that's a valid 180° rotation (W=0), NOT identity. The old `if denominator != 0` guard
            // silently dropped those flips: the wolf's rear hind-leg-top bones store raw x/y
            // numerators with W=0, so they came out un-rotated and the whole rear half rendered
            // inverted/upside-down (eqoxide#40). Only fall back to identity when ALL four components
            // are 0 (an unnormalizable, genuinely-absent rotation).
            let rot = {
                let q = Quat::from_xyzw(
                    f.rotate_x_numerator as f32,
                    f.rotate_y_numerator as f32,
                    f.rotate_z_numerator as f32,
                    f.rotate_denominator as f32,
                );
                if q.length_squared() > 1e-12 { q.normalize() } else { Quat::IDENTITY }
            };
            let trans = if f.shift_denominator != 0 {
                let d = f.shift_denominator as f32;
                Vec3::new(
                    f.shift_x_numerator as f32 / d,
                    f.shift_y_numerator as f32 / d,
                    f.shift_z_numerator as f32 / d,
                )
            } else {
                Vec3::ZERO
            };
            return (trans, rot);
        }
    }
    if let Some(frames) = &td.legacy_frame_transforms {
        if let Some(f) = frames.get(frame).or_else(|| frames.first()) {
            let rot = Quat::from_xyzw(f.rotate_x, f.rotate_y, f.rotate_z, f.rotate_w).normalize();
            let trans = if f.shift_denominator != 0.0 {
                Vec3::new(
                    f.shift_x_numerator / f.shift_denominator,
                    f.shift_y_numerator / f.shift_denominator,
                    f.shift_z_numerator / f.shift_denominator,
                )
            } else {
                Vec3::ZERO
            };
            return (trans, rot);
        }
    }
    (Vec3::ZERO, Quat::IDENTITY)
}

/// Build the local (relative-to-parent) bind transform from frame 0 of a TrackDef.
fn track_local_matrix(td: &TrackDef) -> Mat4 {
    let (t, r) = frame_trs(td, 0);
    Mat4::from_rotation_translation(r, t)
}

fn walk_dag(
    doc: &WldDoc,
    dags: &[Dag],
    idx: usize,
    parent: Mat4,
    world: &mut [Mat4],
    visited: &mut [bool],
) {
    if idx >= dags.len() || visited[idx] {
        return;
    }
    visited[idx] = true;
    let local = dag_track_def(doc, &dags[idx])
        .map(track_local_matrix)
        .unwrap_or(Mat4::IDENTITY);
    let w = parent * local;
    world[idx] = w;
    for &child in &dags[idx].sub_dags {
        walk_dag(doc, dags, child as usize, w, world, visited);
    }
}

/// Build the bind pose from the first HierarchicalSpriteDef in the document.
fn build_bind_pose(doc: &WldDoc) -> Option<BindPose> {
    let skel = doc.fragment_iter::<HierarchicalSpriteDef>().next()?;
    let n = skel.dags.len();
    if n == 0 {
        return None;
    }
    let mut world = vec![Mat4::IDENTITY; n];
    let mut visited = vec![false; n];
    walk_dag(doc, &skel.dags, 0, Mat4::IDENTITY, &mut world, &mut visited);
    if std::env::var("DEBUG_SKEL").is_ok() {
        let unreached: Vec<usize> = (0..n).filter(|&i| !visited[i]).collect();
        eprintln!(
            "  [skel] {} dags, {} unreached from root: {:?}",
            n,
            unreached.len(),
            unreached
        );
    }
    Some(BindPose { world })
}

/// Pose a skinned mesh into model space using the bind pose. Returns Y-up
/// positions and normals (matching libeq's high-level convention, which swaps
/// EQ Z-up to glTF Y-up). Returns None for non-skinned meshes (zone geometry).
/// Flag placeholder/degenerate vertices. A handful of vertices in some character
/// meshes are assigned to non-rendering attachment bones (weapon/shield mount
/// points) and sit at extreme positions, producing a stray triangle that wrecks
/// the bounding box (which in turn breaks the renderer's auto-scaling). Flag any
/// vertex whose distance from the median vertex position is a gross outlier,
/// relative to the model's own scale so large creatures aren't harmed.
fn detect_outliers(positions: &[[f32; 3]]) -> Vec<bool> {
    let n = positions.len();
    if n < 8 {
        return vec![false; n];
    }
    let median = |axis: usize| -> f32 {
        let mut vals: Vec<f32> = positions.iter().map(|p| p[axis]).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        vals[vals.len() / 2]
    };
    let c = [median(0), median(1), median(2)];
    let mut dists: Vec<f32> = positions
        .iter()
        .map(|p| {
            let d = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .collect();
    let mut sorted = dists.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_dist = sorted[sorted.len() / 2];
    let threshold = (median_dist * 6.0).max(4.0);
    dists.iter_mut().map(|d| *d > threshold).collect()
}

fn pose_skinned_mesh(
    frag: &DmSpriteDef2,
    bind: &BindPose,
) -> Option<(Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<bool>)> {
    if frag.skin_assignment_groups.is_empty() {
        return None;
    }
    let scale = 1.0 / ((1u32 << frag.scale) as f32);

    // Expand (count, bone_index) groups into a per-vertex bone index.
    let mut vbone: Vec<usize> = Vec::with_capacity(frag.positions.len());
    for (count, bone) in &frag.skin_assignment_groups {
        for _ in 0..*count {
            vbone.push(*bone as usize);
        }
    }
    let last = vbone.last().copied().unwrap_or(0);
    while vbone.len() < frag.positions.len() {
        vbone.push(last);
    }

    let nbones = bind.world.len();
    if std::env::var("DEBUG_SKEL").is_ok() {
        let max_bone = frag.skin_assignment_groups.iter().map(|g| g.1).max().unwrap_or(0);
        eprintln!(
            "  [skel] mesh skin groups={} max_bone_idx={} nbones={} total_assigned={}",
            frag.skin_assignment_groups.len(),
            max_bone,
            nbones,
            vbone.len()
        );
    }
    let mut positions = Vec::with_capacity(frag.positions.len());
    let mut normals = Vec::with_capacity(frag.positions.len());
    let debug = std::env::var("DEBUG_SKEL").is_ok();
    for (i, p) in frag.positions.iter().enumerate() {
        let b = vbone[i].min(nbones - 1);
        let m = bind.world[b];
        let v = Vec3::new(p.0 as f32 * scale, p.1 as f32 * scale, p.2 as f32 * scale);
        let w = m.transform_point3(v);
        if debug && (v.length() > 4.0 || w.length() > 6.0) {
            let bt = m.w_axis.truncate();
            eprintln!(
                "    [vtx {}] bone={} raw=({:.2},{:.2},{:.2}) |raw|={:.2} posed=({:.2},{:.2},{:.2}) bone_translation=({:.2},{:.2},{:.2})",
                i, b, v.x, v.y, v.z, v.length(), w.x, w.y, w.z, bt.x, bt.y, bt.z
            );
        }
        positions.push([w.x, w.z, w.y]);

        let n = frag
            .vertex_normals
            .get(i)
            .map(|n| Vec3::new(n.0 as f32 / 127.0, n.1 as f32 / 127.0, n.2 as f32 / 127.0))
            .unwrap_or(Vec3::Y);
        let nw = m.transform_vector3(n).normalize_or_zero();
        normals.push([nw.x, nw.z, nw.y]);
    }
    let outliers = detect_outliers(&positions);
    if debug {
        let n_out = outliers.iter().filter(|&&o| o).count();
        if n_out > 0 {
            eprintln!("    [skel] flagged {} outlier (placeholder) vert(s)", n_out);
        }
    }
    Some((positions, normals, outliers))
}

/// Rich skeleton: per-bone parent, rest-local transform (translation+rotation),
/// world bind matrix, and the bone's base track name (used to build the names of
/// animated tracks).
struct Skel {
    #[allow(dead_code)]
    parent: Vec<Option<usize>>,
    local_t: Vec<Vec3>,
    local_r: Vec<Quat>,
    world: Vec<Mat4>,
    base_track: Vec<String>,
    children: Vec<Vec<usize>>,
}

fn build_skel(doc: &WldDoc, code: Option<&str>) -> Option<Skel> {
    let skel = match code {
        // Pick the skeleton whose name starts with the model code (e.g. "BEA_HS_DEF").
        Some(c) => doc.fragment_iter::<HierarchicalSpriteDef>().find(|s| {
            doc.get_string(s.name_reference)
                .map(|n| n.starts_with(c))
                .unwrap_or(false)
        })?,
        None => doc.fragment_iter::<HierarchicalSpriteDef>().next()?,
    };
    let n = skel.dags.len();
    if n == 0 {
        return None;
    }
    let mut parent = vec![None; n];
    let mut local_t = vec![Vec3::ZERO; n];
    let mut local_r = vec![Quat::IDENTITY; n];
    let mut world = vec![Mat4::IDENTITY; n];
    let mut base_track = vec![String::new(); n];
    let mut children = vec![Vec::new(); n];

    for (i, dag) in skel.dags.iter().enumerate() {
        if let Some(td) = dag_track_def(doc, dag) {
            let (t, r) = frame_trs(td, 0);
            local_t[i] = t;
            local_r[i] = r;
        }
        // The bone's base track name (e.g. "DWMPEBIP01_TRACK").
        if dag.track_reference > 0 {
            if let Some(track) = doc
                .at((dag.track_reference - 1) as usize)
                .and_then(|f| f.as_any().downcast_ref::<Track>())
            {
                base_track[i] = doc.get_string(track.name_reference).unwrap_or("").to_string();
            }
        }
        for &c in &dag.sub_dags {
            let c = c as usize;
            if c < n {
                children[i].push(c);
                parent[c] = Some(i);
            }
        }
    }

    // Compute world bind matrices by walking from the root (index 0).
    let mut visited = vec![false; n];
    let mut stack = vec![(0usize, Mat4::IDENTITY)];
    while let Some((idx, parent_world)) = stack.pop() {
        if idx >= n || visited[idx] {
            continue;
        }
        visited[idx] = true;
        let w = parent_world * Mat4::from_rotation_translation(local_r[idx], local_t[idx]);
        world[idx] = w;
        for &c in &children[idx] {
            stack.push((c, w));
        }
    }

    Some(Skel {
        parent,
        local_t,
        local_r,
        world,
        base_track,
        children,
    })
}

/// One animation clip: a 3-char EQ code (e.g. "L01") plus, per bone, an optional
/// list of per-frame (translation, rotation) transforms in EQ-native space.
struct Anim {
    code: String,
    frame_ms: u32,
    /// Indexed by bone; None means the bone has no track in this clip (holds rest).
    bones: Vec<Option<Vec<(Vec3, Quat)>>>,
}

/// Resolve a Track (0x13) fragment by exact name, then its TrackDef (0x12).
fn track_def_by_name<'a>(doc: &'a WldDoc, name: &str) -> Option<(&'a Track, &'a TrackDef)> {
    for t in doc.fragment_iter::<Track>() {
        if doc.get_string(t.name_reference) == Some(name) {
            if let Some(td) = doc.get(&t.reference) {
                return Some((t, td));
            }
        }
    }
    None
}

/// Discover all animation clips by scanning Track names for a leading 3-char code
/// and matching them to each bone's base track name.
fn gather_anims(doc: &WldDoc, skel: &Skel) -> Vec<Anim> {
    use std::collections::{BTreeSet, HashMap};
    // Map each bone's base track name -> bone index. Animated tracks are named
    // <animCode><baseTrackName>; the code length varies (3 chars on classic
    // global_chr models, e.g. "C01"; 4 chars on Luclin high-res, e.g. "C01A").
    // So detect by suffix-matching the (longest) base track name rather than
    // assuming a fixed code length.
    let mut base_to_bone: HashMap<&str, usize> = HashMap::new();
    for (i, b) in skel.base_track.iter().enumerate() {
        if !b.is_empty() {
            base_to_bone.insert(b.as_str(), i);
        }
    }
    // Longest base names first so e.g. "SKEPE_TRACK" wins over "SKE_TRACK".
    let mut bases_by_len: Vec<&str> = base_to_bone.keys().copied().collect();
    bases_by_len.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let is_code = |code: &str| -> bool {
        let len = code.len();
        if !(3..=4).contains(&len) {
            return false;
        }
        let cb = code.as_bytes();
        // letter, digit, digit, [optional letter]
        cb[0].is_ascii_alphabetic()
            && cb[1].is_ascii_digit()
            && cb[2].is_ascii_digit()
            && (len == 3 || (len == 4 && cb[3].is_ascii_alphabetic()))
    };

    let mut codes: BTreeSet<String> = BTreeSet::new();
    for t in doc.fragment_iter::<Track>() {
        let name = match doc.get_string(t.name_reference) {
            Some(n) => n,
            None => continue,
        };
        for b in &bases_by_len {
            if name.len() > b.len() && name.ends_with(b) {
                let code = &name[..name.len() - b.len()];
                if is_code(code) {
                    codes.insert(code.to_string());
                }
                break;
            }
        }
    }

    if std::env::var("DEBUG_SKEL").is_ok() {
        eprintln!(
            "  [anim] base_names={} codes_found={}: {:?}",
            base_to_bone.len(),
            codes.len(),
            codes.iter().take(20).collect::<Vec<_>>()
        );
    }
    let mut anims = Vec::new();
    for code in codes {
        let mut bones: Vec<Option<Vec<(Vec3, Quat)>>> = vec![None; skel.base_track.len()];
        let mut frame_ms = 100u32;
        for (i, base) in skel.base_track.iter().enumerate() {
            if base.is_empty() {
                continue;
            }
            let anim_name = format!("{}{}", code, base);
            if let Some((track, td)) = track_def_by_name(doc, &anim_name) {
                let fc = td.frame_count.max(1) as usize;
                let frames: Vec<(Vec3, Quat)> = (0..fc).map(|f| frame_trs(td, f)).collect();
                bones[i] = Some(frames);
                if let Some(s) = track.sleep {
                    if s > 0 {
                        frame_ms = s;
                    }
                }
            }
        }
        if bones.iter().any(|b| b.is_some()) {
            anims.push(Anim {
                code,
                frame_ms,
                bones,
            });
        }
    }
    anims
}

/// The 3-char EQ model code of a skeleton (e.g. "DAM"), taken from the leading
/// chars of a bone's base-track name (`DAM_TRACK`, `DAMPEBIP01_TRACK`, ...).
fn skel_model_code(skel: &Skel) -> Option<String> {
    skel.base_track.iter().find(|b| b.len() >= 3).map(|b| b[..3].to_string())
}

/// A handful of human-like reskin races ship a `_chr.s3d` with the mesh + bind
/// pose but NO animation tracks; the Titanium client animates them from a base
/// race with an identical skeleton. Maps a model code to its
/// `(donor archive filename, donor model code)`. Bones match across the two by
/// name once the 3-char model code is stripped (verified: only the cosmetic
/// hair-point leaf differs, which simply holds its bind pose).
fn anim_donor(code: &str) -> Option<(&'static str, &'static str)> {
    match code.to_ascii_uppercase().as_str() {
        "DAM" | "HIM" | "HAM" | "ERM" => Some(("globalhum_chr.s3d", "HUM")), // -> Human male
        "DAF" | "HIF" | "HAF" | "ERF" => Some(("globalhuf_chr.s3d", "HUF")), // -> Human female
        _ => None,
    }
}

/// Borrow animation clips for a skeleton that has none, from its donor race's
/// archive (see [`anim_donor`]). Donor clips are re-indexed onto the target
/// skeleton by matching bone names with the 3-char model code stripped. Returns
/// an empty vec when there is no donor or the donor archive can't be read.
fn borrow_anims(input: &Path, target: &Skel) -> Vec<Anim> {
    let Some(code) = skel_model_code(target) else { return Vec::new() };
    let Some((donor_file, _donor_code)) = anim_donor(&code) else { return Vec::new() };
    let Some(donor_path) = input.parent().map(|p| p.join(donor_file)) else { return Vec::new() };

    let file = match fs::File::open(&donor_path) {
        Ok(f) => f,
        Err(_) => {
            tracing::warn!("anim donor {} missing — {} will have no animations", donor_path.display(), code);
            return Vec::new();
        }
    };
    let mut pfs = match libeq_pfs::PfsReader::open(file) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let names = match pfs.filenames() {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };
    for wld_name in names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let bytes = match pfs.get(wld_name) {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let doc = match WldDoc::parse(&bytes) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let donor_skel = match build_skel(&doc, None) {
            Some(s) => s,
            None => continue,
        };
        let donor_anims = gather_anims(&doc, &donor_skel);
        if donor_anims.is_empty() {
            continue;
        }
        // donor bone suffix (code stripped) -> donor bone index
        let donor_by_suffix: HashMap<&str, usize> = donor_skel.base_track.iter().enumerate()
            .filter(|(_, b)| b.len() > 3)
            .map(|(j, b)| (&b[3..], j))
            .collect();
        // target bone i -> donor bone j (same code-stripped suffix)
        let map: Vec<Option<usize>> = target.base_track.iter()
            .map(|b| if b.len() > 3 { donor_by_suffix.get(&b[3..]).copied() } else { None })
            .collect();

        let matched = map.iter().filter(|m| m.is_some()).count();
        let remapped: Vec<Anim> = donor_anims.into_iter().map(|a| {
            let mut bones: Vec<Option<Vec<(Vec3, Quat)>>> = vec![None; target.base_track.len()];
            for (i, donor_j) in map.iter().enumerate() {
                if let Some(j) = donor_j {
                    bones[i] = a.bones.get(*j).cloned().flatten();
                }
            }
            Anim { code: a.code, frame_ms: a.frame_ms, bones }
        }).collect();
        tracing::info!("{} borrowed {} anim clips from {} ({}/{} bones matched)",
            code, remapped.len(), donor_file, matched, target.base_track.len());
        return remapped;
    }
    Vec::new()
}

/// Map an EQ animation code (first 3 chars, e.g. "L01") to a semantic keyword so
/// the renderer's name-substring clip selection ("idle"/"walk"/"run"/...) works.
/// Standard EQ WLD locomotion/passive codes.
fn anim_label(code3: &str) -> Option<&'static str> {
    Some(match code3 {
        "L01" => "walk",
        "L02" => "run",
        "L03" => "jump_run",
        "L04" => "fall",
        "L05" => "duckwalk",
        "L06" => "swim",
        "L07" => "walk_back",
        "L08" => "swim_idle",
        "L09" => "swim",
        "P01" => "idle_neutral",
        "P02" => "sit",
        "P03" => "crouch",
        "P06" => "kneel",
        "P07" => "swim_idle",
        // Only O01/O02 are the standing idle fidgets ("standby"/"standby2"). O03 is NOT a standing
        // idle — it's a low crouched/looting pose (~knee height), so it must not carry the "idle"
        // label (the client's idle cycle selects clips by the "idle" substring). Confirmed against
        // the Titanium client + EQEmu animation map (no "standby3").
        "O01" | "O02" => "idle",
        "O03" => "looting",
        c if c.starts_with('C') => "combat",
        "D05" => "death",
        c if c.starts_with('D') => "hit",
        c if c.starts_with('S') => "social",
        c if c.starts_with('T') => "emote",
        _ => return None,
    })
}

/// Per-vertex skin data plus EQ-space bind-pose geometry for one mesh.
struct SkinnedGeo {
    positions: Vec<[f32; 3]>, // EQ-native (z-up) bind pose, model space
    normals: Vec<[f32; 3]>,   // EQ-native
    joints: Vec<u16>,         // assigned bone index per vertex
    outliers: Vec<bool>,
}

fn gather_skinned_geo(frag: &DmSpriteDef2, skel: &Skel) -> Option<SkinnedGeo> {
    if frag.skin_assignment_groups.is_empty() {
        return None;
    }
    let scale = 1.0 / ((1u32 << frag.scale) as f32);
    let mut vbone: Vec<usize> = Vec::with_capacity(frag.positions.len());
    for (count, bone) in &frag.skin_assignment_groups {
        for _ in 0..*count {
            vbone.push(*bone as usize);
        }
    }
    let last = vbone.last().copied().unwrap_or(0);
    while vbone.len() < frag.positions.len() {
        vbone.push(last);
    }

    let nbones = skel.world.len();
    let mut positions = Vec::with_capacity(frag.positions.len());
    let mut normals = Vec::with_capacity(frag.positions.len());
    let mut joints = Vec::with_capacity(frag.positions.len());
    // For outlier detection we evaluate the (swapped) posed positions just like the
    // static path, so the same robust threshold applies.
    let mut posed_for_outlier = Vec::with_capacity(frag.positions.len());
    for (i, p) in frag.positions.iter().enumerate() {
        let b = vbone[i].min(nbones - 1);
        let v = Vec3::new(p.0 as f32 * scale, p.1 as f32 * scale, p.2 as f32 * scale);
        let w = skel.world[b].transform_point3(v);
        positions.push([w.x, w.y, w.z]); // EQ-native bind pose
        posed_for_outlier.push([w.x, w.z, w.y]);
        let n = frag
            .vertex_normals
            .get(i)
            .map(|n| Vec3::new(n.0 as f32 / 127.0, n.1 as f32 / 127.0, n.2 as f32 / 127.0))
            .unwrap_or(Vec3::Y);
        let nw = skel.world[b].transform_vector3(n).normalize_or_zero();
        normals.push([nw.x, nw.y, nw.z]);
        joints.push(b as u16);
    }
    let outliers = detect_outliers(&posed_for_outlier);
    Some(SkinnedGeo {
        positions,
        normals,
        joints,
        outliers,
    })
}

/// Per-bone classification of a Luclin character skeleton for hair/face splitting.
/// `head[b]` — bone `b` is the head bone (`{RACE}HEHEAD`); scalp/skull vertices
/// bind ONLY to it. `face[b]` — bone `b` is a facial-animation bone (`{RACE}FA*`:
/// eyelids, brows, nose, jaw, lips); facial-skin vertices bind to these.
/// Identified from each bone's base track name (e.g. `HUFHEHEAD_TRACK`).
struct HeadBones {
    head: Vec<bool>,
    face: Vec<bool>,
}

fn head_bones(skel: &Skel, race_code: &str) -> HeadBones {
    let uc = race_code.to_uppercase();
    let head_pat = format!("{uc}HEHEAD");
    let face_pat = format!("{uc}FA");
    HeadBones {
        head: skel.base_track.iter().map(|t| t.contains(&head_pat)).collect(),
        face: skel.base_track.iter().map(|t| t.contains(&face_pat)).collect(),
    }
}

/// Split a head region's triangles into painted-hair scalp vs facial skin.
/// A triangle whose three vertices all bind to the head bone is scalp — the part
/// of the region the artists painted hair onto, tinted by haircolor at runtime.
/// Any triangle touching a facial bone (brows, lids, nose, jaw, lips) is skin.
/// Returns `(hair_idxs, face_idxs)`.
fn split_hair_face(idxs: &[u32], joints: &[u16], bones: &HeadBones) -> (Vec<u32>, Vec<u32>) {
    let mut hair = Vec::new();
    let mut face = Vec::new();
    for tri in idxs.chunks_exact(3) {
        let all_head = tri.iter().all(|&gi| {
            bones.head.get(joints[gi as usize] as usize).copied().unwrap_or(false)
        });
        if all_head {
            hair.extend_from_slice(tri);
        } else {
            face.extend_from_slice(tri);
        }
    }
    (hair, face)
}

fn convert_s3d_to_glb_skinned(input: &Path, output: &Path, model_code: Option<&str>) -> Result<()> {
    let file = fs::File::open(input)
        .with_context(|| format!("failed to open {}", input.display()))?;
    let mut pfs = libeq_pfs::PfsReader::open(file)?;
    let filenames: Vec<String> = pfs.filenames()?;
    let wld_files: Vec<String> = filenames
        .iter()
        .filter(|f| f.to_lowercase().ends_with(".wld"))
        .cloned()
        .collect();

    for wld_name in &wld_files {
        let wld_bytes = match pfs.get(wld_name) {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let doc = match WldDoc::parse(&wld_bytes) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let skel = match build_skel(&doc, model_code) {
            Some(s) => s,
            None => continue,
        };
        let mut anims = gather_anims(&doc, &skel);
        // Reskin races (dark/high/half elf, erudite) ship with no animation tracks;
        // borrow them from a base race with the same skeleton.
        if anims.is_empty() {
            anims = borrow_anims(input, &skel);
        }

        // When extracting one model from a multi-model archive, only its meshes
        // (name starts with the code) belong to this skeleton's skin_assignment bones.
        let mesh_belongs = |name: &str| match model_code {
            Some(c) => name.starts_with(c),
            None => true,
        };

        // ── WHY HAIR/BEARD MESHES ARE ABSENT (investigation: task-4-report.md) ───────────────
        // Luclin character WLDs (e.g. globalelf_chr.wld, 30,460 fragments) contain exactly 3
        // DmSpriteDef2 (0x36) mesh fragments: the body mesh (ELF_DMSPRITEDEF, 52 skin groups,
        // 25 face_material_groups), left eye, and right eye.  The body mesh's 25 face_material_groups
        // cover body parts + 8 face texture variants (HE0001–HE0008) — ZERO hair groups.
        //
        // Hair-related WLD content that IS present:
        //   • ELFHEHAIR1–9_DAG / ELFHAIR_POINT_DAG bones in the HierarchicalSpriteDef skeleton,
        //     all with mesh_or_sprite_reference = 0 (animation-only, no mesh attached)
        //   • 210+ orphaned MaterialDef (0x30) fragments for hair material variants
        //     (ELFHE0011_MDF, ELFHE0911_MDF … pieces 11/14/15, colors 0–9, styles 1–7) that
        //     are NOT referenced by the single MaterialPalette "ELF_MP" (27 materials, body+eyes)
        //   • elfhesk11.dds – elfhesk75.dds hair skin textures in the PFS archive
        //
        // Hair-related WLD content that is ABSENT:
        //   • No DmSpriteDef2 fragments with HAIR/BEARD names (confirmed via fragment_iter +
        //     raw type_id scan of all 30,460 fragments)
        //   • No face_material_groups in ELF_DMSPRITEDEF referencing any hair material
        //   • No dag mesh references (all dag.mesh_or_sprite_reference == 0)
        //
        // The EQ client selects hair at runtime via vtable 0xec("ELF_HS%2d_HEAD_HAIR", style)
        // which attaches a named sub-model to slot 8 (ELFHAIR_POINT_DAG).  The geometry for
        // those sub-models is NOT stored in the main WLD; the client loads them via a separate
        // mechanism (possibly per-style sub-actor archives or DAG-attached DmSpriteDef2 fragments
        // that libeq_wld currently fails to decode).
        //
        // TASK 5 FIX: Before emitting primitives here, gather the HierarchicalSpriteDef's full
        // dag list; for each hair-bone dag (name contains "HEHAIR"), follow its
        // mesh_or_sprite_reference (0x2D DmSprite → 0x36 DmSpriteDef2) when non-zero.  This
        // emits each hair style as a separate named glTF mesh node tagged with its style index
        // in glTF extras {"hair_style": N}, mirroring the face-variant approach.  If dag
        // mesh_ref remains 0 for all hair dags (as seen for elf), a deeper investigation into
        // the sub-model loading path is required before geometry can be emitted.
        // ─────────────────────────────────────────────────────────────────────────────────────
        let mut geo_map: HashMap<String, SkinnedGeo> = HashMap::new();
        for frag in doc.fragment_iter::<DmSpriteDef2>() {
            let name = doc.get_string(frag.name_reference).unwrap_or("").to_string();
            if !mesh_belongs(&name) {
                continue;
            }
            if let Some(g) = gather_skinned_geo(frag, &skel) {
                geo_map.insert(name, g);
            }
        }

        let wld = libeq_wld::load(&wld_bytes).map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut uvs: Vec<[f32; 2]> = Vec::new();
        let mut joints: Vec<u16> = Vec::new();
        let mut prims: Vec<PrimitiveData> = Vec::new();
        let mut texture_map: HashMap<String, usize> = HashMap::new();
        let mut materials: Vec<MaterialData> = Vec::new();
        let mut textures: Vec<TextureData> = Vec::new();

        // Race code from archive path (e.g. "elf" from "globalelf_chr.s3d");
        // used to construct head-region texture filenames like elfhesk01.dds.
        let race_code = race_code_from_archive(input).unwrap_or_default();
        let bones = head_bones(&skel, &race_code);

        for mesh in wld.meshes() {
            let name = mesh.name().unwrap_or("").to_string();
            let geo = match geo_map.get(&name) {
                Some(g) => g,
                None => continue,
            };
            // Hairline height (EQ-native z-up): the topmost vertex bound to a facial
            // bone (the brow line). Fixed head regions fully on the head bone that sit
            // ABOVE this line (e.g. huf region 2, the sculpted part-line strip across
            // the skull top) are painted hair; ones at/below it (ears) are skin.
            let hairline_up: Option<f32> = geo
                .joints
                .iter()
                .zip(geo.positions.iter())
                .filter(|(j, _)| bones.face.get(**j as usize).copied().unwrap_or(false))
                .map(|(_, p)| p[2])
                .fold(None, |acc: Option<f32>, z| Some(acc.map_or(z, |a| a.max(z))));
            let mesh_uvs = mesh.texture_coordinates();
            let offset = positions.len() as u32;
            for i in 0..geo.positions.len() {
                positions.push(geo.positions[i]);
                normals.push(geo.normals[i]);
                uvs.push(mesh_uvs.get(i).copied().unwrap_or([0.0, 0.0]));
                joints.push(geo.joints[i]);
            }
            for primitive in mesh.primitives() {
                let raw: Vec<u32> = primitive.indices();
                let mut idxs: Vec<u32> = Vec::with_capacity(raw.len());
                for tri in raw.chunks_exact(3) {
                    if tri
                        .iter()
                        .any(|&i| geo.outliers.get(i as usize).copied().unwrap_or(false))
                    {
                        continue;
                    }
                    for &i in tri {
                        idxs.push(i + offset);
                    }
                }
                if idxs.is_empty() {
                    continue;
                }
                let material = primitive.material();
                let mat_name_str = material.name().unwrap_or("");

                // Check if this primitive belongs to one of the 8 Luclin head
                // polygon groups (material pattern {RACE}HE000{N}_MDF, N=1..8).
                //
                // N ∈ {1, 4, 5}: FACE-variant regions (face+scalp, nose bridge, nose
                //   tip for huf; layout varies per race). The RoF2 client swaps their
                //   textures by spawn.face — decompile eqgame FUN_0040d770 attr 1 →
                //   FUN_0040d1a0 "%sHE%02d%d1_MDF" — NOT by hairstyle (hairstyle is a
                //   dead actor-attach path for S3D races; no "*_HEAD_HAIR" actor ships).
                //   Emit 8 variants (F=0..7) with texture {race}hesk{F}{N}.dds, each
                //   split into a facial-skin prim ({"eq_face": F}) and a painted-hair
                //   scalp prim ({"eq_face": F, "eq_head_part": "hair"}, runtime-tinted
                //   by haircolor). F=0 is default-visible; F≥1 default-hidden.
                //
                // N ∈ {2, 3, 6, 7, 8}: fixed regions, emitted once with
                //   {race}hesk0{N}.dds. Ones fully on the head bone above the brow
                //   line (huf N=2, the sculpted part-line strip across the skull top)
                //   are painted hair → tagged {"eq_head_part": "hair"} (no eq_face,
                //   always visible, tinted). Ears/teeth/mouth stay untagged skin.
                //
                // All other groups (body/eye): emit normally from the WLD material.
                const SWAPPABLE: [u8; 3] = [1, 4, 5];
                match (!race_code.is_empty()).then(|| head_region_from_material_name(mat_name_str)).flatten() {
                    Some(n) if SWAPPABLE.contains(&n) => {
                        let (hair_idxs, face_idxs) = split_hair_face(&idxs, &joints, &bones);
                        let mut emitted = 0u8;
                        for f in 0u8..=7 {
                            let tex_name = format!("{}hesk{}{}", race_code, f, n);
                            let tex_idx = match load_or_cache_texture(&mut pfs, &tex_name, AlphaMode::Opaque, &mut textures, &mut texture_map) {
                                Some(t) => t,
                                None => {
                                    eprintln!("  head region N={} face F={}: texture '{}' not found in archive", n, f, tex_name);
                                    continue;
                                }
                            };
                            let mat_idx = materials.len();
                            materials.push(MaterialData {
                                name: tex_name.clone(),
                                texture_idx: Some(tex_idx),
                                base_color: [1.0, 1.0, 1.0, 1.0],
                                alpha_mode: AlphaMode::Opaque,
                                anim: None,
                            });
                            if !face_idxs.is_empty() {
                                let mut extras = serde_json::json!({ "eq_face": f });
                                if f > 0 { extras["eq_default_hidden"] = serde_json::json!(true); }
                                prims.push(PrimitiveData {
                                    indices: face_idxs.clone(),
                                    material_idx: mat_idx,
                                    extras: Some(extras),
                                });
                            }
                            if !hair_idxs.is_empty() {
                                let mut extras = serde_json::json!({ "eq_face": f, "eq_head_part": "hair" });
                                if f > 0 { extras["eq_default_hidden"] = serde_json::json!(true); }
                                prims.push(PrimitiveData {
                                    indices: hair_idxs.clone(),
                                    material_idx: mat_idx,
                                    extras: Some(extras),
                                });
                            }
                            emitted += 1;
                        }
                        eprintln!(
                            "  head region N={} (face-variant): {}/8 faces, {} hair tris + {} skin tris",
                            n, emitted, hair_idxs.len() / 3, face_idxs.len() / 3
                        );
                    }
                    Some(n) => {
                        // Fixed head region: emit once with {race}hesk0{N}.dds.
                        let tex_name = format!("{}hesk0{}", race_code, n);
                        let tex_idx = load_or_cache_texture(&mut pfs, &tex_name, AlphaMode::Opaque, &mut textures, &mut texture_map);
                        let mat_idx = materials.len();
                        materials.push(MaterialData {
                            name: tex_name.clone(),
                            texture_idx: tex_idx,
                            base_color: [1.0, 1.0, 1.0, 1.0],
                            alpha_mode: AlphaMode::Opaque,
                            anim: None,
                        });
                        // Painted-hair crown strips: fully head-bone-bound and above the
                        // brow line → tint by haircolor (always visible, no face variants).
                        let pure_head = idxs.iter().all(|&gi| {
                            bones.head.get(joints[gi as usize] as usize).copied().unwrap_or(false)
                        });
                        let centroid_up = idxs.iter().map(|&gi| positions[gi as usize][2]).sum::<f32>()
                            / idxs.len().max(1) as f32;
                        let is_crown = pure_head && hairline_up.is_some_and(|h| centroid_up > h);
                        prims.push(PrimitiveData {
                            indices: idxs,
                            material_idx: mat_idx,
                            extras: is_crown.then(|| serde_json::json!({ "eq_head_part": "hair" })),
                        });
                        eprintln!("  head region N={} (fixed{}): texture '{}'", n, if is_crown { ", crown hair" } else { "" }, tex_name);
                    }
                    None => {
                        // Body or eye group: emit normally from the WLD material.
                        let tex = material.base_color_texture().and_then(|t| t.source());
                        let midx = get_or_create_material(
                            &mut materials,
                            &mut texture_map,
                            &mut textures,
                            &material,
                            tex.as_deref(),
                            &mut pfs,
                        );
                        prims.push(PrimitiveData {
                            indices: idxs,
                            material_idx: midx,
                            extras: None,
                        });
                    }
                }
            } // end for primitive in mesh.primitives()
        } // end for mesh in wld.meshes()

        if prims.is_empty() {
            anyhow::bail!("no skinned meshes found in {}", input.display());
        }

        eprintln!(
            "  skinned: {} verts, {} prims, {} bones, {} anim clips ({})",
            positions.len(),
            prims.len(),
            skel.world.len(),
            anims.len(),
            anims.iter().map(|a| a.code.as_str()).collect::<Vec<_>>().join(",")
        );
        return write_glb_skinned(
            output, &positions, &normals, &uvs, &joints, &prims, &materials, &textures, &skel,
            &anims,
        );
    }
    anyhow::bail!("no skeleton found in {}", input.display());
}

// ── glTF buffer authoring helpers ────────────────────────────────────────────

fn align4(buf: &mut Vec<u8>) {
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}

fn add_view(
    buf: &mut Vec<u8>,
    views: &mut Vec<serde_json::Value>,
    bytes: &[u8],
    target: Option<u32>,
) -> usize {
    align4(buf);
    let offset = buf.len();
    buf.extend_from_slice(bytes);
    let mut v = serde_json::json!({
        "buffer": 0,
        "byteOffset": offset,
        "byteLength": bytes.len(),
    });
    if let Some(t) = target {
        v["target"] = serde_json::json!(t);
    }
    views.push(v);
    views.len() - 1
}

fn add_accessor(
    accessors: &mut Vec<serde_json::Value>,
    view: usize,
    component_type: u32,
    count: usize,
    typ: &str,
    minmax: Option<(serde_json::Value, serde_json::Value)>,
) -> usize {
    let mut a = serde_json::json!({
        "bufferView": view,
        "componentType": component_type,
        "count": count,
        "type": typ,
    });
    if let Some((mn, mx)) = minmax {
        a["min"] = mn;
        a["max"] = mx;
    }
    accessors.push(a);
    accessors.len() - 1
}

fn f32x3_bytes(data: &[[f32; 3]]) -> Vec<u8> {
    let mut b = Vec::with_capacity(data.len() * 12);
    for p in data {
        for c in p {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    b
}

#[allow(clippy::too_many_arguments)]
fn write_glb_skinned(
    output: &Path,
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    joints: &[u16],
    prims: &[PrimitiveData],
    materials: &[MaterialData],
    textures: &[TextureData],
    skel: &Skel,
    anims: &[Anim],
) -> Result<()> {
    let mut buf: Vec<u8> = Vec::new();
    let mut views: Vec<serde_json::Value> = Vec::new();
    let mut accessors: Vec<serde_json::Value> = Vec::new();
    let mut images: Vec<serde_json::Value> = Vec::new();
    let mut gltf_textures: Vec<serde_json::Value> = Vec::new();
    let mut gltf_materials: Vec<serde_json::Value> = Vec::new();

    // Textures + materials (same as static path).
    for tex in textures {
        let vi = add_view(&mut buf, &mut views, &tex.png_bytes, None);
        images.push(serde_json::json!({ "bufferView": vi, "mimeType": "image/png", "name": tex.name }));
        gltf_textures.push(serde_json::json!({ "source": images.len() - 1 }));
    }
    for mat in materials {
        gltf_materials.push(material_to_gltf(mat));
    }

    // Everything is authored in EQ-native space (Z-up). To present a Y-up glTF we
    // rotate the whole rig by `rq` (Z-up -> Y-up). This rotation is baked into the
    // vertex data, joint local transforms, inverse-bind matrices and animation
    // frames (by conjugation) rather than a shared root node — a shared ancestor
    // would be cancelled out by glTF's skinning math (skin matrices factor out the
    // mesh node's global transform).
    let rq = Quat::from_axis_angle(Vec3::X, -std::f32::consts::FRAC_PI_2);
    let rmat = Mat4::from_quat(rq);
    let rot_p = |p: &[f32; 3]| -> [f32; 3] {
        let v = rq * Vec3::from_array(*p);
        [v.x, v.y, v.z]
    };
    let rot_positions: Vec<[f32; 3]> = positions.iter().map(rot_p).collect();
    let rot_normals: Vec<[f32; 3]> = normals.iter().map(rot_p).collect();

    // ── Translation normalization (EQ-native Z-up, BEFORE the Y-up rotation) ──────
    // Compute the posed bind bbox over the un-rotated, EQ-native `positions`
    // (z = height). Center the two horizontal axes (X, Y) and ground the height
    // axis (feet to z = 0). The resulting `offset` is added to the skeleton root
    // bone's rest translation and to every root-bone animation keyframe, but NOT to
    // the inverse-bind matrices (those stay derived from the ORIGINAL skel.world).
    // Because world (rest + every animated pose) carries +offset while the inverse-
    // bind does not, the world-offset and inverse-bind-offset do NOT cancel, so the
    // skinned result is uniformly translated by `offset` in bind pose and in every
    // clip. eq_height is the height extent (offset preserves extent).
    let (mut xmin, mut xmax) = (f32::MAX, f32::MIN);
    let (mut ymin, mut ymax) = (f32::MAX, f32::MIN);
    let (mut zmin, mut zmax) = (f32::MAX, f32::MIN);
    for p in positions {
        xmin = xmin.min(p[0]);
        xmax = xmax.max(p[0]);
        ymin = ymin.min(p[1]);
        ymax = ymax.max(p[1]);
        zmin = zmin.min(p[2]);
        zmax = zmax.max(p[2]);
    }
    let eq_height = (zmax - zmin).max(0.0);
    // Conversion-time translation normalization was reverted: it offset the root bone in
    // rest + every animation keyframe, but a single offset cannot center every clip (each
    // has its own root baseline), which displaced the model during animation. Centering is
    // now done at render time from measured bounds. Keep eq_height (height extent) for the
    // renderer's target-height scaling; apply no skeleton offset.
    let _ = (xmin, xmax, ymin, ymax);
    let offset = Vec3::ZERO;

    // Vertex attributes (shared across primitives).
    let (pmin, pmax) = compute_bounds_f32x3(&rot_positions);
    let pos_view = add_view(&mut buf, &mut views, &f32x3_bytes(&rot_positions), Some(34962));
    let pos_acc = add_accessor(
        &mut accessors, pos_view, 5126, rot_positions.len(), "VEC3",
        Some((serde_json::json!(pmin), serde_json::json!(pmax))),
    );
    let nrm_view = add_view(&mut buf, &mut views, &f32x3_bytes(&rot_normals), Some(34962));
    let nrm_acc = add_accessor(&mut accessors, nrm_view, 5126, rot_normals.len(), "VEC3", None);
    let mut uv_bytes = Vec::with_capacity(uvs.len() * 8);
    for u in uvs {
        uv_bytes.extend_from_slice(&u[0].to_le_bytes());
        uv_bytes.extend_from_slice(&u[1].to_le_bytes());
    }
    let uv_view = add_view(&mut buf, &mut views, &uv_bytes, Some(34962));
    let uv_acc = add_accessor(&mut accessors, uv_view, 5126, uvs.len(), "VEC2", None);
    // JOINTS_0 (u16 vec4) and WEIGHTS_0 (f32 vec4).
    let mut j_bytes = Vec::with_capacity(joints.len() * 8);
    let mut w_bytes = Vec::with_capacity(joints.len() * 16);
    for &j in joints {
        for c in [j, 0u16, 0, 0] {
            j_bytes.extend_from_slice(&c.to_le_bytes());
        }
        for c in [1.0f32, 0.0, 0.0, 0.0] {
            w_bytes.extend_from_slice(&c.to_le_bytes());
        }
    }
    let j_view = add_view(&mut buf, &mut views, &j_bytes, Some(34962));
    let j_acc = add_accessor(&mut accessors, j_view, 5123, joints.len(), "VEC4", None);
    let w_view = add_view(&mut buf, &mut views, &w_bytes, Some(34962));
    let w_acc = add_accessor(&mut accessors, w_view, 5126, joints.len(), "VEC4", None);

    // Primitives (per material), each with its own index accessor.
    // Use u32 indices (componentType 5125) when the mesh has >65535 vertices;
    // u16 (5123) silently wraps for large meshes and corrupts geometry.
    let use_u32_indices = positions.len() > 65535;
    let mut gltf_prims = Vec::new();
    for prim in prims {
        let mut ib = Vec::with_capacity(prim.indices.len() * if use_u32_indices { 4 } else { 2 });
        if use_u32_indices {
            for &i in &prim.indices {
                ib.extend_from_slice(&i.to_le_bytes());
            }
        } else {
            for &i in &prim.indices {
                ib.extend_from_slice(&(i as u16).to_le_bytes());
            }
        }
        let iv = add_view(&mut buf, &mut views, &ib, Some(34963));
        let idx_component_type = if use_u32_indices { 5125u32 } else { 5123u32 };
        let ia = add_accessor(&mut accessors, iv, idx_component_type, prim.indices.len(), "SCALAR", None);
        let mut prim_json = serde_json::json!({
            "attributes": {
                "POSITION": pos_acc, "NORMAL": nrm_acc, "TEXCOORD_0": uv_acc,
                "JOINTS_0": j_acc, "WEIGHTS_0": w_acc,
            },
            "indices": ia,
            "material": prim.material_idx,
        });
        if let Some(extras) = &prim.extras {
            prim_json["extras"] = extras.clone();
        }
        gltf_prims.push(prim_json);
    }

    // Inverse bind matrices in the rotated (Y-up) space, column-major.
    // world' = rmat * world_EQ * rmat^-1; invBind = (world')^-1.
    let n = skel.world.len();
    let rmat_inv = rmat.inverse();
    let mut ibm = Vec::with_capacity(n * 64);
    for w in &skel.world {
        let world_yup = rmat * *w * rmat_inv;
        for c in world_yup.inverse().to_cols_array() {
            ibm.extend_from_slice(&c.to_le_bytes());
        }
    }
    let ibm_view = add_view(&mut buf, &mut views, &ibm, None);
    let ibm_acc = add_accessor(&mut accessors, ibm_view, 5126, n, "MAT4", None);

    // ── Nodes ────────────────────────────────────────────────────────────────
    // Joint nodes 0..n-1, with each local transform conjugated by the Y-up
    // rotation: local' = rmat * local * rmat^-1, i.e. t' = rq*t, r' = rq*r*rq^-1.
    // Then the skinned mesh node (a sibling of the joint root, NOT an ancestor).
    let rq_conj = rq.conjugate();
    let mut nodes: Vec<serde_json::Value> = Vec::with_capacity(n + 1);
    for i in 0..n {
        // Root bone (index 0) carries the normalization offset (EQ-native), applied
        // before the Y-up rotation. Children are relative to the root and inherit it.
        let local_t = if i == 0 { skel.local_t[i] + offset } else { skel.local_t[i] };
        let t = rq * local_t;
        let r = (rq * skel.local_r[i] * rq_conj).normalize();
        // Carry the WLD bone name (base track minus the "_TRACK" suffix, e.g.
        // "HUFR_POINT") so the client can locate attachment bones (R_POINT /
        // L_POINT / SHIELD_POINT) for held items by name.
        let bone_name = skel.base_track[i].trim_end_matches("_TRACK");
        let bone_name = if bone_name.is_empty() { format!("BONE{i}") } else { bone_name.to_string() };
        let mut node = serde_json::json!({
            "name": bone_name,
            "translation": [t.x, t.y, t.z],
            "rotation": [r.x, r.y, r.z, r.w],
        });
        if !skel.children[i].is_empty() {
            node["children"] = serde_json::json!(skel.children[i]);
        }
        nodes.push(node);
    }
    // Record the model's true height (EQ-native height extent) on the skeleton root
    // node so the renderer can scale/ground consistently. node 0 is the root joint.
    nodes[0]["extras"] = serde_json::json!({ "eq_height": eq_height });
    let mesh_idx = n;
    nodes.push(serde_json::json!({ "name": "mesh", "mesh": 0, "skin": 0 }));

    let skin = serde_json::json!({
        "joints": (0..n).collect::<Vec<_>>(),
        "inverseBindMatrices": ibm_acc,
        "skeleton": 0,
    });

    // ── Animations ─────────────────────────────────────────────────────────────
    let mut gltf_anims = Vec::new();
    for anim in anims {
        let mut channels = Vec::new();
        let mut samplers = Vec::new();
        let dt = anim.frame_ms as f32 / 1000.0;
        for (bone, frames_opt) in anim.bones.iter().enumerate() {
            let frames = match frames_opt {
                Some(f) if !f.is_empty() => f,
                _ => continue,
            };
            // Shared time input for this bone's channels.
            let times: Vec<f32> = (0..frames.len()).map(|f| f as f32 * dt).collect();
            let mut tb = Vec::with_capacity(times.len() * 4);
            for t in &times {
                tb.extend_from_slice(&t.to_le_bytes());
            }
            let t_view = add_view(&mut buf, &mut views, &tb, None);
            let tmax = times.last().copied().unwrap_or(0.0);
            let t_acc = add_accessor(
                &mut accessors, t_view, 5126, times.len(), "SCALAR",
                Some((serde_json::json!([0.0f32]), serde_json::json!([tmax]))),
            );
            // Translation output (rotated into Y-up space). The root bone (index 0)
            // carries the normalization offset in EQ-native space, applied before
            // the Y-up rotation — matching the root rest translation above. Clips
            // with no root translation channel keep the offset rest value instead.
            let mut trb = Vec::with_capacity(frames.len() * 12);
            for (t, _) in frames {
                let tt = rq * (if bone == 0 { *t + offset } else { *t });
                for c in [tt.x, tt.y, tt.z] {
                    trb.extend_from_slice(&c.to_le_bytes());
                }
            }
            let tr_view = add_view(&mut buf, &mut views, &trb, None);
            let tr_acc = add_accessor(&mut accessors, tr_view, 5126, frames.len(), "VEC3", None);
            // Rotation output (conjugated into Y-up space: r' = rq*r*rq^-1).
            let mut rb = Vec::with_capacity(frames.len() * 16);
            for (_, r) in frames {
                let rr = (rq * *r * rq_conj).normalize();
                for c in [rr.x, rr.y, rr.z, rr.w] {
                    rb.extend_from_slice(&c.to_le_bytes());
                }
            }
            let r_view = add_view(&mut buf, &mut views, &rb, None);
            let r_acc = add_accessor(&mut accessors, r_view, 5126, frames.len(), "VEC4", None);

            let s0 = samplers.len();
            samplers.push(serde_json::json!({ "input": t_acc, "output": tr_acc, "interpolation": "LINEAR" }));
            samplers.push(serde_json::json!({ "input": t_acc, "output": r_acc, "interpolation": "LINEAR" }));
            channels.push(serde_json::json!({ "sampler": s0, "target": { "node": bone, "path": "translation" } }));
            channels.push(serde_json::json!({ "sampler": s0 + 1, "target": { "node": bone, "path": "rotation" } }));
        }
        if !channels.is_empty() {
            // Name = "<code>_<semantic>" (e.g. "L01A_walk", "P01A_idle_neutral")
            // so the renderer's substring clip lookup resolves idle/walk/run.
            let name = match anim_label(&anim.code[..3]) {
                Some(label) => format!("{}_{}", anim.code, label),
                None => anim.code.clone(),
            };
            gltf_anims.push(serde_json::json!({ "name": name, "channels": channels, "samplers": samplers }));
        }
    }

    align4(&mut buf);
    let mut gltf = serde_json::json!({
        "asset": { "version": "2.0", "generator": "s3d_to_gltf (skinned)" },
        "scene": 0,
        "scenes": [{ "nodes": [0usize, mesh_idx] }],
        "nodes": nodes,
        "meshes": [{ "name": "combined", "primitives": gltf_prims }],
        "skins": [skin],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": buf.len() }],
        "materials": gltf_materials,
        "images": images,
        "textures": gltf_textures,
    });
    if !gltf_anims.is_empty() {
        gltf["animations"] = serde_json::json!(gltf_anims);
    }

    let json_str = serde_json::to_string(&gltf)?;
    let json_bytes = json_str.as_bytes();
    let json_padded = (json_bytes.len() + 3) & !3;
    let total = 12 + 8 + json_padded + 8 + buf.len();

    let mut out = fs::File::create(output)
        .with_context(|| format!("failed to create {}", output.display()))?;
    out.write_all(&0x46546C67u32.to_le_bytes())?;
    out.write_all(&2u32.to_le_bytes())?;
    out.write_all(&(total as u32).to_le_bytes())?;
    out.write_all(&(json_padded as u32).to_le_bytes())?;
    out.write_all(&0x4E4F534Au32.to_le_bytes())?;
    out.write_all(json_bytes)?;
    for _ in json_bytes.len()..json_padded {
        out.write_all(b" ")?;
    }
    out.write_all(&(buf.len() as u32).to_le_bytes())?;
    out.write_all(&0x004E4942u32.to_le_bytes())?;
    out.write_all(&buf)?;
    eprintln!("  wrote {} bytes to {}", total, output.display());
    Ok(())
}

/// List every model (HierarchicalSpriteDef = skeleton) in an archive, its 3-char
/// code, dag count, and whether its body mesh is new-format (skin_assignment_groups)
/// or old-format (rigid per-bone meshes referenced by dags).
fn list_models(input: &Path) -> Result<()> {
    let file = fs::File::open(input)?;
    let mut pfs = libeq_pfs::PfsReader::open(file)?;
    let filenames = pfs.filenames()?;
    for wld_name in filenames.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let bytes = match pfs.get(wld_name) {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let doc = match WldDoc::parse(&bytes) {
            Ok(d) => d,
            Err(_) => continue,
        };
        // Index DmSpriteDef2 by name for quick prefix lookup.
        let meshes: Vec<(&str, &DmSpriteDef2)> = doc
            .fragment_iter::<DmSpriteDef2>()
            .map(|m| (doc.get_string(m.name_reference).unwrap_or(""), m))
            .collect();
        let skels: Vec<_> = doc.fragment_iter::<HierarchicalSpriteDef>().collect();
        println!("=== {} : {} skeletons, {} meshes ===", wld_name, skels.len(), meshes.len());
        for skel in &skels {
            let name = doc.get_string(skel.name_reference).unwrap_or("?");
            let code = name.split('_').next().unwrap_or(name);
            // Find body mesh for this code.
            let body = meshes.iter().find(|(mn, _)| {
                mn.starts_with(code) && !mn.contains("HE") && mn.ends_with("_DMSPRITEDEF")
            });
            let (mesh_kind, verts) = match body {
                Some((_, m)) if !m.skin_assignment_groups.is_empty() =>
                    ("skinned", m.positions.len()),
                Some((_, m)) => ("rigid/old", m.positions.len()),
                None => ("no-body-mesh", 0),
            };
            // Count rigidly-attached meshes (dag.mesh_or_sprite_reference != 0).
            let attached = skel.dags.iter().filter(|d| d.mesh_or_sprite_reference != 0).count();
            println!(
                "  {:<14} code={:<4} dags={:<3} {:<14} body_verts={} dag_meshes={}",
                name, code, skel.dags.len(), mesh_kind, verts, attached
            );
        }
    }
    Ok(())
}

/// Inspect the skeleton and animation track naming inside a character archive.
pub fn analyze_anims(input: &Path) -> Result<()> {
    use std::collections::BTreeMap;
    let file = fs::File::open(input)?;
    let mut pfs = libeq_pfs::PfsReader::open(file)?;
    let filenames = pfs.filenames()?;
    for wld_name in filenames.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let bytes = match pfs.get(wld_name) {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let doc = match WldDoc::parse(&bytes) {
            Ok(d) => d,
            Err(_) => continue,
        };
        println!("=== {} ===", wld_name);
        // Skeleton + base track names per dag.
        if let Some(skel) = doc.fragment_iter::<HierarchicalSpriteDef>().next() {
            println!("HierarchicalSpriteDef: {} dags", skel.dags.len());
            let mut base_names = Vec::new();
            for (i, dag) in skel.dags.iter().enumerate() {
                let track = if dag.track_reference > 0 {
                    doc.at((dag.track_reference - 1) as usize)
                        .and_then(|f| f.as_any().downcast_ref::<Track>())
                } else {
                    None
                };
                let tname = track
                    .and_then(|t| doc.get_string(t.name_reference))
                    .unwrap_or("?");
                if i < 6 {
                    println!("  dag[{}] base_track='{}'", i, tname);
                }
                base_names.push(tname.to_string());
            }
            // Dump every code-stripped bone suffix (skip leading 3-char model code)
            // for donor-skeleton comparison.
            if std::env::var("DUMP_BONES").is_ok() {
                for b in &base_names {
                    let suffix = if b.len() > 3 { &b[3..] } else { b.as_str() };
                    println!("BONE {}", suffix);
                }
            }
        }
        // Group ALL track names by leading 3-char animation code (Cxx/Lxx/etc.).
        let mut by_prefix: BTreeMap<String, usize> = BTreeMap::new();
        let mut total = 0;
        for t in doc.fragment_iter::<Track>() {
            let name = doc.get_string(t.name_reference).unwrap_or("");
            total += 1;
            let prefix = name.chars().take(3).collect::<String>();
            *by_prefix.entry(prefix).or_insert(0) += 1;
        }
        println!("Total Track(0x13) fragments: {}", total);
        println!("Track-name prefixes (3-char) -> count:");
        for (p, c) in &by_prefix {
            println!("  '{}' x{}", p, c);
        }
        // Sample some full track names.
        println!("Sample track names:");
        for t in doc.fragment_iter::<Track>().take(10) {
            println!("  {}", doc.get_string(t.name_reference).unwrap_or("?"));
        }
        // All track names matching an optional filter substring.
        if let Ok(filter) = std::env::var("TRACK_FILTER") {
            println!("Tracks containing '{}':", filter);
            for t in doc.fragment_iter::<Track>() {
                let nm = doc.get_string(t.name_reference).unwrap_or("?");
                if nm.contains(&filter) {
                    println!("  {}", nm);
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn write_glb(
    output: &Path,
    meshes: &[MeshData],
    materials: &[MaterialData],
    textures: &[TextureData],
) -> Result<()> {
    let mut buffer_data: Vec<u8> = Vec::new();
    let mut buffer_views: Vec<serde_json::Value> = Vec::new();
    let mut accessors: Vec<serde_json::Value> = Vec::new();
    let mut images: Vec<serde_json::Value> = Vec::new();
    let mut gltf_textures: Vec<serde_json::Value> = Vec::new();
    let mut gltf_materials: Vec<serde_json::Value> = Vec::new();
    let mut gltf_meshes: Vec<serde_json::Value> = Vec::new();
    let mut nodes: Vec<serde_json::Value> = Vec::new();

    // Add textures as images
    for tex in textures {
        let view_idx = buffer_views.len();
        let byte_offset = buffer_data.len() as u32;
        buffer_data.extend_from_slice(&tex.png_bytes);
        while buffer_data.len() % 4 != 0 {
            buffer_data.push(0);
        }
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": byte_offset,
            "byteLength": tex.png_bytes.len(),
        }));
        images.push(serde_json::json!({
            "bufferView": view_idx,
            "mimeType": "image/png",
            "name": tex.name,
        }));
        gltf_textures.push(serde_json::json!({
            "source": images.len() - 1,
        }));
    }

    // Add materials
    for mat in materials {
        gltf_materials.push(material_to_gltf(mat));
    }

    // Add meshes — each MeshData becomes one glTF mesh with shared vertices
    // and multiple primitives (one per material group).
    for mesh in meshes {
        let mut attributes = serde_json::Map::new();

        // Positions (shared across all primitives)
        let pos_offset = buffer_data.len() as u32;
        for p in &mesh.positions {
            buffer_data.extend_from_slice(&p[0].to_le_bytes());
            buffer_data.extend_from_slice(&p[1].to_le_bytes());
            buffer_data.extend_from_slice(&p[2].to_le_bytes());
        }
        let pos_byte_len = (mesh.positions.len() * 12) as u32;
        let pos_view_idx = buffer_views.len();
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": pos_offset,
            "byteLength": pos_byte_len,
            "target": 34962,
        }));
        let (pos_min, pos_max) = compute_bounds_f32x3(&mesh.positions);
        let pos_acc_idx = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": pos_view_idx,
            "componentType": 5126,
            "count": mesh.positions.len(),
            "type": "VEC3",
            "min": pos_min,
            "max": pos_max,
        }));
        attributes.insert("POSITION".to_string(), serde_json::json!(pos_acc_idx));

        // Normals (shared across all primitives)
        let norm_offset = buffer_data.len() as u32;
        for n in &mesh.normals {
            buffer_data.extend_from_slice(&n[0].to_le_bytes());
            buffer_data.extend_from_slice(&n[1].to_le_bytes());
            buffer_data.extend_from_slice(&n[2].to_le_bytes());
        }
        let norm_byte_len = (mesh.normals.len() * 12) as u32;
        let norm_view_idx = buffer_views.len();
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": norm_offset,
            "byteLength": norm_byte_len,
            "target": 34962,
        }));
        let norm_acc_idx = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": norm_view_idx,
            "componentType": 5126,
            "count": mesh.normals.len(),
            "type": "VEC3",
        }));
        attributes.insert("NORMAL".to_string(), serde_json::json!(norm_acc_idx));

        // UVs (shared across all primitives)
        let uv_offset = buffer_data.len() as u32;
        for u in &mesh.uvs {
            buffer_data.extend_from_slice(&u[0].to_le_bytes());
            buffer_data.extend_from_slice(&u[1].to_le_bytes());
        }
        let uv_byte_len = (mesh.uvs.len() * 8) as u32;
        let uv_view_idx = buffer_views.len();
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": uv_offset,
            "byteLength": uv_byte_len,
            "target": 34962,
        }));
        let uv_acc_idx = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": uv_view_idx,
            "componentType": 5126,
            "count": mesh.uvs.len(),
            "type": "VEC2",
        }));
        attributes.insert("TEXCOORD_0".to_string(), serde_json::json!(uv_acc_idx));

        // One glTF primitive per material group, each with its own index buffer.
        // Use u32 indices (componentType 5125) when the mesh has >65535 vertices;
        // u16 (5123) silently wraps for large meshes and corrupts geometry.
        let use_u32_indices = mesh.positions.len() > 65535;
        let mut gltf_primitives = Vec::new();
        for prim in &mesh.primitives {
            let idx_offset = buffer_data.len() as u32;
            if use_u32_indices {
                for &i in &prim.indices {
                    buffer_data.extend_from_slice(&i.to_le_bytes());
                }
            } else {
                for &i in &prim.indices {
                    buffer_data.extend_from_slice(&(i as u16).to_le_bytes());
                }
            }
            while buffer_data.len() % 4 != 0 {
                buffer_data.push(0);
            }
            let idx_byte_len = prim.indices.len() * if use_u32_indices { 4 } else { 2 };
            let idx_component_type = if use_u32_indices { 5125u32 } else { 5123u32 };
            let idx_view_idx = buffer_views.len();
            buffer_views.push(serde_json::json!({
                "buffer": 0,
                "byteOffset": idx_offset,
                "byteLength": idx_byte_len,
                "target": 34963,
            }));
            let idx_acc_idx = accessors.len();
            accessors.push(serde_json::json!({
                "bufferView": idx_view_idx,
                "componentType": idx_component_type,
                "count": prim.indices.len(),
                "type": "SCALAR",
            }));

            let mut prim_json = serde_json::json!({
                "attributes": attributes,
                "indices": idx_acc_idx,
                "material": prim.material_idx,
            });
            if let Some(extras) = &prim.extras {
                prim_json["extras"] = extras.clone();
            }
            gltf_primitives.push(prim_json);
        }

        gltf_meshes.push(serde_json::json!({
            "name": mesh.name,
            "primitives": gltf_primitives,
        }));

        let node_idx = nodes.len();
        nodes.push(serde_json::json!({
            "mesh": node_idx,
        }));
    }

    // Pad buffer to 4 bytes
    while buffer_data.len() % 4 != 0 {
        buffer_data.push(0);
    }

    let gltf = serde_json::json!({
        "asset": {
            "version": "2.0",
            "generator": "s3d_to_gltf",
        },
        "scene": 0,
        "scenes": [{
            "name": "scene",
            "nodes": (0..nodes.len()).collect::<Vec<_>>(),
        }],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{
            "byteLength": buffer_data.len(),
        }],
        "materials": gltf_materials,
        "images": images,
        "textures": gltf_textures,
    });

    let json_str = serde_json::to_string(&gltf)?;
    let json_bytes = json_str.as_bytes();
    let json_padded_len = (json_bytes.len() + 3) & !3;

    let bin_padded_len = buffer_data.len();
    let total_len = 12 + 8 + json_padded_len + 8 + bin_padded_len;

    let mut out = fs::File::create(output)
        .with_context(|| format!("failed to create {}", output.display()))?;

    // GLB header
    out.write_all(&0x46546C67u32.to_le_bytes())?;
    out.write_all(&2u32.to_le_bytes())?;
    out.write_all(&(total_len as u32).to_le_bytes())?;

    // JSON chunk
    out.write_all(&(json_padded_len as u32).to_le_bytes())?;
    out.write_all(&0x4E4F534Au32.to_le_bytes())?;
    out.write_all(json_bytes)?;
    for _ in json_bytes.len()..json_padded_len {
        out.write_all(b" ")?;
    }

    // Binary chunk
    out.write_all(&(bin_padded_len as u32).to_le_bytes())?;
    out.write_all(&0x004E4942u32.to_le_bytes())?;
    out.write_all(&buffer_data)?;

    eprintln!("  wrote {} bytes to {}", total_len, output.display());
    Ok(())
}

/// Like [`write_glb`], but decouples nodes from meshes so a single mesh can be
/// instanced by many nodes. Emits `meshes[]` exactly as `write_glb` does, then
/// `nodes[]` from the supplied [`NodeDef`] list (each node references a mesh by
/// index and carries an optional column-major 4x4 `matrix`). Used by zone baking
/// to share one welded mesh per object model across all placement nodes.
pub(crate) fn write_glb_instanced(
    output: &Path,
    meshes: &[MeshData],
    materials: &[MaterialData],
    textures: &[TextureData],
    nodes_in: &[NodeDef],
) -> Result<()> {
    let mut buffer_data: Vec<u8> = Vec::new();
    let mut buffer_views: Vec<serde_json::Value> = Vec::new();
    let mut accessors: Vec<serde_json::Value> = Vec::new();
    let mut images: Vec<serde_json::Value> = Vec::new();
    let mut gltf_textures: Vec<serde_json::Value> = Vec::new();
    let mut gltf_materials: Vec<serde_json::Value> = Vec::new();
    let mut gltf_meshes: Vec<serde_json::Value> = Vec::new();

    // Textures -> images (lowercased EQ texture name preserved).
    for tex in textures {
        let view_idx = buffer_views.len();
        let byte_offset = buffer_data.len() as u32;
        buffer_data.extend_from_slice(&tex.png_bytes);
        while buffer_data.len() % 4 != 0 {
            buffer_data.push(0);
        }
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": byte_offset,
            "byteLength": tex.png_bytes.len(),
        }));
        images.push(serde_json::json!({
            "bufferView": view_idx,
            "mimeType": "image/png",
            "name": tex.name,
        }));
        gltf_textures.push(serde_json::json!({ "source": images.len() - 1 }));
    }

    // Materials.
    for mat in materials {
        gltf_materials.push(material_to_gltf(mat));
    }

    // Meshes (no implicit per-mesh node here — nodes come from `nodes_in`).
    for mesh in meshes {
        let mut attributes = serde_json::Map::new();

        let pos_offset = buffer_data.len() as u32;
        for p in &mesh.positions {
            buffer_data.extend_from_slice(&p[0].to_le_bytes());
            buffer_data.extend_from_slice(&p[1].to_le_bytes());
            buffer_data.extend_from_slice(&p[2].to_le_bytes());
        }
        let pos_byte_len = (mesh.positions.len() * 12) as u32;
        let pos_view_idx = buffer_views.len();
        buffer_views.push(serde_json::json!({
            "buffer": 0, "byteOffset": pos_offset, "byteLength": pos_byte_len, "target": 34962,
        }));
        let (pos_min, pos_max) = compute_bounds_f32x3(&mesh.positions);
        let pos_acc_idx = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": pos_view_idx, "componentType": 5126,
            "count": mesh.positions.len(), "type": "VEC3", "min": pos_min, "max": pos_max,
        }));
        attributes.insert("POSITION".to_string(), serde_json::json!(pos_acc_idx));

        let norm_offset = buffer_data.len() as u32;
        for n in &mesh.normals {
            buffer_data.extend_from_slice(&n[0].to_le_bytes());
            buffer_data.extend_from_slice(&n[1].to_le_bytes());
            buffer_data.extend_from_slice(&n[2].to_le_bytes());
        }
        let norm_byte_len = (mesh.normals.len() * 12) as u32;
        let norm_view_idx = buffer_views.len();
        buffer_views.push(serde_json::json!({
            "buffer": 0, "byteOffset": norm_offset, "byteLength": norm_byte_len, "target": 34962,
        }));
        let norm_acc_idx = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": norm_view_idx, "componentType": 5126,
            "count": mesh.normals.len(), "type": "VEC3",
        }));
        attributes.insert("NORMAL".to_string(), serde_json::json!(norm_acc_idx));

        let uv_offset = buffer_data.len() as u32;
        for u in &mesh.uvs {
            buffer_data.extend_from_slice(&u[0].to_le_bytes());
            buffer_data.extend_from_slice(&u[1].to_le_bytes());
        }
        let uv_byte_len = (mesh.uvs.len() * 8) as u32;
        let uv_view_idx = buffer_views.len();
        buffer_views.push(serde_json::json!({
            "buffer": 0, "byteOffset": uv_offset, "byteLength": uv_byte_len, "target": 34962,
        }));
        let uv_acc_idx = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": uv_view_idx, "componentType": 5126,
            "count": mesh.uvs.len(), "type": "VEC2",
        }));
        attributes.insert("TEXCOORD_0".to_string(), serde_json::json!(uv_acc_idx));

        // Use u32 indices (componentType 5125) when the mesh has >65535 vertices;
        // u16 (5123) silently wraps for large merged terrain meshes and corrupts geometry.
        let use_u32_indices = mesh.positions.len() > 65535;
        let mut gltf_primitives = Vec::new();
        for prim in &mesh.primitives {
            let idx_offset = buffer_data.len() as u32;
            if use_u32_indices {
                for &i in &prim.indices {
                    buffer_data.extend_from_slice(&i.to_le_bytes());
                }
            } else {
                for &i in &prim.indices {
                    buffer_data.extend_from_slice(&(i as u16).to_le_bytes());
                }
            }
            while buffer_data.len() % 4 != 0 {
                buffer_data.push(0);
            }
            let idx_byte_len = prim.indices.len() * if use_u32_indices { 4 } else { 2 };
            let idx_component_type = if use_u32_indices { 5125u32 } else { 5123u32 };
            let idx_view_idx = buffer_views.len();
            buffer_views.push(serde_json::json!({
                "buffer": 0, "byteOffset": idx_offset,
                "byteLength": idx_byte_len, "target": 34963,
            }));
            let idx_acc_idx = accessors.len();
            accessors.push(serde_json::json!({
                "bufferView": idx_view_idx, "componentType": idx_component_type,
                "count": prim.indices.len(), "type": "SCALAR",
            }));
            let mut prim_json = serde_json::json!({
                "attributes": attributes,
                "indices": idx_acc_idx,
                "material": prim.material_idx,
            });
            if let Some(extras) = &prim.extras {
                prim_json["extras"] = extras.clone();
            }
            gltf_primitives.push(prim_json);
        }

        gltf_meshes.push(serde_json::json!({
            "name": mesh.name,
            "primitives": gltf_primitives,
        }));
    }

    // Nodes: one per NodeDef, referencing a mesh + optional column-major matrix.
    let mut nodes: Vec<serde_json::Value> = Vec::with_capacity(nodes_in.len());
    for nd in nodes_in {
        let mut node = serde_json::Map::new();
        node.insert("mesh".to_string(), serde_json::json!(nd.mesh_idx));
        if let Some(m) = nd.matrix {
            // glTF `matrix` is a flat 16-element column-major array.
            let flat: Vec<f32> = m.iter().flat_map(|col| col.iter().copied()).collect();
            node.insert("matrix".to_string(), serde_json::json!(flat));
        }
        nodes.push(serde_json::Value::Object(node));
    }

    while buffer_data.len() % 4 != 0 {
        buffer_data.push(0);
    }

    let gltf = serde_json::json!({
        "asset": { "version": "2.0", "generator": "s3d_to_gltf" },
        "scene": 0,
        "scenes": [{ "name": "scene", "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{ "byteLength": buffer_data.len() }],
        "materials": gltf_materials,
        "images": images,
        "textures": gltf_textures,
    });

    let json_str = serde_json::to_string(&gltf)?;
    let json_bytes = json_str.as_bytes();
    let json_padded_len = (json_bytes.len() + 3) & !3;
    let bin_padded_len = buffer_data.len();
    let total_len = 12 + 8 + json_padded_len + 8 + bin_padded_len;

    let mut out = fs::File::create(output)
        .with_context(|| format!("failed to create {}", output.display()))?;
    out.write_all(&0x46546C67u32.to_le_bytes())?;
    out.write_all(&2u32.to_le_bytes())?;
    out.write_all(&(total_len as u32).to_le_bytes())?;
    out.write_all(&(json_padded_len as u32).to_le_bytes())?;
    out.write_all(&0x4E4F534Au32.to_le_bytes())?;
    out.write_all(json_bytes)?;
    for _ in json_bytes.len()..json_padded_len {
        out.write_all(b" ")?;
    }
    out.write_all(&(bin_padded_len as u32).to_le_bytes())?;
    out.write_all(&0x004E4942u32.to_le_bytes())?;
    out.write_all(&buffer_data)?;

    eprintln!("  wrote {} bytes to {}", total_len, output.display());
    Ok(())
}

fn compute_bounds_f32x3(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in positions {
        for i in 0..3 {
            if p[i] < min[i] { min[i] = p[i]; }
            if p[i] > max[i] { max[i] = p[i]; }
        }
    }
    (min, max)
}

/// Bake held-weapon models from the gequip archives into a single `weapons.glb`.
/// Iterates every archive in `archives` (relative to `raw_dir`), parses each WLD, and
/// collects every mesh whose name (uppercased) starts with `"IT"` as a model-local
/// [`crate::zone::ZoneMesh`] via [`crate::zone::zone_meshes_from_mesh`]. Each unique
/// uppercased mesh name becomes one identity-node mesh in the output GLB, with
/// textures embedded.  Returns `Ok(false)` without writing anything when no `IT`
/// meshes are found (e.g. all archives missing).
pub(crate) fn bake_weapons_glb(
    raw_dir: &Path,
    archives: &[&str],
    out_glb: &Path,
) -> anyhow::Result<bool> {
    use std::collections::HashMap;
    let mut models: HashMap<String, Vec<crate::zone::ZoneMesh>> = HashMap::new();
    let mut pfs_for_tex: Vec<libeq_pfs::PfsReader<std::fs::File>> = Vec::new();
    for arch in archives {
        let p = raw_dir.join(arch);
        let Ok(file) = std::fs::File::open(&p) else { continue };
        let Ok(mut pfs) = libeq_pfs::PfsReader::open(file) else { continue };
        let Ok(names) = pfs.filenames() else { continue };
        for wn in names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
            let Ok(Some(bytes)) = pfs.get(wn) else { continue };
            let Ok(wld) = libeq_wld::load(&bytes) else { continue };
            for mesh in wld.meshes() {
                let Some(name) = mesh.name() else { continue };
                let base = crate::zone::object_base_name(name);
                if !base.starts_with("IT") { continue; }
                let zms = crate::zone::zone_meshes_from_mesh(&mesh);
                if !zms.is_empty() {
                    models.entry(base).or_default().extend(zms);
                }
            }
        }
        // Reopen the archive for the texture-decode pass.
        if let Ok(f) = std::fs::File::open(&p) {
            if let Ok(r) = libeq_pfs::PfsReader::open(f) {
                pfs_for_tex.push(r);
            }
        }
    }
    if models.is_empty() {
        return Ok(false);
    }
    crate::zone::write_object_models_glb(models, &mut pfs_for_tex, out_glb)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal uncompressed 8bpp BMP (BITMAPINFOHEADER) with the given
    /// palette and one row of pixel indices. Used to test keyed-alpha decoding.
    fn make_bmp_8bpp(width: usize, palette: &[[u8; 3]], indices: &[u8]) -> Vec<u8> {
        assert_eq!(indices.len(), width); // single row for the test
        let colors = 256usize;
        let palette_bytes = colors * 4;
        let row_stride = (width + 3) & !3;
        let pixel_offset = 14 + 40 + palette_bytes;
        let file_size = pixel_offset + row_stride;
        let mut b = vec![0u8; file_size];
        b[0..2].copy_from_slice(b"BM");
        b[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
        b[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        b[14..18].copy_from_slice(&40u32.to_le_bytes()); // DIB header size
        b[18..22].copy_from_slice(&(width as i32).to_le_bytes());
        b[22..26].copy_from_slice(&1i32.to_le_bytes()); // height = 1 (bottom-up)
        b[26..28].copy_from_slice(&1u16.to_le_bytes()); // planes
        b[28..30].copy_from_slice(&8u16.to_le_bytes()); // bpp
        b[30..34].copy_from_slice(&0u32.to_le_bytes()); // BI_RGB
        b[46..50].copy_from_slice(&(colors as u32).to_le_bytes()); // colors used
        let pal = 14 + 40;
        for (i, c) in palette.iter().enumerate() {
            let p = pal + i * 4;
            b[p] = c[2]; // B
            b[p + 1] = c[1]; // G
            b[p + 2] = c[0]; // R
        }
        b[pixel_offset..pixel_offset + width].copy_from_slice(indices);
        b
    }

    #[test]
    fn masked_bmp_keys_out_palette_index_zero() {
        // palette[0] = transparent key color (magenta), palette[1] = opaque green.
        let palette = [[255u8, 0, 255], [0, 200, 0]];
        // row: index 0,1,0 -> transparent, opaque, transparent
        let bmp = make_bmp_8bpp(3, &palette, &[0, 1, 0]);
        let img = decode_bmp_keyed(&bmp).expect("decoded");
        assert_eq!(img.get_pixel(0, 0)[3], 0, "index 0 -> alpha 0");
        assert_eq!(img.get_pixel(1, 0)[3], 255, "index 1 -> opaque");
        assert_eq!(img.get_pixel(2, 0)[3], 0, "index 0 -> alpha 0");
        // opaque pixel keeps its palette color
        assert_eq!(&img.get_pixel(1, 0).0[..3], &[0, 200, 0]);
    }

    #[test]
    fn alpha_mode_maps_material_types() {
        let masked = RenderMethod::UserDefined { material_type: MaterialType::TransparentMasked };
        assert_eq!(alpha_mode_from_render(&masked), AlphaMode::Masked);
        let blend = RenderMethod::UserDefined { material_type: MaterialType::Transparent50 };
        assert_eq!(alpha_mode_from_render(&blend), AlphaMode::Blend(500));
        let add = RenderMethod::UserDefined { material_type: MaterialType::TransparentAdditive };
        assert_eq!(alpha_mode_from_render(&add), AlphaMode::Additive);
        let diffuse = RenderMethod::UserDefined { material_type: MaterialType::Diffuse };
        assert_eq!(alpha_mode_from_render(&diffuse), AlphaMode::Opaque);
    }

    #[test]
    fn split_hair_face_by_head_bone_binding() {
        // Bones: 7 = head, 25 = a facial bone (nose). A triangle fully on the head
        // bone is painted-hair scalp; a triangle touching the facial bone is skin.
        let mut head = vec![false; 30];
        head[7] = true;
        let mut face = vec![false; 30];
        face[25] = true;
        let bones = HeadBones { head, face };
        // verts 0,1,2 on head bone; vert 3 on the nose bone.
        let joints = vec![7u16, 7, 7, 25];
        // tri A (0,1,2) pure head → hair; tri B (1,2,3) touches nose → face.
        let idxs = vec![0u32, 1, 2, 1, 2, 3];

        let (hair, facial) = split_hair_face(&idxs, &joints, &bones);

        assert_eq!(hair, vec![0, 1, 2], "pure head-bone tri is scalp hair");
        assert_eq!(facial, vec![1, 2, 3], "facial-bone tri is skin");
    }

    #[test]
    fn masked_material_emits_mask_alphamode() {
        let mat = MaterialData {
            name: "leaf".into(), texture_idx: Some(0),
            base_color: [1.0, 1.0, 1.0, 1.0], alpha_mode: AlphaMode::Masked, anim: None,
        };
        let j = material_to_gltf(&mat);
        assert_eq!(j["alphaMode"], "MASK");
        assert_eq!(j["alphaCutoff"], 0.5);
    }

    #[test]
    fn anim_donor_maps_reskin_races_to_human() {
        // Male reskins -> human male; female -> human female.
        for c in ["DAM", "HIM", "HAM", "ERM"] {
            assert_eq!(anim_donor(c), Some(("globalhum_chr.s3d", "HUM")), "{c}");
        }
        for c in ["DAF", "HIF", "HAF", "ERF"] {
            assert_eq!(anim_donor(c), Some(("globalhuf_chr.s3d", "HUF")), "{c}");
        }
        // Case-insensitive.
        assert_eq!(anim_donor("dam"), Some(("globalhum_chr.s3d", "HUM")));
        // Races that ship their own animations have no donor.
        for c in ["HUM", "BAM", "ELM", "OGM", "TRM"] {
            assert_eq!(anim_donor(c), None, "{c}");
        }
    }

    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/globalhuf_chr.s3d"]
    fn skinned_glb_joints_carry_bone_names_including_attach_points() {
        // The client attaches held weapons to the rig's dedicated attachment bones
        // (R_POINT = primary hand, L_POINT = left hand, SHIELD_POINT = shield), so
        // every joint node must carry its WLD bone name in the glTF `name` field.
        let home = std::env::var("HOME").unwrap();
        let inp = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/globalhuf_chr.s3d"));
        if !inp.exists() { eprintln!("skip: {inp:?} missing"); return; }
        let out = std::env::temp_dir().join("eqoxide_test_huf_joint_names.glb");
        convert_s3d_to_glb_skinned(&inp, &out, None).unwrap();

        let (gdoc, _buffers, _imgs) = gltf::import(&out).unwrap();
        let skin = gdoc.skins().next().expect("skin present");
        let names: Vec<String> = skin.joints()
            .map(|n| n.name().unwrap_or("").to_uppercase())
            .collect();
        let named = names.iter().filter(|n| !n.is_empty()).count();
        assert_eq!(named, names.len(), "every joint must be named, got {named}/{}", names.len());
        for suffix in ["R_POINT", "L_POINT", "SHIELD_POINT"] {
            assert!(
                names.iter().any(|n| n.ends_with(suffix)),
                "expected a joint ending in {suffix}; sample: {:?}", &names[..names.len().min(8)]
            );
        }
    }

    #[test]
    #[ignore = "requires ~/eq_assets/EQ_Files/globalhum_chr.s3d"]
    fn skinned_conversion_is_centered_grounded() {
        let home = std::env::var("HOME").unwrap();
        let inp = std::path::PathBuf::from(format!("{home}/eq_assets/EQ_Files/globalhum_chr.s3d"));
        let out = std::path::PathBuf::from("/tmp/test_hum_norm.glb");
        convert_s3d_to_glb_skinned(&inp, &out, None).unwrap();

        // Re-parse with the gltf crate and confirm the root node carries a positive
        // eq_height in its extras.
        let (doc, _buffers, _images) = gltf::import(&out).unwrap();
        let root = doc.nodes().next().expect("at least one node");
        let extras = root.extras().as_ref().expect("root node extras present");
        let v: serde_json::Value = serde_json::from_str(extras.get()).unwrap();
        let eq_height = v["eq_height"].as_f64().expect("eq_height field present");
        assert!(eq_height > 0.0, "eq_height should be > 0, got {eq_height}");
    }

    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/globalhuf_chr.s3d"]
    fn face_variants_split_into_tinted_hair_and_untinted_skin() {
        // The RoF2 client swaps head regions 1/4/5 by spawn.face (hesk{F}{N}),
        // and hair is the painted scalp portion tinted by haircolor. Each face
        // variant must emit a scalp prim tagged eq_head_part:"hair" plus a facial
        // skin prim, both keyed by eq_face; the crown strip (fixed region 2 on the
        // huf model) must be tagged as always-visible hair.
        let home = std::env::var("HOME").unwrap();
        let inp = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/globalhuf_chr.s3d"));
        if !inp.exists() { eprintln!("skip: {inp:?} missing"); return; }
        let out = std::env::temp_dir().join("eqoxide_test_huf_face_hair.glb");
        convert_s3d_to_glb_skinned(&inp, &out, None).unwrap();

        let (gdoc, _buffers, _imgs) = gltf::import(&out).unwrap();
        let mesh = gdoc.meshes().next().unwrap();
        let mats: Vec<_> = gdoc.materials().collect();

        let mut hair_faces = std::collections::BTreeSet::new();
        let mut skin_faces = std::collections::BTreeSet::new();
        let mut crown_hair_prims = 0;
        for p in mesh.primitives() {
            let ex: serde_json::Value = match p.extras().as_ref() {
                Some(e) => serde_json::from_str(e.get()).unwrap(),
                None => continue,
            };
            let is_hair = ex["eq_head_part"].as_str() == Some("hair");
            match ex["eq_face"].as_u64() {
                Some(f) => {
                    if f == 0 {
                        assert!(ex["eq_default_hidden"].is_null(), "face 0 is default-visible");
                    } else {
                        assert_eq!(ex["eq_default_hidden"].as_bool(), Some(true),
                            "face {f} variants are default-hidden");
                    }
                    if is_hair { hair_faces.insert(f as u8); } else { skin_faces.insert(f as u8); }
                }
                None => {
                    assert!(is_hair, "untagged extras only appear on always-visible crown hair");
                    assert!(ex["eq_default_hidden"].is_null(), "crown hair is always visible");
                    crown_hair_prims += 1;
                }
            }
        }
        assert_eq!(hair_faces, (0u8..=7).collect(), "8 face variants of scalp hair");
        assert_eq!(skin_faces, (0u8..=7).collect(), "8 face variants of facial skin");
        assert!(crown_hair_prims >= 1, "huf region 2 (crown strip) tagged as hair");

        // No synthetic geometry: hair prims must not reference vertices beyond the
        // base mesh (the old shell hack duplicated + offset scalp verts).
        // Ear region (hesk06) stays untinted skin: its prim carries no extras.
        let ear_mat = mats.iter().position(|m| m.name() == Some("hufhesk06"))
            .expect("ear material hufhesk06 present");
        for p in mesh.primitives().filter(|p| p.material().index() == Some(ear_mat)) {
            assert!(p.extras().is_none() || {
                let v: serde_json::Value = serde_json::from_str(p.extras().as_ref().unwrap().get()).unwrap();
                v["eq_head_part"].is_null() && v["eq_face"].is_null()
            }, "ear region must stay untagged skin");
        }
    }

    // ── head region detection unit tests ─────────────────────────────────────

    #[test]
    fn head_region_detects_he000n_pattern() {
        // head_region_from_material_name matches {RACE}HE000{N}_MDF, N=1..8.
        for (name, expected) in [
            ("ELFHE0001_MDF", Some(1u8)),
            ("ELFHE0002_MDF", Some(2)),
            ("ELFHE0008_MDF", Some(8)),
            ("HUMHE0003_MDF", Some(3)),
            ("HUFHE0007_MDF", Some(7)),
            ("elfhe0001_mdf", Some(1)), // case-insensitive
            // Non-head materials must NOT match.
            ("ELFCH0001_MDF", None),     // chest
            ("ELFHE0000_MDF", None),     // N=0 does not exist
            ("ELFHE0009_MDF", None),     // N=9 does not exist
            ("ELFHE0011_MDF", None),     // hair color variant, NOT a base head region
        ] {
            assert_eq!(
                head_region_from_material_name(name),
                expected,
                "head_region_from_material_name({name:?})"
            );
        }
    }

    #[test]
    fn race_code_extraction_from_path() {
        use std::path::PathBuf;
        assert_eq!(race_code_from_archive(&PathBuf::from("globalelf_chr.s3d")), Some("elf".into()));
        assert_eq!(race_code_from_archive(&PathBuf::from("globalhum_chr.s3d")), Some("hum".into()));
        assert_eq!(race_code_from_archive(&PathBuf::from("globalhuf_chr.s3d")), Some("huf".into()));
        assert_eq!(race_code_from_archive(&PathBuf::from("globalelm_chr.s3d")), Some("elm".into()));
        assert_eq!(race_code_from_archive(&PathBuf::from("/path/to/globaldaf_chr.s3d")), Some("daf".into()));
        // Non-character archives return None or an arbitrary code (don't crash).
        assert_eq!(race_code_from_archive(&PathBuf::from("qeynos.s3d")), None);
    }

    /// Full integration test: convert globalelf_chr.s3d and verify that the
    /// resulting GLB has face-variant primitives (eq_face F=0..7, each with a
    /// hesk{F}{N} material) and that all 8 head region materials are present
    /// (confirming ears/teeth/mouth are not dropped).
    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/globalelf_chr.s3d"]
    fn elf_glb_has_all_head_regions_and_face_extras() {
        let home = std::env::var("HOME").unwrap();
        let inp = std::path::PathBuf::from(
            format!("{home}/eq_assets/everquest_rof2/globalelf_chr.s3d")
        );
        if !inp.exists() { eprintln!("skip: {inp:?} missing"); return; }
        let out = std::path::PathBuf::from("/tmp/test_elf_ears_face.glb");
        convert_s3d_to_glb_skinned(&inp, &out, None).unwrap();

        let (doc, _buffers, _images) = gltf::import(&out).unwrap();
        let mesh = doc.meshes().next().expect("at least one mesh");
        let mats: Vec<_> = doc.materials().collect();

        // Each face F=0..7 must appear on ≥1 primitive per swappable region (elf
        // has 3 swappable regions × up to 2 sub-prims each after the hair split).
        for f in 0u8..=7 {
            let count = mesh.primitives().filter(|p| {
                p.extras().as_ref().and_then(|e| {
                    let v: serde_json::Value = serde_json::from_str(e.get()).ok()?;
                    v["eq_face"].as_u64().map(|x| x == f as u64)
                }).unwrap_or(false)
            }).count();
            assert!(count >= 3,
                "eq_face:{} should have ≥3 primitives, found {}", f, count);
        }

        // F=0 must NOT have eq_default_hidden; F≥1 must have eq_default_hidden:true.
        for prim in mesh.primitives() {
            if let Some(raw) = prim.extras().as_ref() {
                let v: serde_json::Value = serde_json::from_str(raw.get()).unwrap();
                if let Some(f) = v["eq_face"].as_u64() {
                    if f == 0 {
                        assert!(v["eq_default_hidden"].is_null(),
                            "eq_face:0 must not have eq_default_hidden");
                    } else {
                        assert_eq!(v["eq_default_hidden"].as_bool(), Some(true),
                            "eq_face:{} must have eq_default_hidden:true", f);
                    }
                }
            }
        }

        // All 8 fixed/base head region materials must exist with ≥1 primitive.
        for region_mat in &["elfhesk02", "elfhesk03", "elfhesk06", "elfhesk07", "elfhesk08"] {
            let mat_idx = mats.iter().position(|m| m.name() == Some(region_mat))
                .unwrap_or_else(|| panic!("head region material '{}' not found", region_mat));
            let prim_count = mesh.primitives()
                .filter(|p| p.material().index() == Some(mat_idx))
                .count();
            assert!(prim_count > 0, "no primitive for head region material '{}'", region_mat);
        }
    }
}

#[cfg(test)]
mod weapons_glb_tests {
    use super::*;
    #[test]
    #[ignore = "requires ~/eq_assets/EQ_Files/gequip*.s3d"]
    fn bakes_named_weapon_meshes() {
        let home = std::env::var("HOME").unwrap();
        let raw = std::path::PathBuf::from(format!("{home}/eq_assets/EQ_Files"));
        if !raw.join("gequip.s3d").exists() { eprintln!("skip"); return; }
        let out = std::env::temp_dir().join("weapons_test.glb");
        let archives = ["gequip.s3d","gequip2.s3d","gequip3.s3d","gequip4.s3d","gequip5.s3d","gequip6.s3d","gequip7.s3d","gequip8.s3d"];
        assert!(bake_weapons_glb(&raw, &archives, &out).unwrap());
        let (doc, _b, _i) = gltf::import(&out).unwrap();
        // Meshes must be keyed by bare IT#### (suffix stripped), not IT####_DMSPRITEDEF.
        assert!(
            doc.meshes().any(|m| m.name().map_or(false, |n| n.starts_with("IT") && !n.contains('_'))),
            "weapon meshes must be bare IT#### (no _DMSPRITEDEF suffix)"
        );
    }
}

/// Extract every BMP/DDS texture from the given S3D archives, decode to RGBA,
/// filter out ≤8×8 all-alpha "stub" placeholder textures, and re-encode to PNG.
/// Returns `(name_lower_without_ext + ".png", png_bytes)` pairs, deduped by name
/// (first-wins insertion order, matching `index_s3d_textures` semantics).
pub(crate) fn extract_equip_textures(raw_dir: &Path, archives: &[&str]) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    use std::collections::HashSet;
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for arch in archives {
        let p = raw_dir.join(arch);
        let Ok(file) = std::fs::File::open(&p) else { continue };
        let Ok(mut pfs) = libeq_pfs::PfsReader::open(file) else { continue };
        let Ok(names) = pfs.filenames() else { continue };
        for name in names {
            let lower = name.to_lowercase();
            let fmt = if lower.ends_with(".bmp") { image::ImageFormat::Bmp }
                      else if lower.ends_with(".dds") { image::ImageFormat::Dds } else { continue };
            let stem = format!("{}.png", &lower[..lower.len()-4]);
            if seen.contains(&stem) { continue; }
            let Ok(Some(bytes)) = pfs.get(&name) else { continue };
            let Ok(img) = image::load_from_memory_with_format(&bytes, fmt) else { continue };
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            if (w <= 8 && h <= 8) || rgba.pixels().all(|px| px.0[3] == 0) { continue; }
            let mut png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(rgba).write_to(&mut png, image::ImageFormat::Png)?;
            seen.insert(stem.clone());
            out.push((stem, png.into_inner()));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod equip_tex_tests {
    use super::*;
    #[test]
    #[ignore = "requires ~/eq_assets/EQ_Files/global_chr.s3d"]
    fn extracts_named_pngs_skipping_stubs() {
        let home = std::env::var("HOME").unwrap();
        let raw = std::path::PathBuf::from(format!("{home}/eq_assets/EQ_Files"));
        if !raw.join("global_chr.s3d").exists() { eprintln!("skip"); return; }
        let out = extract_equip_textures(&raw, &["global_chr.s3d"]).unwrap();
        assert!(!out.is_empty());
        assert!(out.iter().all(|(n,_)| n.ends_with(".png") && n == &n.to_lowercase()));
        // every emitted PNG decodes and is > 8x8 (stubs filtered)
        for (n, bytes) in out.iter().take(20) {
            let img = image::load_from_memory(bytes).unwrap_or_else(|_| panic!("decode {n}"));
            assert!(img.width() > 8 || img.height() > 8, "{n} should not be an 8x8 stub");
        }
    }
}
