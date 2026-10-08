//! Source-independent static visual input and enforced GLB artifact boundary.
//!
//! Importers resolve their source semantics before constructing this scene. This
//! baseline supports primary UVs, static PNGs and ordinary glTF alpha semantics;
//! additive effects, texture sequences, secondary UVs, skins, animation, collision
//! and regions require separate contracts and are not represented by these types.
use std::{collections::BTreeMap, io::Cursor, path::Path};

use anyhow::{ensure, Context, Result};
use glam::{DMat4, DVec3, Mat4, Vec3};

use crate::{
    compatibility::{valid_hash, ReaderRequirements},
    convert::{GlbMetadata, MeshData, NodeDef, PrimitiveColors, PrimitiveData, TextureData},
};

/// Resolved scene in server geometry axes with CCW winding. No actor-origin offset.
#[derive(Debug, Clone, Default)]
pub struct StaticScene {
    pub meshes: Vec<StaticMesh>,
    pub materials: Vec<StaticMaterial>,
    pub textures: Vec<StaticTexture>,
    pub instances: Vec<StaticInstance>,
}

#[derive(Debug, Clone)]
pub struct StaticMesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    /// Unit-length normals in the same server geometry axes as positions.
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub primitives: Vec<StaticPrimitive>,
}

#[derive(Debug, Clone)]
pub struct StaticPrimitive {
    pub indices: Vec<u32>,
    pub material_index: usize,
    /// Optional normalized RGBA for the mesh's entire vertex pool, local to this primitive.
    pub colors: Option<Vec<[f32; 4]>>,
}

#[derive(Debug, Clone)]
pub struct StaticMaterial {
    pub name: String,
    pub base_color: [f32; 4],
    pub texture_index: Option<usize>,
    pub alpha_mode: StaticAlphaMode,
    pub double_sided: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StaticAlphaMode {
    Opaque,
    Mask { cutoff: f32 },
    Blend,
}

#[derive(Debug, Clone)]
pub struct StaticTexture {
    pub name: String,
    pub png_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct StaticInstance {
    pub mesh_index: usize,
    /// Column-major affine matrix. Proper rotation, positive uniform scale and translation only.
    pub matrix: [[f32; 4]; 4],
}

/// Validate, convert server `(x,y,z)` to `(x,z,-y)`, and atomically publish a GLB.
///
/// The returned reader requirements reserve this new contract; current production
/// reader advertisements remain unchanged. Unit scale 1 is an authoring convention.
pub fn write_static_visual(
    scene: &StaticScene,
    bake_revision: &str,
    output: &Path,
) -> Result<ReaderRequirements> {
    validate(scene, bake_revision)?;
    let has_colors = scene.meshes.iter()
        .flat_map(|m| &m.primitives)
        .any(|p| p.colors.is_some());
    let requirements = ReaderRequirements {
        reader_version: 2,
        capabilities: if has_colors {
            vec!["static-visual-v1".into(), "vertex-rgba-v1".into()]
        } else {
            vec!["static-visual-v1".into()]
        },
    };
    let meshes: Vec<_> = scene.meshes.iter().map(|m| MeshData {
        name: m.name.clone(),
        positions: m.positions.iter().copied().map(map_axes).collect(),
        normals: m.normals.iter().copied().map(map_axes).collect(),
        uvs: m.uvs.clone(),
        primitives: m.primitives.iter().map(|p| PrimitiveData {
            indices: p.indices.clone(),
            material_idx: p.material_index,
            extras: None,
        }).collect(),
    }).collect();
    let nodes: Vec<_> = scene.instances.iter().map(|i| NodeDef {
        mesh_idx: i.mesh_index,
        matrix: Some(convert_matrix(i.matrix)),
    }).collect();
    let textures: Vec<_> = scene.textures.iter().map(|t| TextureData {
        name: t.name.clone(),
        png_bytes: t.png_bytes.clone(),
    }).collect();
    let materials: Vec<_> = scene.materials.iter().map(material_json).collect();
    let colors: BTreeMap<_, _> = scene.meshes.iter().enumerate().flat_map(|(mi, m)| {
        m.primitives.iter().enumerate().filter_map(move |(pi, p)| {
            p.colors.as_ref().map(|c| ((mi, pi), PrimitiveColors::Rgba(c)))
        })
    }).collect();
    let metadata = GlbMetadata {
        extras: Some(serde_json::json!({"eqoxideAsset": {
            "schemaVersion": 1,
            "role": "visual",
            "coordinateProfile": "eqoxide-static-y-up-v1",
            "unitScale": 1.0,
            "bakeRevision": bake_revision,
            "requirements": requirements,
        }})),
        ..Default::default()
    };
    let parent = output.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temp = tempfile::NamedTempFile::new_in(parent)
        .context("create temporary static visual")?;
    crate::convert::write_glb_instanced_prepared(
        temp.path(), &meshes, &materials, &textures, &nodes, &colors, Some(&metadata),
        "eqoxide-static-visual-v1",
    )?;
    temp.as_file().sync_all().context("sync static visual")?;
    temp.persist(output).context("publish static visual")?;
    Ok(requirements)
}

fn normalized(x: f32) -> bool {
    x.is_finite() && (0.0..=1.0).contains(&x)
}

fn map_axes(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], -p[1]]
}

