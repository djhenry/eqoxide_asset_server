#[path = "../examples/editable_materials.rs"]
mod command;
#[allow(dead_code)]
mod support {
    include!("support/material_edit.rs");
}
use std::{ffi::OsString, path::Path};

fn inputs() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let reference = support::reference();
    let bundle = eqoxide_asset_server::editable_visual::export_editable_visual(
        &reference,
        &Default::default(),
    )
    .unwrap();
    let directory = temp.path().join("bundle");
    std::fs::create_dir(&directory).unwrap();
    for (name, bytes) in bundle.files {
        let p = directory.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    let (edited, _) = support::editor(&reference, [200, 17, 41, 91]);
    let edited_path = temp.path().join("edited.glb");
    std::fs::write(&edited_path, edited).unwrap();
    (temp, directory, edited_path)
}
fn args(bundle: &Path, edited: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "--material".into(),
        "material-000000".into(),
        bundle.into(),
        edited.into(),
        output.into(),
    ]
}

#[test]
fn cli_requires_selection_and_applies_real_selected_material() {
    let (temp, bundle, edited) = inputs();
    let output = temp.path().join("out.glb");
    assert!(command::run(
        vec![
            bundle.clone().into_os_string(),
            edited.clone().into_os_string(),
            output.clone().into_os_string()
        ],
        Vec::new()
    )
    .is_err());
    assert!(!output.exists());
    let mut report = Vec::new();
    command::run(args(&bundle, &edited, &output), &mut report).unwrap();
    let decoded =
        eqoxide_assets::decode_static_visual(&std::fs::read(&output).unwrap(), &Default::default())
            .unwrap();
    assert_eq!(decoded.materials.len(), 2);
    let report: serde_json::Value = serde_json::from_slice(&report).unwrap();
    assert_eq!(report["selectedMaterials"][0]["id"], "material-000000");
}

#[test]
fn cli_refuses_input_aliases_and_bundle_outputs_without_changing_files() {
    let (temp, bundle, edited) = inputs();
    for output in [
        bundle.join("reference.glb"),
        bundle.join("authoring.json"),
        bundle.join("scene.gltf"),
        bundle.join("new-output.glb"),
        edited.clone(),
    ] {
        let before = std::fs::read(&output).ok();
        assert!(
            command::run(args(&bundle, &edited, &output), Vec::new()).is_err(),
            "{}",
            output.display()
        );
        assert_eq!(std::fs::read(output).ok(), before);
    }
    #[cfg(unix)]
    {
        let reference = bundle.join("reference.glb");
        let original = std::fs::read(&reference).unwrap();
        for hard in [false, true] {
            let alias = temp
                .path()
                .join(if hard { "hard.glb" } else { "symbolic.glb" });
            if hard {
                std::fs::hard_link(&reference, &alias).unwrap()
            } else {
                std::os::unix::fs::symlink(&reference, &alias).unwrap()
            }
            assert!(command::run(args(&bundle, &edited, &alias), Vec::new()).is_err());
            assert_eq!(std::fs::read(&reference).unwrap(), original);
        }
    }
}

#[test]
fn cli_refusal_preserves_existing_output_and_stdout_error_follows_commit() {
    let (temp, bundle, edited) = inputs();
    let output = temp.path().join("out.glb");
    std::fs::write(&output, b"old artifact").unwrap();
    let before = std::fs::read(&edited).unwrap();
    std::fs::write(&edited, b"malformed").unwrap();
    assert!(command::run(args(&bundle, &edited, &output), Vec::new()).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), b"old artifact");
    std::fs::write(&edited, before).unwrap();
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let error = command::run(args(&bundle, &edited, &output), Broken).unwrap_err();
    assert!(format!("{error:#}").contains("committed"));
    eqoxide_assets::decode_static_visual(&std::fs::read(output).unwrap(), &Default::default())
        .unwrap();
}
