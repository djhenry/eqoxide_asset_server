//! Generate synthetic inspection candidates without source-specific asset data.
use anyhow::{Context, Result};
use eqoxide_asset_server::static_collision::*;
use glam::{Mat4, Quat, Vec3};
use std::path::PathBuf;
fn main() -> Result<()> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("usage: static_collision_fixture OUTPUT_DIRECTORY")?,
    );
    std::fs::create_dir_all(&output)?;
    let scene = StaticCollisionScene::Available {
        meshes: vec![StaticCollisionMesh {
            positions: vec![[0., 0., 0.], [2., 0., 0.], [0., 3., 0.], [2., 3., 0.]],
            primitives: vec![StaticCollisionPrimitive {
                indices: vec![0, 1, 2, 1, 3, 2],
            }],
        }],
        instances: vec![
            StaticCollisionInstance {
                owner_id: "authored-floor".into(),
                mesh_index: 0,
                matrix: Mat4::IDENTITY.to_cols_array_2d(),
            },
            StaticCollisionInstance {
                owner_id: "authored-platform".into(),
                mesh_index: 0,
                matrix: Mat4::from_scale_rotation_translation(
                    Vec3::splat(2.),
                    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                    Vec3::new(10., 20., 5.),
                )
                .to_cols_array_2d(),
            },
        ],
    };
    write_static_collision(
        &scene,
        &"a".repeat(64),
        InspectionQueryPolicy::FrontFaces,
        &output.join("collision.glb"),
        &CollisionWriteLimits::default(),
    )?;
    write_static_collision(
        &StaticCollisionScene::AvailableEmpty,
        &"a".repeat(64),
        InspectionQueryPolicy::BothFaces,
        &output.join("empty.glb"),
        &CollisionWriteLimits::default(),
    )?;
    Ok(())
}