fn convert_matrix(matrix: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    // C has determinant +1; conjugation preserves proper rotation and CCW winding.
    let c = Mat4::from_cols_array_2d(&[
        [1., 0., 0., 0.], [0., 0., -1., 0.],
        [0., 1., 0., 0.], [0., 0., 0., 1.],
    ]);
    (c * Mat4::from_cols_array_2d(&matrix) * c.transpose()).to_cols_array_2d()
}

fn material_json(m: &StaticMaterial) -> serde_json::Value {
    let mut pbr = serde_json::json!({
        "baseColorFactor": m.base_color, "metallicFactor": 0.0, "roughnessFactor": 1.0,
    });
    if let Some(t) = m.texture_index {
        pbr["baseColorTexture"] = serde_json::json!({"index": t});
    }
    let mut json = serde_json::json!({
        "name": m.name, "pbrMetallicRoughness": pbr, "doubleSided": m.double_sided,
    });
    match m.alpha_mode {
        StaticAlphaMode::Opaque => { json["alphaMode"] = serde_json::json!("OPAQUE"); }
        StaticAlphaMode::Mask { cutoff } => {
            json["alphaMode"] = serde_json::json!("MASK");
            json["alphaCutoff"] = serde_json::json!(cutoff);
        }
        StaticAlphaMode::Blend => { json["alphaMode"] = serde_json::json!("BLEND"); }
    }
    json
}

