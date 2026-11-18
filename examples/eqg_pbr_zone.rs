//! Explicit offline binary EQG static PBR preview; no production publication.
use anyhow::{ensure, Context, Result};
use eqoxide_asset_server::{
    eqg::{
        pbr_visual::{bounded_json, load_eqg_pbr_preview, EqgPbrLimits},
        DescriptorSource,
    },
    static_scene::{
        write_static_visual_with_surface_profile, ResolvedStaticMaterialPolicy,
        StaticMaterialPolicy, StaticMaterialPolicySource, StaticMaterialProfile, StaticScene,
    },
};
use serde::Serialize;
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
};
const PROFILE: &str = "scalar-pbr-hdr-blend-v1";
struct Arguments {
    inspect: bool,
    archive: PathBuf,
    descriptor_file: Option<PathBuf>,
    descriptor_member: Option<String>,
    output: Option<PathBuf>,
}
fn parse_args(args: Vec<OsString>) -> Result<Arguments> {
    let (mut preview, mut inspect, mut positional, mut file, mut member) =
        (false, false, Vec::new(), None, None);
    let mut it = args.into_iter();
    let mut ended = false;
    while let Some(a) = it.next() {
        if !ended && a == "--" {
            ended = true;
            continue;
        }
        if !ended && a == "--static-preview" {
            ensure!(!preview, "duplicate --static-preview");
            preview = true;
        } else if !ended && a == "--inspect" {
            ensure!(!inspect, "duplicate --inspect");
            inspect = true;
        } else if !ended && (a == "--descriptor-file" || a == "--descriptor-member") {
            ensure!(
                file.is_none() && member.is_none(),
                "exactly one descriptor selector required"
            );
            let value = it.next().context("missing descriptor selector value")?;
            ensure!(
                !value.to_string_lossy().starts_with('-'),
                "missing descriptor selector value"
            );
            if a == "--descriptor-file" {
                file = Some(PathBuf::from(value));
            } else {
                member = Some(
                    value
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("descriptor member must be UTF-8"))?,
                );
            }
        } else {
            ensure!(
                ended || !a.to_string_lossy().starts_with('-'),
                "unknown option"
            );
            positional.push(PathBuf::from(a));
        }
    }
    ensure!(preview, "--static-preview is required");
    ensure!(
        file.is_some() ^ member.is_some(),
        "exactly one descriptor selector required"
    );
    ensure!(
        positional.len() == if inspect { 1 } else { 2 },
        "expected ARCHIVE and, unless --inspect, OUTPUT.glb"
    );
    Ok(Arguments {
        inspect,
        archive: positional.remove(0),
        descriptor_file: file,
        descriptor_member: member,
        output: positional.pop(),
    })
}
fn refuse_input_alias(a: &Arguments) -> Result<()> {
    let Some(output) = &a.output else {
        return Ok(());
    };
    if output.try_exists()? {
        let target = output.canonicalize()?;
        ensure!(
            target != a.archive.canonicalize()?,
            "output aliases the source archive"
        );
        if let Some(descriptor) = &a.descriptor_file {
            ensure!(
                target != descriptor.canonicalize()?,
                "output aliases the source descriptor"
            );
        }
    }
    Ok(())
}
fn choices(
    scene: &StaticScene,
    policy: &StaticMaterialPolicy,
) -> Vec<ResolvedStaticMaterialPolicy> {
    scene
        .materials
        .iter()
        .enumerate()
        .map(|(material_index, _)| {
            let (surface, source) = match policy.overrides.get(&material_index) {
                Some(&s) => (s, StaticMaterialPolicySource::Override),
                None => (policy.default, StaticMaterialPolicySource::Default),
            };
            ResolvedStaticMaterialPolicy {
                material_index,
                surface,
                source,
            }
        })
        .collect()
}
#[derive(Serialize)]
struct Envelope<'a, T: Serialize> {
    conversion: &'a T,
    material_policy: &'a [ResolvedStaticMaterialPolicy],
    writer_profile: &'static str,
    glb_bake_revision: Option<&'a str>,
    stage: &'static str,
    glb_written: bool,
    production_publication: bool,
    gameplay_ready: bool,
}
fn final_revision(conversion: &str, profile: &str, policy: &impl Serialize) -> Result<String> {
    let mut h = blake3::Hasher::new();
    h.update(b"eqg-standard-glb-bake-v1");
    for b in [
        conversion.as_bytes(),
        profile.as_bytes(),
        &serde_json::to_vec(policy)?,
        include_bytes!("eqg_pbr_zone.rs"),
        include_bytes!("../src/static_scene.rs"),
        include_bytes!("../src/convert/mod.rs"),
    ] {
        h.update(&(b.len() as u64).to_le_bytes());
        h.update(b);
    }
    Ok(h.finalize().to_hex().to_string())
}
fn publish<T: Serialize>(
    scene: &StaticScene,
    policy: &StaticMaterialPolicy,
    conversion: &T,
    conversion_revision: &str,
    l: &EqgPbrLimits,
    output: &Path,
) -> Result<Vec<u8>> {
    let actual = choices(scene, policy);
    let revision = final_revision(conversion_revision, PROFILE, &actual)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // into_temp_path closes the initial descriptor. The common writer replaces
    // this pathname, so all inspection/sync must reopen its resulting inode.
    let candidate = tempfile::NamedTempFile::new_in(parent)?.into_temp_path();
    let written = write_static_visual_with_surface_profile(
        scene,
        policy,
        StaticMaterialProfile::ScalarPbrHdrBlendV1,
        &revision,
        &candidate,
    )?;
    let file = std::fs::File::open(&candidate)?;
    ensure!(
        file.metadata()?.len() <= l.max_glb_bytes as u64,
        "encoded GLB byte limit exceeded"
    );
    let bytes = bounded_json(
        &Envelope {
            conversion,
            material_policy: &written.materials,
            writer_profile: PROFILE,
            glb_bake_revision: Some(&revision),
            stage: "offline-static-preview",
            glb_written: true,
            production_publication: false,
            gameplay_ready: false,
        },
        l.max_report_json_bytes,
    )?;
    file.sync_all()?;
    drop(file);
    candidate.persist(output).map_err(|e| e.error)?;
    Ok(bytes)
}
fn main() -> Result<()> {
    let a = parse_args(std::env::args_os().skip(1).collect())?;
    let l = EqgPbrLimits::default();
    refuse_input_alias(&a)?;
    let descriptor = if let Some(p) = &a.descriptor_file {
        DescriptorSource::Loose(p)
    } else {
        DescriptorSource::Archive(a.descriptor_member.as_deref().unwrap())
    };
    let resolved = load_eqg_pbr_preview(&a.archive, descriptor, &l)?;
    let bytes = if a.inspect {
        let actual = choices(&resolved.scene, &resolved.recommended_material_policy);
        bounded_json(
            &Envelope {
                conversion: &resolved.report,
                material_policy: &actual,
                writer_profile: PROFILE,
                glb_bake_revision: None,
                stage: "cpu-source-inspection",
                glb_written: false,
                production_publication: false,
                gameplay_ready: false,
            },
            l.max_report_json_bytes,
        )?
    } else {
        publish(
            &resolved.scene,
            &resolved.recommended_material_policy,
            &resolved.report,
            &resolved.report.conversion_revision,
            &l,
            a.output.as_deref().unwrap(),
        )?
    };
    // An stdout error after persistence cannot roll back the committed artifact.
    let mut out = std::io::stdout().lock();
    out.write_all(&bytes)?;
    out.write_all(b"\n")?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(s: &[&str]) -> Vec<std::ffi::OsString> {
        s.iter().map(Into::into).collect()
    }
    #[test]
    fn strict_forms_accept_only_one_explicit_descriptor_and_mode() {
        for flag in ["--descriptor-file", "--descriptor-member"] {
            assert!(parse_args(args(&[
                "--static-preview",
                flag,
                "zone.zon",
                "zone.eqg",
                "out.glb"
            ]))
            .is_ok());
            assert!(parse_args(args(&[
                "--static-preview",
                "--inspect",
                flag,
                "zone.zon",
                "zone.eqg"
            ]))
            .is_ok());
        }
        for a in [
            vec!["--descriptor-file", "zone.zon", "zone.eqg", "out.glb"],
            vec!["--static-preview", "zone.eqg", "out.glb"],
            vec![
                "--static-preview",
                "--static-preview",
                "--descriptor-file",
                "zone.zon",
                "zone.eqg",
                "out.glb",
            ],
            vec![
                "--static-preview",
                "--inspect",
                "--descriptor-file",
                "zone.zon",
                "zone.eqg",
                "out.glb",
            ],
            vec![
                "--static-preview",
                "--descriptor-file",
                "zone.zon",
                "--descriptor-member",
                "zone.zon",
                "zone.eqg",
                "out.glb",
            ],
            vec![
                "--static-preview",
                "--descriptor-file",
                "zone.zon",
                "zone.eqg",
                "out.glb",
                "extra",
            ],
            vec!["--static-preview", "--unknown"],
            vec!["--static-preview", "--descriptor-file"],
            vec![
                "--static-preview",
                "--descriptor-file",
                "--inspect",
                "zone.eqg",
            ],
        ] {
            assert!(parse_args(args(&a)).is_err(), "{a:?}");
        }
    }
    #[test]
    fn outer_publication_preserves_prior_output_on_glb_and_report_caps() {
        use eqoxide_asset_server::static_scene::{
            StaticAlphaMode, StaticInstance, StaticMaterial, StaticMesh, StaticPrimitive,
        };
        let scene = StaticScene {
            meshes: vec![StaticMesh {
                name: "triangle".into(),
                positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                normals: vec![[0., 0., 1.]; 3],
                uvs: vec![[0., 0.]; 3],
                primitives: vec![StaticPrimitive {
                    indices: vec![0, 1, 2],
                    material_index: 0,
                    colors: None,
                }],
            }],
            materials: vec![StaticMaterial {
                name: "mat".into(),
                base_color: [1.; 4],
                texture_index: None,
                alpha_mode: StaticAlphaMode::Opaque,
                double_sided: false,
            }],
            textures: vec![],
            instances: vec![StaticInstance {
                mesh_index: 0,
                matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
            }],
        };
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("out.glb");
        let policy = StaticMaterialPolicy::default();
        let report = serde_json::json!({"static":true});
        for report_cap in [false, true] {
            std::fs::write(&output, b"prior").unwrap();
            let mut l = EqgPbrLimits::default();
            if report_cap {
                l.max_report_json_bytes = 1;
            } else {
                l.max_glb_bytes = 1;
            }
            assert!(publish(&scene, &policy, &report, &"0".repeat(64), &l, &output).is_err());
            assert_eq!(std::fs::read(&output).unwrap(), b"prior");
            assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
        }
        publish(
            &scene,
            &policy,
            &report,
            &"0".repeat(64),
            &EqgPbrLimits::default(),
            &output,
        )
        .unwrap();
        assert!(std::fs::read(&output).unwrap().starts_with(b"glTF"));
    }
    #[test]
    fn source_output_aliases_refuse_before_reading_or_writing() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("input.eqg");
        let descriptor = t.path().join("zone.zon");
        std::fs::write(&source, b"source archive").unwrap();
        std::fs::write(&descriptor, b"source descriptor").unwrap();
        let alias = t.path().join("alias.glb");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source, &alias).unwrap();
        for target in [&source, &descriptor, &t.path().join("./input.eqg"), &alias] {
            let a = Arguments {
                inspect: false,
                archive: source.clone(),
                descriptor_file: Some(descriptor.clone()),
                descriptor_member: None,
                output: Some(target.clone()),
            };
            assert!(refuse_input_alias(&a).is_err());
            assert_eq!(std::fs::read(&source).unwrap(), b"source archive");
            assert_eq!(std::fs::read(&descriptor).unwrap(), b"source descriptor");
        }
    }
    #[test]
    fn identity_binds_actual_policy_and_hdr_profile() {
        let p = serde_json::json!({"default":"a"});
        let q = serde_json::json!({"default":"b"});
        let a = final_revision("conversion", "hdr", &p).unwrap();
        assert_ne!(a, final_revision("conversion", "other", &p).unwrap());
        assert_ne!(a, final_revision("conversion", "hdr", &q).unwrap());
    }
}
