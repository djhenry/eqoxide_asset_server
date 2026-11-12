//! Generate synthetic inspection candidates without source-specific asset data.
use anyhow::{ensure, Context, Result};
use eqoxide_asset_server::static_collision::*;
use glam::{Mat4, Quat, Vec3};
use std::path::PathBuf;
fn main() -> Result<()> {
    run(std::env::args_os().skip(1))
}
fn run(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let output = PathBuf::from(args.next().context(
        "usage: static_collision_fixture OUTPUT_DIRECTORY [--declare-preserved-source-query-frames]",
    )?);
    ensure!(
        !output.as_os_str().to_string_lossy().starts_with("--"),
        "output directory must precede the optional flag"
    );
    let query_unit_frames = match args.next() {
        None => QueryUnitFrames::Unspecified,
        Some(flag) if flag == "--declare-preserved-source-query-frames" => {
            QueryUnitFrames::DeclaredSourceQueryFramesPreserved
        }
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

#[cfg(test)]
mod cli_tests {
    use super::*;
    #[test]
    fn strict_arguments_refuse_before_writing() {
        let root = tempfile::tempdir().unwrap();
        assert!(run(Vec::<std::ffi::OsString>::new().into_iter()).is_err());
        let output = root.path().join("must-not-exist");
        for tail in [
            vec!["--canonical-query-units"], // Retired explicit flag is not an alias.
            vec!["--unknown"],
            vec![
                "--declare-preserved-source-query-frames",
                "--declare-preserved-source-query-frames",
            ],
            vec!["--declare-preserved-source-query-frames", "trailing"],
            vec!["trailing"],
        ] {
            let args = std::iter::once(output.as_os_str().to_owned())
                .chain(tail.into_iter().map(Into::into));
            assert!(run(args).is_err());
            assert!(!output.exists());
        }
        for flag in [
            "--canonical-query-units",
            "--declare-preserved-source-query-frames",
        ] {
            assert!(run([flag.into(), output.as_os_str().to_owned()].into_iter()).is_err());
            assert!(!output.exists());
        }
    }
    #[test]
    fn default_and_declared_requirements_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        for declare in [false, true] {
            let output = root
                .path()
                .join(if declare { "preserved" } else { "unspecified" });
            let mut args = vec![output.as_os_str().to_owned()];
            if declare {
                args.push("--declare-preserved-source-query-frames".into());
            }
            run(args.into_iter()).unwrap();
            for name in FILES {
                let bytes = std::fs::read(output.join(name)).unwrap();
                let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
                let j: serde_json::Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
                let expected = if declare {
                    serde_json::json!([
                        "eqoxide-source-query-frames-preserved-v1",
                        "static-collision-triangles-v1"
                    ])
                } else {
                    serde_json::json!(["static-collision-triangles-v1"])
                };
                assert_eq!(
                    j["extras"]["eqoxideAsset"]["requirements"]["capabilities"],
                    expected
                );
            }
        }
    }
    const FILES: &[&str] = &["collision.glb", "empty.glb"];
}
