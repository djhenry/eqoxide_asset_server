//! Requires ~/eq_assets/EQ_Files/qcat.s3d (+ qcat_obj.s3d). Run with --ignored.
use std::path::PathBuf;

#[test]
#[ignore]
fn bakes_qcat_with_named_textures_and_placed_objects() {
    let home = std::env::var("HOME").unwrap();
    let main = PathBuf::from(format!("{home}/eq_assets/EQ_Files/qcat.s3d"));
    let obj = PathBuf::from(format!("{home}/eq_assets/EQ_Files/qcat_obj.s3d"));
    if !main.exists() { eprintln!("skip"); return; }
    let out = tempfile::tempdir().unwrap();
    let glb = out.path().join("qcat.glb");
    eqoxide_asset_server::zone::bake_zone(&main, obj.exists().then_some(obj.as_path()), &glb).unwrap();
    assert!(glb.metadata().unwrap().len() > 0);

    // Reload the glb and assert: named textures preserved, geometry present.
    let g = gltf::Gltf::open(&glb).unwrap();
    let named_images = g.document.images().filter(|i| i.name().is_some()).count();
    assert!(named_images > 0, "glb images must carry EQ texture names");
    let mesh_prims: usize = g.document.meshes().flat_map(|m| m.primitives()).count();
    assert!(mesh_prims > 0);
}
