//! Native-free curved material swatches for the opt-in scalar PBR profile.
use anyhow::{Context, Result};
use eqoxide_asset_server::static_scene::{
    write_static_visual_with_material_policy, StaticAlphaMode, StaticInstance, StaticMaterial,
    StaticMaterialPolicy, StaticMesh, StaticPrimitive, StaticScene, StaticSurfaceModel,
};
use glam::{Mat4, Quat, Vec3};

/// Five identical domes with distinct materials and two additional shared instances.
/// Output axes: main centers x=-6,-3,0,3,6, y=0, z=0; domes face +Z.
/// The two smaller matte instances sit at x=-1.5,+1.5, y=-3, with Y rotation.
pub fn fixture_scene() -> (StaticScene, StaticMaterialPolicy) {
    let names = [
        "matte-dielectric",
        "smooth-dielectric",
        "rough-metal",
        "smooth-metal",
        "unlit-control",
    ];
    let mut scene = StaticScene::default();
    for (index, name) in names.into_iter().enumerate() {
        let mut mesh = dome();
        mesh.name = name.into();
        mesh.primitives[0].material_index = index;
        scene.meshes.push(mesh);
        scene.materials.push(StaticMaterial {
            name: name.into(),
            base_color: [0.65, 0.38, 0.18, 1.],
            texture_index: None,
            alpha_mode: StaticAlphaMode::Opaque,
            double_sided: false,
        });
        scene.instances.push(StaticInstance {
            mesh_index: index,
            matrix: Mat4::from_translation(Vec3::new((index as f32 - 2.) * 3., 0., 0.))
                .to_cols_array_2d(),
        });
    }
    for (x, angle) in [(-1.5, -0.6), (1.5, 0.6)] {
        scene.instances.push(StaticInstance {
            mesh_index: 0,
            // Source Z is output Y; a source Z rotation becomes output Y rotation.
            matrix: Mat4::from_scale_rotation_translation(
                Vec3::splat(0.65),
                Quat::from_rotation_z(angle),
                Vec3::new(x, 0., -3.),
            )
            .to_cols_array_2d(),
        });
    }
    let mut policy = StaticMaterialPolicy::default();
    policy.overrides.insert(
        1,
        StaticSurfaceModel::Pbr {
            metallic: 0.,
            roughness: 0.08,
        },
    );
    policy.overrides.insert(
        2,
        StaticSurfaceModel::Pbr {
            metallic: 1.,
            roughness: 0.8,
        },
    );
    policy.overrides.insert(
        3,
        StaticSurfaceModel::Pbr {
            metallic: 1.,
            roughness: 0.08,
        },
    );
    policy.overrides.insert(4, StaticSurfaceModel::Unlit);
    (scene, policy)
}

fn dome() -> StaticMesh {
    const RINGS: usize = 12;
    const SEGMENTS: usize = 32;
    let mut positions = vec![[0., -1.15, 0.]];
    let mut normals = vec![[0., -1., 0.]];
    let mut uvs = vec![[0.5, 0.5]];
    for ring in 1..=RINGS {
        let theta = ring as f32 / RINGS as f32 * std::f32::consts::FRAC_PI_2;
        for segment in 0..SEGMENTS {
            let phi = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let output = Vec3::new(
                theta.sin() * phi.cos(),
                theta.sin() * phi.sin(),
                theta.cos(),
            );
            let source = Vec3::new(output.x, -output.z, output.y);
            positions.push((source * 1.15).to_array());
            normals.push(source.to_array());
            uvs.push([0.5 + output.x * 0.5, 0.5 + output.y * 0.5]);
        }
    }
    let vertex =
        |ring: usize, segment: usize| (1 + (ring - 1) * SEGMENTS + segment % SEGMENTS) as u32;
    let mut indices = Vec::new();
    for segment in 0..SEGMENTS {
        indices.extend([0, vertex(1, segment), vertex(1, segment + 1)]);
    }
    for ring in 2..=RINGS {
        for segment in 0..SEGMENTS {
            let [a, b, c, d] = [
                vertex(ring - 1, segment),
                vertex(ring, segment),
                vertex(ring, segment + 1),
                vertex(ring - 1, segment + 1),
            ];
            indices.extend([a, b, c, a, c, d]);
        }
    }
    StaticMesh {
        name: String::new(),
        positions,
        normals,
        uvs,
        primitives: vec![StaticPrimitive {
            indices,
            material_index: 0,
            colors: None,
        }],
    }
}

fn main() -> Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .context("usage: static_pbr_fixture OUTPUT.glb")?;
    let (scene, policy) = fixture_scene();
    let mut identity = blake3::Hasher::new();
    identity.update(b"static-pbr-fixture-policy-v1\0");
    identity.update(include_bytes!("static_pbr_fixture.rs"));
    let revision = identity.finalize().to_hex().to_string();
    let report = write_static_visual_with_material_policy(
        &scene,
        &policy,
        &revision,
        std::path::Path::new(&output),
    )?;
    // The glTF crate does not enable the unlit feature; inspect raw standard JSON.
    let bytes = std::fs::read(&output)?;
    let glb = gltf::binary::Glb::from_slice(&bytes)?;
    let json: serde_json::Value = serde_json::from_slice(&glb.json)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "artifact": json["extras"]["eqoxideAsset"], "policy": report,
            "meshes": scene.meshes.len(), "instances": scene.instances.len(),
            "materials": json["materials"], "extensionsUsed": json["extensionsUsed"],
            "extensionsRequired": json["extensionsRequired"],
        }))?
    );
    Ok(())
}