fn validate(scene: &StaticScene, revision: &str) -> Result<()> {
    ensure!(valid_hash(revision), "bake revision must be 64 lowercase hexadecimal digits");
    ensure!(!scene.instances.is_empty() && !scene.meshes.is_empty(), "static scene must be renderable");
    let mut binary_bytes = 0u64;
    for (mi, m) in scene.meshes.iter().enumerate() {
        ensure!(!m.positions.is_empty() && !m.primitives.is_empty(), "mesh {mi} is empty");
        ensure!(m.positions.len() == m.normals.len() && m.positions.len() == m.uvs.len(),
            "mesh {mi} attribute counts differ");
        account_binary(&mut binary_bytes, m.positions.len(), 32)?; // POSITION, NORMAL, TEXCOORD_0
        ensure!(m.positions.iter().flatten().chain(m.uvs.iter().flatten()).all(|f| f.is_finite()),
            "mesh {mi} has nonfinite attributes");
        ensure!(m.normals.iter().all(|n| {
            let n = Vec3::from_array(*n);
            n.is_finite() && (n.length_squared() - 1.).abs() <= 1e-4
        }), "mesh {mi} normals must be finite and unit length");
        for (pi, p) in m.primitives.iter().enumerate() {
            // Conservatively allow u32 indices even when serialization can use u16.
            account_binary(&mut binary_bytes, p.indices.len(), 4)?;
            ensure!(!p.indices.is_empty() && p.indices.len() % 3 == 0,
                "mesh {mi} primitive {pi} must contain triangles");
            ensure!(p.indices.iter().all(|&i| (i as usize) < m.positions.len()),
                "mesh {mi} primitive {pi} index out of bounds");
            ensure!(p.material_index < scene.materials.len(),
                "mesh {mi} primitive {pi} material out of bounds");
            if let Some(c) = &p.colors {
                account_binary(&mut binary_bytes, c.len(), 16)?;
                ensure!(c.len() == m.positions.len() && c.iter().flatten().copied().all(normalized),
                    "mesh {mi} primitive {pi} invalid RGBA colors");
            }
        }
    }
    for (mi, m) in scene.materials.iter().enumerate() {
        ensure!(m.base_color.into_iter().all(normalized), "material {mi} invalid RGBA factor");
        ensure!(m.texture_index.is_none_or(|i| i < scene.textures.len()),
            "material {mi} texture out of bounds");
        if let StaticAlphaMode::Mask { cutoff } = m.alpha_mode {
            ensure!(normalized(cutoff), "material {mi} invalid alpha cutoff");
        }
    }
    for (ti, t) in scene.textures.iter().enumerate() {
        account_binary(&mut binary_bytes, t.png_bytes.len(), 1)?;
        // Limit both the compressed payload and decoder allocation before full decode.
        ensure!(t.png_bytes.len() <= 64 * 1024 * 1024, "texture {ti} PNG exceeds byte limit");
        let mut reader = image::ImageReader::with_format(Cursor::new(&t.png_bytes), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        reader.decode().with_context(|| format!("texture {ti} invalid or excessive PNG"))?;
    }
    for (ii, i) in scene.instances.iter().enumerate() {
        let mesh = scene.meshes.get(i.mesh_index)
            .with_context(|| format!("instance {ii} mesh out of bounds"))?;
        validate_matrix(i.matrix).with_context(|| format!("instance {ii} unsupported transform"))?;
        let source = Mat4::from_cols_array_2d(&i.matrix);
        ensure!(mesh.positions.iter().all(|p| source.transform_point3(Vec3::from_array(*p)).is_finite()),
            "instance {ii} transformed bounds are nonfinite");
        let converted = Mat4::from_cols_array_2d(&convert_matrix(i.matrix));
        ensure!(converted.is_finite(), "instance {ii} converted matrix is nonfinite");
        // Signed permutation changes floating-point addition order, so source-space
        // finiteness alone does not establish that the serialized instance is finite.
        ensure!(mesh.positions.iter().all(|p| {
            converted.transform_point3(Vec3::from_array(map_axes(*p))).is_finite()
        }), "instance {ii} converted bounds are nonfinite");
    }
    Ok(())
}

fn validate_matrix(matrix: [[f32; 4]; 4]) -> Result<()> {
    ensure!(matrix.iter().flatten().all(|x| x.is_finite()), "nonfinite matrix");
    // Affine inputs use an exact structural row. Tolerance belongs to rotation and scale.
    ensure!(matrix[0][3] == 0. && matrix[1][3] == 0. && matrix[2][3] == 0. && matrix[3][3] == 1.,
        "projective matrix");
    // Use f64 for validation so large or small valid f32 scale does not overflow/underflow.
    let m = DMat4::from_cols_array(
        &Mat4::from_cols_array_2d(&matrix).to_cols_array().map(f64::from),
    );
    let a = m.x_axis.truncate();
    let b = m.y_axis.truncate();
    let c = m.z_axis.truncate();
    let lengths = [a.length(), b.length(), c.length()];
    let scale = lengths[0];
    ensure!(scale > 0. && lengths.iter().all(|l| (l / scale - 1.).abs() <= 1e-5),
        "nonuniform or zero scale");
    let axes: [DVec3; 3] = [a / lengths[0], b / lengths[1], c / lengths[2]];
    ensure!(axes[0].dot(axes[1]).abs() <= 1e-5
        && axes[0].dot(axes[2]).abs() <= 1e-5
        && axes[1].dot(axes[2]).abs() <= 1e-5, "sheared matrix");
    ensure!(axes[0].cross(axes[1]).dot(axes[2]) > 0., "reflected matrix");
    Ok(())
}

// Bound aggregate binary allocation before preparing any copies. Each view is
// four-byte aligned; reserve the fixed GLB header/chunks here and account for JSON
// in the serializer before creating its output file.
fn account_binary(total: &mut u64, count: usize, stride: u64) -> Result<()> {
    let bytes = (count as u64).checked_mul(stride).context("static binary size overflow")?;
    let padded = bytes.checked_add(3).context("static binary size overflow")? & !3;
    *total = total.checked_add(padded).context("static binary size overflow")?;
    ensure!(*total <= u32::MAX as u64 - 28, "static binary exceeds GLB size limit");
    Ok(())
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn binary_budget_bounds_multiplication_padding_and_aggregate() {
        assert!(account_binary(&mut 0, usize::MAX, 32).is_err());
        let mut total = u32::MAX as u64 - 35;
        account_binary(&mut total, 3, 1).unwrap();
        assert!(account_binary(&mut total, 1, 1).is_err());
        assert!(account_binary(&mut 0, u32::MAX as usize, 4).is_err());
    }
}
