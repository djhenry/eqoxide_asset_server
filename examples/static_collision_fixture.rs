//! Generate synthetic inspection candidates without source-specific asset data.
use anyhow::{ensure, Context, Result};
use eqoxide_asset_server::static_collision::*;
use glam::{Mat4, Quat, Vec3};
use std::path::PathBuf;
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let output = PathBuf::from(args.next().context(
        "usage: static_collision_fixture OUTPUT_DIRECTORY [--canonical-query-units]",
    )?);
    ensure!(
        !output.as_os_str().to_string_lossy().starts_with("--"),
        "output directory must precede the optional flag"
    );
    let query_unit_frames = match args.next() {
        None => QueryUnitFrames::Unspecified,
        Some(flag) if flag == "--canonical-query-units" => QueryUnitFrames::DeclaredCanonical,
        Some(_) => anyhow::bail!("unknown fixture option"),
    };
    ensure!(args.next().is_none(), "unexpected fixture argument");
    std::fs::create_dir_all(&output)?;
    let mut identity = blake3::Hasher::new();
    identity.update(b"static-collision-authored-policy-v1\0");
    identity.update(include_bytes!("static_collision_fixture.rs"));
    identity.update(query_unit_frames.identifier().as_bytes());
    let bake_revision = identity.finalize().to_hex().to_string();
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
    write_static_collision_with_query_units(
        &scene,
        &bake_revision,
        InspectionQueryPolicy::FrontFaces,
        query_unit_frames,
        &output.join("collision.glb"),
        &CollisionWriteLimits::default(),
    )?;
    write_static_collision_with_query_units(
        &StaticCollisionScene::AvailableEmpty,
        &bake_revision,
        InspectionQueryPolicy::BothFaces,
        query_unit_frames,
        &output.join("empty.glb"),
        &CollisionWriteLimits::default(),
    )?;
    Ok(())
}
