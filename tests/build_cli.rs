use eqoxide_asset_server::build::ingest_dir;
use eqoxide_asset_server::cas::Cas;
use eqoxide_asset_server::manifest::ManifestStore;

#[test]
fn jobs_flag_rejects_zero() {
    let exe = env!("CARGO_BIN_EXE_eqoxide-assets");
    let out = std::process::Command::new(exe)
        .args(["build", "--out", "/tmp/unused-eqoxide", "--jobs", "0", "--zones-only"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected --jobs 0 to be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("jobs") || stderr.contains("0"), "stderr was: {stderr}");
}

#[test]
fn ingest_dir_chunks_all_files_with_relative_paths() {
    let src = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(src.path().join("textures")).unwrap();
    std::fs::write(src.path().join("humanoid.glb"), vec![1u8; 80_000]).unwrap();
    std::fs::write(src.path().join("textures/skin.png"), vec![2u8; 4_000]).unwrap();

    let data = tempfile::tempdir().unwrap();
    let cas = Cas::new(data.path());
    let store = ManifestStore::new(data.path());

    let m = ingest_dir(&cas, &store, "common", src.path()).unwrap();
    let mut paths: Vec<_> = m.files.iter().map(|f| f.path.clone()).collect();
    paths.sort();
    assert_eq!(paths, vec!["humanoid.glb", "textures/skin.png"]);

    // re-ingesting identical content reuses chunks and yields the same content digest
    // (the store is content-addressed; there is no version counter to bump)
    let m2 = ingest_dir(&cas, &store, "common", src.path()).unwrap();
    let a = m.files.iter().find(|f| f.path == "humanoid.glb").unwrap();
    let b = m2.files.iter().find(|f| f.path == "humanoid.glb").unwrap();
    assert_eq!(a.chunks, b.chunks);
    assert_eq!(m.digest, m2.digest);
}
