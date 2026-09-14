use libeq_pfs::PfsWriter;
use std::{io::Cursor, process::Command};

fn words(out: &mut Vec<u8>, values: impl IntoIterator<Item = u32>) {
    for word in values {
        out.extend(word.to_le_bytes());
    }
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
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
    fixture_with_mesh(mesh)
}

fn fixture_with_mesh(mesh: Vec<u8>) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
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

#[test]
fn inspection_preserves_raw_surface_values_and_groups_by_both_triangle_words() {
    let strings = b"mat\0shader\0prop\0\xff\0tex\xfe\0";
    let mut mesh = b"EQGM".to_vec();
    words(&mut mesh, [2, strings.len() as u32, 2, 3, 5, 0]);
    mesh.extend(strings);
    words(&mut mesh, [99, 0, 4, 2]);
    words(&mut mesh, [11, 0, 0x7fc01234, 16, 2, 18]);
    words(&mut mesh, [7, 16, 16, 1]);
    words(&mut mesh, [11, 0xfffffff0, 0xdeadbeef]);
    for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        words(
            &mut mesh,
            p.into_iter()
                .chain([0.0, 0.0, 1.0, 0.0, 0.0])
                .map(f32::to_bits),
        );
    }
    for (material, flags) in [
        (u32::MAX, 0x80000001),
        (1, 0x80000001),
        (1, 2),
        (1, 2),
        (0, 2),
    ] {
        words(&mut mesh, [0, 1, 2, material, flags]);
    }
    words(&mut mesh, [0]);
    let (_dir, archive) = fixture_with_mesh(mesh);
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_eqoxide-assets"))
            .args(["inspect-eqg-zone", "--member", "ZONE.ZON", "--archive"])
            .arg(&archive)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, run().stdout);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mesh = &report["meshes"][0];
    let assessments = mesh["surface_assessments"].as_array().unwrap();
    assert_eq!(assessments.len(), 4);
    assert_eq!(assessments[0]["assessment"]["render_reference"], "material");
    assert_eq!(
        assessments[0]["assessment"]["default_collision_query_candidate"],
        true
    );
    assert_eq!(
        assessments[0]["assessment"]["excluded_by_flag2_query"],
        true
    );
    assert_eq!(assessments[3]["assessment"]["render_reference"], "sentinel");
    assert_eq!(
        assessments[3]["assessment"]["default_collision_query_candidate"],
        false
    );
    assert_eq!(assessments[3]["assessment"]["upper_flags"], 0x8000);
    let materials = &mesh["material_details"];
    assert_eq!(materials[0]["ordinal"], 0);
    assert_eq!(materials[0]["index"], 99);
    assert_eq!(materials[1]["ordinal"], 1);
    assert_eq!(materials[1]["index"], 7);
    assert_eq!(
        materials[0]["name"],
        serde_json::json!({"bytes": [109,97,116], "utf8": "mat"})
    );
    assert_eq!(materials[0]["shader"]["utf8"], "shader");
    assert_eq!(
        materials[1]["name"],
        serde_json::json!({"bytes": [255], "utf8": null})
    );
    assert_eq!(materials[1]["shader"], materials[1]["name"]);
    let properties = &materials[0]["properties"];
    assert_eq!(properties[0]["name"]["utf8"], "prop");
    assert_eq!(properties[0]["kind"], 0);
    assert_eq!(properties[0]["value_bits"], 0x7fc01234u32);
    assert!(properties[0]["string_value"].is_null());
    assert_eq!(properties[1]["name"]["bytes"], serde_json::json!([255]));
    assert_eq!(properties[1]["kind"], 2);
    assert_eq!(properties[1]["value_bits"], 18);
    assert_eq!(
        properties[1]["string_value"],
        serde_json::json!({"bytes": [116,101,120,254], "utf8": null})
    );
    assert_eq!(materials[1]["properties"][0]["kind"], 0xfffffff0u32);
    assert_eq!(materials[1]["properties"][0]["value_bits"], 0xdeadbeefu32);
    assert_eq!(
        mesh["triangle_surface_groups"],
        serde_json::json!([
            {"material_index": 0, "flags": 2, "triangles": 1},
            {"material_index": 1, "flags": 2, "triangles": 2},
            {"material_index": 1, "flags": 0x80000001u32, "triangles": 1},
            {"material_index": u32::MAX, "flags": 0x80000001u32, "triangles": 1}
        ])
    );
}
