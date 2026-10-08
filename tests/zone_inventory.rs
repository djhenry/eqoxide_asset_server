use libeq_pfs::PfsWriter;
use serde_json::Value;
use std::io::Cursor;
use std::path::Path;
use std::process::{Command, Output};

fn archive(root: &Path, name: &str, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(root.join(name)).unwrap();
    let mut writer = PfsWriter::create(file).unwrap();
    for (name, bytes) in entries {
        writer.insert(*name, Cursor::new(bytes)).unwrap();
    }
    writer.finish().unwrap();
}

fn inventory(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_eqoxide-assets"))
        .args(["inventory-zones", "--raw"])
        .arg(root)
        .arg("--json")
        .output()
        .unwrap()
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "invalid inventory JSON: {e}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn discovers_loose_and_embedded_descriptors_without_counting_models_as_zones() {
    let raw = tempfile::tempdir().unwrap();
    archive(
        raw.path(),
        "Crescent.EQG",
        &[("ter_crescent.ter", b"EQGT\x03\0\0\0")],
    );
    std::fs::write(raw.path().join("crescent.ZON"), b"EQGZ\x02\0\0\0").unwrap();
    archive(
        raw.path(),
        "arcstone.eqg",
        &[
            ("FarStone.ZON", b"EQTZP\n*NAME farstone"),
            ("farstone.dat", b"data"),
        ],
    );
    archive(raw.path(), "boat.eqg", &[("boat.mod", b"EQGM\x03\0\0\0")]);
    std::fs::write(raw.path().join("CRESCENT.s3d"), b"legacy").unwrap();
    let output = inventory(raw.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = report(&output);
    assert_eq!(data["archives_scanned"], 3);
    assert_eq!(data["zone_archives"], 2);
    assert_eq!(data["zone_archives_without_s3d"], 1);
    assert_eq!(data["non_zone_archives"], serde_json::json!(["boat.eqg"]));
    let candidates = data["candidates"].as_array().unwrap();
    assert_eq!(candidates[0]["archive"], "arcstone.eqg");
    assert_eq!(candidates[0]["descriptors"][0]["name"], "FarStone.ZON");
    assert_eq!(candidates[0]["descriptors"][0]["format"], "eqtzp");
    assert_eq!(candidates[1]["descriptors"][0]["location"], "loose");
    assert_eq!(candidates[1]["descriptors"][0]["version"], 2);
    assert_eq!(
        candidates[1]["s3d_twins"],
        serde_json::json!(["CRESCENT.s3d"])
    );
    assert_eq!(data["errors"], serde_json::json!([]));
    assert_eq!(
        output.stdout,
        inventory(raw.path()).stdout,
        "inventory must be deterministic"
    );
}

#[test]
fn retains_competing_descriptors_without_inventing_selection_precedence() {
    let raw = tempfile::tempdir().unwrap();
    archive(
        raw.path(),
        "oldcommons.eqg",
        &[
            ("commonlands.zon", b"EQTZP"),
            ("oldcommons.zon", b"EQTZP"),
            ("commonlands.dat", b"data"),
        ],
    );
    std::fs::write(raw.path().join("oldcommons.zon"), b"EQTZP").unwrap();
    let output = inventory(raw.path());
    assert!(output.status.success());
    let data = report(&output);
    assert_eq!(data["zone_archives"], 1);
    assert_eq!(
        data["candidates"][0]["descriptors"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(data["candidates"][0]["selection"], "not_attempted");
}

#[test]
fn unrecognized_and_truncated_descriptors_and_corrupt_archives_fail_visibly() {
    let raw = tempfile::tempdir().unwrap();
    archive(raw.path(), "unknown.eqg", &[("unknown.zon", b"mystery")]);
    archive(raw.path(), "short.eqg", &[("short.zon", b"EQGZ\x02")]);
    std::fs::write(raw.path().join("broken.eqg"), b"not a PFS archive").unwrap();
    let output = inventory(raw.path());
    assert!(!output.status.success());
    let data = report(&output);
    assert_eq!(data["archives_scanned"], 3);
    assert_eq!(data["errors"].as_array().unwrap().len(), 3);
    assert_eq!(data["zone_archives"], 0);
    assert!(String::from_utf8_lossy(&output.stderr).contains("inventory incomplete"));
}

#[test]
fn terrain_without_descriptor_and_orphan_descriptor_remain_unresolved() {
    let raw = tempfile::tempdir().unwrap();
    archive(
        raw.path(),
        "portal.eqg",
        &[("ter_plane.ter", b"EQGT\x03\0\0\0")],
    );
    std::fs::write(raw.path().join("orphan.zon"), b"EQGZ\x02\0\0\0").unwrap();
    let output = inventory(raw.path());
    assert!(output.status.success());
    let data = report(&output);
    assert_eq!(data["zone_archives"], 0);
    assert_eq!(data["candidates"][0]["descriptors"], serde_json::json!([]));
    assert_eq!(
        data["orphan_descriptors"],
        serde_json::json!(["orphan.zon"])
    );
}

#[test]
fn case_collisions_are_reported_without_selecting_a_provider() {
    let raw = tempfile::tempdir().unwrap();
    archive(raw.path(), "zone.eqg", &[("zone.zon", b"EQGZ\x02\0\0\0")]);
    archive(raw.path(), "ZONE.eqg", &[("zone.zon", b"EQGZ\x01\0\0\0")]);
    let output = inventory(raw.path());
    assert!(!output.status.success());
    let data = report(&output);
    assert!(data["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["message"].as_str().unwrap().contains("case collision")));
    assert_eq!(data["zone_archives"], 0);
}

#[test]
fn zone_bake_warns_about_unsupported_eqg_without_publishing_it() {
    let raw = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    archive(
        raw.path(),
        "crescent.eqg",
        &[("crescent.zon", b"EQGZ\x02\0\0\0")],
    );
    let output = Command::new(env!("CARGO_BIN_EXE_eqoxide-assets"))
        .args(["build", "--zones-only", "--raw"])
        .arg(raw.path())
        .arg("--out")
        .arg(out.path())
        .output()
        .unwrap();
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(log.contains("1 EQG zone archive(s) unsupported"), "{log}");
    assert!(!out.path().join("manifests/zone/crescent/latest").exists());
}

#[cfg(unix)]
#[test]
fn unrelated_non_utf8_filename_does_not_break_inventory_or_zone_bake() {
    use std::os::unix::ffi::OsStrExt;
    let raw = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    std::fs::write(
        raw.path()
            .join(std::ffi::OsStr::from_bytes(b"unrelated\xff.txt")),
        b"unrelated",
    )
    .unwrap();
    let output = inventory(raw.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(report(&output)["errors"], serde_json::json!([]));
    let output = Command::new(env!("CARGO_BIN_EXE_eqoxide-assets"))
        .args(["build", "--zones-only", "--raw"])
        .arg(raw.path())
        .arg("--out")
        .arg(out.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
