//! Build a synthetic common artifact through the source material and texture seams.
use anyhow::{Context, Result};
use eqoxide_asset_server::{
    source_material::{
        decode_static_texture, normalize_material, AlphaMode, SourceMaterial, TextureDecodePolicy,
    },
    static_scene::{write_static_visual, StaticInstance, StaticMesh, StaticPrimitive, StaticScene},
};
use glam::{Mat4, Vec3};
use std::io::Cursor;

fn main() -> Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .context("usage: source_material_fixture OUTPUT.glb")?;
    let image: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> =
        image::ImageBuffer::from_raw(1, 1, vec![65535, 40001, 17003, 38291]).unwrap();
    let mut input = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba16(image).write_to(&mut input, image::ImageFormat::Png)?;
    let texture = decode_static_texture(
        "source-samples",
        input.get_ref(),
        TextureDecodePolicy::PreserveRgba,
    )?;
    let mut scene = StaticScene {
        textures: vec![texture],
        ..StaticScene::default()
    };
    for (index, opacity) in [250u16, 500, 750].into_iter().enumerate() {
        let source = SourceMaterial {
            name: format!("opacity-{opacity}"),
            base_color: [0.8, 0.5, 0.2, 0.6],
            texture_index: Some(0),
            alpha_mode: AlphaMode::Blend(opacity),
            double_sided: false,
            texture_sequence: None,
        };
        scene.materials.push(normalize_material(&source)?);
        scene.meshes.push(StaticMesh {
            name: format!("card-{opacity}"),
            positions: vec![[-1., 0., 0.], [1., 0., 0.], [0., 0., 2.]],
            normals: vec![[0., -1., 0.]; 3],
            uvs: vec![[0.5, 0.5]; 3],
            primitives: vec![StaticPrimitive {
                indices: vec![0, 1, 2],
                material_index: index,
                colors: None,
            }],
        });
        scene.instances.push(StaticInstance {
            mesh_index: index,
            matrix: Mat4::from_translation(Vec3::new(index as f32 * 3. - 3., 0., 0.))
                .to_cols_array_2d(),
        });
    }
    let mut identity = blake3::Hasher::new();
    identity.update(b"source-material-fixture-policy-v1\0");
    identity.update(include_bytes!("source_material_fixture.rs"));
    let requirements = write_static_visual(
        &scene,
        &identity.finalize().to_hex().to_string(),
        std::path::Path::new(&output),
    )?;
    let (document, _, images) = gltf::import(output)?;
    let factors: Vec<_> = document
        .materials()
        .map(|m| m.pbr_metallic_roughness().base_color_factor())
        .collect();
    anyhow::ensure!(
        images[0].format == gltf::image::Format::R16G16B16A16,
        "source image lost sample depth"
    );
    let samples: Vec<u16> = images[0]
        .pixels
        .chunks_exact(2)
        .map(|bytes| u16::from_ne_bytes(bytes.try_into().unwrap()))
        .collect();
    anyhow::ensure!(
        samples == [65535, 40001, 17003, 38291],
        "source samples changed"
    );
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"requirements":requirements,"material_factors":factors,"source_rgba16":samples,"instances":document.nodes().count()})
        )?
    );
    Ok(())
}
