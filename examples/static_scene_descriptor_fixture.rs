//! Generate and bind authored static metadata; this does not activate or decode a package.
use anyhow::{ensure, Context, Result};
use eqoxide_asset_server::{
    cas::Cas,
    manifest::{Manifest, ManifestStore},
    static_collision::{
        write_static_collision, CollisionWriteLimits, InspectionQueryPolicy,
        StaticCollisionInstance, StaticCollisionMesh, StaticCollisionPrimitive,
        StaticCollisionScene,
    },
    static_scene::{
        write_static_visual, StaticAlphaMode, StaticInstance, StaticMaterial, StaticMesh,
        StaticPrimitive, StaticScene,
    },
    static_scene_descriptor::*,
};
use glam::{Mat4, Quat, Vec3};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let maximum = u64::try_from(maximum)?;
    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= maximum,
        "fixture input exceeds byte limit"
    );
    let mut bytes = Vec::new();
    file.take(
        maximum
            .checked_add(1)
            .context("fixture byte allowance overflow")?,
    )
    .read_to_end(&mut bytes)?;
    ensure!(
        u64::try_from(bytes.len())? <= maximum,
        "fixture input exceeds byte limit"
    );
    Ok(bytes)
}
fn rehash(manifest: &mut Manifest) -> Result<()> {
    manifest.digest = ManifestStore::set_digest(&manifest.files);
    manifest.revision = manifest.canonical_revision()?;
    Ok(())
}
fn main() -> Result<()> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("usage: static_scene_descriptor_fixture OUTPUT_DIRECTORY")?,
    );
    std::fs::create_dir_all(&output)?;
    let limits = DescriptorLimits::default();
    let mut identity = blake3::Hasher::new();
    identity.update(b"static-scene-descriptor-authored-policy-v1\0");
    identity.update(include_bytes!("static_scene_descriptor_fixture.rs"));
    let bake_revision = identity.finalize().to_hex().to_string();
    let positions = vec![[0., 0., 0.], [2., 0., 0.], [0., 3., 0.]];
    let matrices = [
        Mat4::IDENTITY.to_cols_array_2d(),
        Mat4::from_scale_rotation_translation(
            Vec3::splat(2.),
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            Vec3::new(10., 20., 5.),
        )
        .to_cols_array_2d(),
    ];
    let visual = StaticScene {
        meshes: vec![StaticMesh {
            name: "authored-floor".into(),
            positions: positions.clone(),
            normals: vec![[0., 0., 1.]; 3],
            uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
            primitives: vec![StaticPrimitive {
                indices: vec![0, 1, 2],
                material_index: 0,
                colors: Some(vec![
                    [1., 0.4, 0.2, 1.],
                    [0.3, 0.8, 0.2, 0.7],
                    [0.2, 0.4, 1., 0.5],
                ]),
            }],
        }],
        materials: vec![StaticMaterial {
            name: "authored-color".into(),
            base_color: [0.8, 0.7, 0.6, 0.9],
            texture_index: None,
            alpha_mode: StaticAlphaMode::Mask { cutoff: 0.37 },
            double_sided: false,
        }],
        textures: vec![],
        instances: matrices
            .iter()
            .map(|matrix| StaticInstance {
                mesh_index: 0,
                matrix: *matrix,
            })
            .collect(),
    };
    let collision = StaticCollisionScene::Available {
        meshes: vec![StaticCollisionMesh {
            positions,
            primitives: vec![StaticCollisionPrimitive {
                indices: vec![0, 1, 2],
            }],
        }],
        instances: matrices
            .iter()
            .enumerate()
            .map(|(index, matrix)| StaticCollisionInstance {
                owner_id: format!("authored-owner-{index}"),
                mesh_index: 0,
                matrix: *matrix,
            })
            .collect(),
    };
    let visual_requirements =
        write_static_visual(&visual, &bake_revision, &output.join("visual.glb"))?;
    let collision_requirements = write_static_collision(
        &collision,
        &bake_revision,
        InspectionQueryPolicy::FrontFaces,
        &output.join("collision.glb"),
        &CollisionWriteLimits::default(),
    )?;
    let visual_bytes = read_bounded(&output.join("visual.glb"), limits.max_component_bytes)?;
    let collision_bytes = read_bounded(&output.join("collision.glb"), limits.max_component_bytes)?;
    let descriptor = SceneDescriptor::new(
        "authored",
        &bake_revision,
        VisualComponent::Available {
            scope: "declared-static-visual-scene-v1".into(),
            file: SceneFile::from_bytes("visual.glb", &visual_bytes, &limits)?,
            requirements: visual_requirements,
        },
        CollisionComponent::Available {
            scope: "declared-static-triangle-candidates-v1".into(),
            file: SceneFile::from_bytes("collision.glb", &collision_bytes, &limits)?,
            requirements: collision_requirements,
            query_policy: InspectionQueryPolicy::FrontFaces,
        },
    )?;
    let requirements = write_scene_descriptor(&descriptor, &output.join("scene.json"), &limits)?;
    let bytes = read_bounded(&output.join("scene.json"), limits.max_json_bytes)?;
    let store = ManifestStore::new(&output);
    let manifest = store.build_and_write(
        &Cas::new(&output),
        &descriptor.set,
        &[
            ("scene.json".into(), bytes.clone()),
            ("visual.glb".into(), visual_bytes),
            ("collision.glb".into(), collision_bytes),
        ],
        requirements,
    )?;
    let binding = bind_scene_descriptor_to_manifest(&bytes, &manifest, "authored", &limits)?;
    let mut refusals = serde_json::Map::new();
    for kind in [
        "descriptor-hash",
        "descriptor-size",
        "component-hash",
        "component-size",
        "requirements",
        "set",
    ] {
        let mut altered = manifest.clone();
        match kind {
            "descriptor-hash" => altered.files[0].blake3 = "b".repeat(64),
            "descriptor-size" => altered.files[0].size += 1,
            "component-hash" => altered.files[1].blake3 = "b".repeat(64),
            "component-size" => altered.files[1].size += 1,
            "requirements" => altered
                .requirements
                .capabilities
                .push("legacy-assets-v1".into()),
            _ => altered.set = "zone/other".into(),
        }
        rehash(&mut altered)?;
        let refused =
            bind_scene_descriptor_to_manifest(&bytes, &altered, "authored", &limits).is_err();
        ensure!(refused, "metadata corruption accepted: {kind}");
        refusals.insert(kind.into(), refused.into());
    }
    let restored = bind_scene_descriptor_to_manifest(&bytes, &manifest, "authored", &limits)?;
    ensure!(
        restored.manifest_revision() == binding.manifest_revision(),
        "restore mismatch"
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "set": descriptor.set, "bakeRevision": bake_revision,
            "manifestRevision": binding.manifest_revision(), "descriptorBlake3": binding.descriptor_blake3(),
            "files":manifest.files, "requirements":manifest.requirements,
            "availability":{"visual":"available","collision":"available"},
            "descriptorMetadataBound":true, "metadataCorruptionsRefused":refusals,"restoredBinding":true,
            "componentDecodingPerformed":false,"casChunkContentVerificationPerformed":false,
            "segmentQueriesImplemented":false,"runtimeActivationPerformed":false
        }))?
    );
    Ok(())
}
