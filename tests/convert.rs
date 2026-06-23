//! Requires ~/eq_assets/EQ_Files. Run: cargo test --test convert -- --ignored
use std::path::PathBuf;

#[test]
#[ignore]
fn converts_humanoid_archive_to_glb() {
    let raw = PathBuf::from(std::env::var("EQ_ASSETS")
        .unwrap_or_else(|_| format!("{}/eq_assets/EQ_Files", std::env::var("HOME").unwrap())));
    let src = raw.join("globalhum_chr.s3d");
    if !src.exists() { eprintln!("skip: {src:?} missing"); return; }
    let out = tempfile::tempdir().unwrap();
    let glb = out.path().join("humanoid.glb");
    eqoxide_asset_server::convert::s3d_to_glb(&src, &glb, true).unwrap();
    assert!(glb.metadata().unwrap().len() > 0);
}
