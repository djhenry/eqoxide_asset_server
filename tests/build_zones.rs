use std::path::PathBuf;
#[test]
#[ignore]
fn builds_qcat_zone_set() {
    let home = std::env::var("HOME").unwrap();
    let raw = PathBuf::from(format!("{home}/eq_assets/EQ_Files"));
    if !raw.join("qcat.s3d").exists() { eprintln!("skip"); return; }
    let out = tempfile::tempdir().unwrap();
    let cas = eqoxide_asset_server::cas::Cas::new(out.path());
    let store = eqoxide_asset_server::manifest::ManifestStore::new(out.path());
    let work = out.path().join("work");
    let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
    let zones = eqoxide_asset_server::build::build_zones_from_raw(&cas, &store, &raw, &work, &pool).unwrap();
    assert!(zones.iter().any(|z| z == "qcat"));
    let m = store.load_latest("zone/qcat").unwrap();
    assert!(!m.files.is_empty());
}
