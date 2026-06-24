//! Requires ~/eq_assets/EQ_Files/qcat.s3d (+ qcat_obj.s3d). Run with --ignored.
use std::path::PathBuf;

#[test]
#[ignore]
fn bakes_qcat_with_shared_models_and_placement_nodes() {
    let home = std::env::var("HOME").unwrap();
    let main = PathBuf::from(format!("{home}/eq_assets/EQ_Files/qcat.s3d"));
    let obj = PathBuf::from(format!("{home}/eq_assets/EQ_Files/qcat_obj.s3d"));
    if !main.exists() { eprintln!("skip"); return; }
    let out = tempfile::tempdir().unwrap();
    let glb = out.path().join("qcat.glb");
    eqoxide_asset_server::zone::bake_zone(&main, obj.exists().then_some(obj.as_path()), &glb).unwrap();
    let g = gltf::Gltf::open(&glb).unwrap();
    let meshes = g.document.meshes().count();
    let nodes_with_mesh = g.document.nodes().filter(|n| n.mesh().is_some()).count();
    // Instancing invariant: far more placement nodes than distinct object meshes.
    assert!(nodes_with_mesh > meshes, "expected placement nodes ({nodes_with_mesh}) > meshes ({meshes})");
    assert!(g.document.images().any(|i| i.name().is_some()), "named textures preserved");
}
