use libeq_pfs::PfsWriter;
use std::{io::Cursor, process::Command};

fn words(out: &mut Vec<u8>, values: impl IntoIterator<Item = u32>) {
    for word in values {
        out.extend(word.to_le_bytes());
    }
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let mut mesh = b"EQGM".to_vec();
    words(&mut mesh, [2, 4, 1, 3, 1, 0]);
    mesh.extend(b"m\0s\0");
    words(&mut mesh, [0, 0, 2, 0]);
    for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        words(
            &mut mesh,
            p.into_iter()
                .chain([0.0, 0.0, 1.0, 0.0, 0.0])
                .map(f32::to_bits),
        );
    }
    words(&mut mesh, [0, 1, 2, u32::MAX, 0x10000, 0]);
    let mut zone = b"EQGZ".to_vec();
    let strings = b"Thing.MOD\0instance\0";
    words(&mut zone, [1, strings.len() as u32, 1, 1, 0, 0]);
    zone.extend(strings);
    words(&mut zone, [0, 0, 10]);
    words(
        &mut zone,
        [0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0].map(f32::to_bits),
    );
    let archive = dir.path().join("fixture.eqg");
    let mut writer = PfsWriter::create(std::fs::File::create(&archive).unwrap()).unwrap();
    writer.insert("thing.mod", Cursor::new(mesh)).unwrap();
    writer.insert("ZONE.ZON", Cursor::new(&zone)).unwrap();
    writer.finish().unwrap();
    std::fs::write(dir.path().join("loose.zon"), zone).unwrap();
    (dir, archive)
}

#[test]
fn inspection_resolves_explicit_providers_and_reports_source_scope() {
    let (dir, archive) = fixture();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_eqoxide-assets"))
            .arg("inspect-eqg-zone")
            .arg("--archive")
            .arg(&archive)
            .args(args)
            .output()
            .unwrap()
    };
    let first = run(&["--member", "zone.zon"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let again = run(&["--member", "zone.zon"]);
    assert_eq!(first.stdout, again.stdout);
    let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["stage"], "binary_source_assembly");
    assert_eq!(report["unique_meshes"], 1);
    assert_eq!(report["descriptor"]["name"], "ZONE.ZON");
    assert_eq!(report["descriptor"]["location"], "archive");
    assert_eq!(report["model_dependencies"][0]["requested"], "Thing.MOD");
    assert_eq!(report["model_dependencies"][0]["member"], "thing.mod");
    assert_eq!(report["model_slots"], serde_json::json!([0]));
    assert_eq!(report["placement_records"], 1);
    assert_eq!(report["meshes"][0]["member"], "thing.mod");
    assert_eq!(report["meshes"][0]["triangles_without_table_material"], 1);
    assert_eq!(report["limitations"].as_array().unwrap().len(), 4);
    let loose = run(&[
        "--descriptor",
        dir.path().join("loose.zon").to_str().unwrap(),
    ]);
    assert!(
        loose.status.success(),
        "{}",
        String::from_utf8_lossy(&loose.stderr)
    );
    let loose_report: serde_json::Value = serde_json::from_slice(&loose.stdout).unwrap();
    assert_eq!(report["meshes"], loose_report["meshes"]);
    assert_eq!(report["model_slots"], loose_report["model_slots"]);
    assert_eq!(loose_report["descriptor"]["location"], "loose");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn inspection_requires_one_descriptor_and_rejects_missing_member() {
    let (dir, archive) = fixture();
    for args in [
        vec![],
        vec!["--member", "missing.zon"],
        vec![
            "--member",
            "ZONE.ZON",
            "--descriptor",
            dir.path().join("loose.zon").to_str().unwrap(),
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_eqoxide-assets"))
            .arg("inspect-eqg-zone")
            .arg("--archive")
            .arg(&archive)
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}
