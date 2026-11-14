//! Authored standard PBR transparency fixture; no native assets or effects.
use anyhow::{ensure, Context, Result};
use eqoxide_asset_server::static_scene::{
    write_static_visual_with_surface_profile, StaticAlphaMode, StaticInstance, StaticMaterial,
    StaticMaterialPolicy, StaticMaterialProfile, StaticMesh, StaticPrimitive, StaticScene,
    StaticSurfaceModel, StaticTexture,
};
use glam::{Mat4, Vec3};
use std::{collections::BTreeMap, io::Cursor, path::Path};

fn quad(name: &str, half: [f32; 2], material_index: usize, alpha: Option<f32>) -> StaticMesh {
    let [x, y] = half;
    StaticMesh {
        name: name.into(),
        positions: vec![[-x, -y, 0.], [x, -y, 0.], [x, y, 0.], [-x, y, 0.]],
        normals: vec![[0., 0., 1.]; 4],
        uvs: vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]],
        primitives: vec![StaticPrimitive {
            indices: vec![0, 1, 2, 0, 2, 3],
            material_index,
            colors: alpha.map(|a| vec![[1., 1., 1., a]; 4]),
        }],
    }
}
fn material(name: &str, base_color: [f32; 4], alpha_mode: StaticAlphaMode) -> StaticMaterial {
    StaticMaterial {
        name: name.into(),
        base_color,
        texture_index: Some(0),
        alpha_mode,
        double_sided: true,
    }
}
/// The same mesh is reused for the main red layer and a smaller rotated swatch.
pub fn fixture_scene() -> Result<(StaticScene, StaticMaterialPolicy)> {
    let mut image = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]))
        .write_to(&mut image, image::ImageFormat::Png)?;
    let scene = StaticScene {
        meshes: vec![
            quad("opaque-backdrop", [3., 2.], 0, None),
            quad("red-layer", [1.4, 1.4], 1, None),
            quad("blue-layer", [1.4, 1.4], 2, None),
            quad("mask-above-cutoff", [0.4, 0.4], 3, Some(0.75)),
            quad("explicit-unlit-layer", [0.4, 0.4], 4, None),
        ],
        materials: vec![
            material(
                "opaque-dielectric",
                [0.7, 0.7, 0.7, 1.],
                StaticAlphaMode::Opaque,
            ),
            material(
                "red-pbr-blend",
                [1., 0.05, 0.02, 0.5],
                StaticAlphaMode::Blend,
            ),
            material(
                "blue-pbr-blend",
                [0.02, 0.1, 1., 0.4],
                StaticAlphaMode::Blend,
            ),
            material(
                "mask-coverage-product",
                [0.05, 1., 0.05, 1.],
                StaticAlphaMode::Mask { cutoff: 0.5 },
            ),
            material("unlit-blend", [1., 0.7, 0.2, 0.35], StaticAlphaMode::Blend),
        ],
        textures: vec![StaticTexture {
            name: "white-rgba".into(),
            png_bytes: image.into_inner(),
        }],
        instances: vec![
            StaticInstance {
                mesh_index: 0,
                matrix: Mat4::IDENTITY.to_cols_array_2d(),
            },
            StaticInstance {
                mesh_index: 1,
                matrix: Mat4::from_translation(Vec3::new(-0.65, 0., 0.5)).to_cols_array_2d(),
            },
            StaticInstance {
                mesh_index: 2,
                matrix: Mat4::from_translation(Vec3::new(0.65, 0.2, 1.)).to_cols_array_2d(),
            },
            StaticInstance {
                mesh_index: 3,
                matrix: Mat4::from_translation(Vec3::new(-2.2, 0., 1.4)).to_cols_array_2d(),
            },
            StaticInstance {
                mesh_index: 4,
                matrix: Mat4::from_translation(Vec3::new(2.2, 0., 1.4)).to_cols_array_2d(),
            },
            StaticInstance {
                mesh_index: 1,
                matrix: (Mat4::from_translation(Vec3::new(0., -1.8, 1.7))
                    * Mat4::from_rotation_z(0.37)
                    * Mat4::from_scale(Vec3::splat(0.3)))
                .to_cols_array_2d(),
            },
        ],
    };
    let policy = StaticMaterialPolicy {
        overrides: BTreeMap::from([
            (
                1,
                StaticSurfaceModel::Pbr {
                    metallic: 0.,
                    roughness: 0.25,
                },
            ),
            (
                2,
                StaticSurfaceModel::Pbr {
                    metallic: 0.,
                    roughness: 0.4,
                },
            ),
            (4, StaticSurfaceModel::Unlit),
        ]),
        ..StaticMaterialPolicy::default()
    };
    Ok((scene, policy))
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let output = args
        .next()
        .context("usage: static_hdr_fixture OUTPUT.glb")?;
    ensure!(
        args.next().is_none(),
        "usage: static_hdr_fixture OUTPUT.glb"
    );
    let (scene, policy) = fixture_scene()?;
    let policy_bytes = serde_json::to_vec(
        &serde_json::json!({"default":policy.default,"overrides":policy.overrides}),
    )?;
    let mut revision = blake3::Hasher::new();
    for part in [
        b"authored-static-hdr-blend-v1".as_slice(),
        include_bytes!("static_hdr_fixture.rs").as_slice(),
        policy_bytes.as_slice(),
    ] {
        revision.update(&(part.len() as u64).to_le_bytes());
        revision.update(part);
    }
    let bake_revision = revision.finalize().to_hex().to_string();
    let report = write_static_visual_with_surface_profile(
        &scene,
        &policy,
        StaticMaterialProfile::ScalarPbrHdrBlendV1,
        &bake_revision,
        Path::new(&output),
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "source":"authored-static-hdr-blend-v1","bake_revision":bake_revision,
            "render_profile":"pbr-hdr-blend-reference-light-v1",
            "meshes":scene.meshes.len(),"instances":scene.instances.len(),
            "materials":scene.materials.len(),"textures":scene.textures.len(),"material_policy":report,
        }))?
    );
    Ok(())
}
