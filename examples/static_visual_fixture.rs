//! Build and inspect a source-independent static visual fixture without native assets.
use anyhow::{Context, Result};
use eqoxide_asset_server::static_scene::{
    StaticAlphaMode, StaticInstance, StaticMaterial, StaticMesh, StaticPrimitive,
    StaticScene, StaticTexture, write_static_visual,
};
use glam::{Mat4, Vec3};

fn main() -> Result<()> {
    let output = std::env::args_os().nth(1).context("usage: static_visual_fixture OUTPUT.glb")?;
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([220, 180, 120, 192]))
        .write_to(&mut png, image::ImageFormat::Png)?;
    let scene = StaticScene {
        meshes: vec![StaticMesh {
            name: "enhanced-static-mesh".into(),
            positions: vec![[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0, 2.0, 1.0]],
            normals: vec![[0.0, -1.0 / 5.0_f32.sqrt(), 2.0 / 5.0_f32.sqrt()]; 3],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            primitives: vec![
                StaticPrimitive {
                    indices: vec![0, 1, 2], material_index: 0,
                    colors: Some(vec![[0.2, 0.4, 0.8, 0.35], [0.9, 0.6, 0.1, 0.7], [0.3, 1.0, 0.5, 1.0]]),
                },
                StaticPrimitive { indices: vec![0, 2, 1], material_index: 1, colors: None },
            ],
        }],
        materials: vec![
            StaticMaterial {
                name: "tinted-cutout".into(), base_color: [0.8, 0.6, 0.2, 0.65],
                texture_index: Some(0), alpha_mode: StaticAlphaMode::Mask { cutoff: 0.37 },
                double_sided: false,
            },
            StaticMaterial {
                name: "plain-back".into(), base_color: [0.4, 0.5, 0.9, 1.0],
                texture_index: None, alpha_mode: StaticAlphaMode::Opaque, double_sided: false,
            },
        ],
        textures: vec![StaticTexture { name: "authored-color".into(), png_bytes: png.into_inner() }],
        instances: vec![
            StaticInstance {
                mesh_index: 0,
                matrix: (Mat4::from_translation(Vec3::new(13.0, -7.0, 5.0))
                    * Mat4::from_rotation_z(0.7) * Mat4::from_scale(Vec3::splat(1.5))).to_cols_array_2d(),
            },
            StaticInstance { mesh_index: 0, matrix: Mat4::from_translation(Vec3::new(-3.0, 11.0, 2.0)).to_cols_array_2d() },
        ],
    };
    let mut identity = blake3::Hasher::new();
    identity.update(b"static-visual-fixture-policy-v1\0");
    identity.update(include_bytes!("static_visual_fixture.rs"));
    let revision = identity.finalize().to_hex().to_string();
    let requirements = write_static_visual(&scene, &revision, std::path::Path::new(&output))?;
    let (document, buffers, _) = gltf::import(&output)?;
    let header: serde_json::Value = serde_json::from_str(document.as_json().extras.as_ref().context("missing artifact header")?.get())?;
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for node in document.nodes() {
        let transform = Mat4::from_cols_array_2d(&node.transform().matrix());
        for primitive in node.mesh().context("missing mesh")?.primitives() {
            let reader = primitive.reader(|b| Some(&buffers[b.index()]));
            for p in reader.read_positions().context("missing positions")? {
                let p = transform.transform_point3(Vec3::from_array(p));
                min = min.min(p); max = max.max(p);
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "artifact": header, "requirements": requirements,
        "meshes": document.meshes().len(), "instances": document.nodes().len(),
        "bounds": {"min": min.to_array(), "max": max.to_array()}
    }))?);
    Ok(())
}
