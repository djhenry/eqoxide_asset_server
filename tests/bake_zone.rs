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

/// qeytoqrg.s3d keeps a stale, fully OPAQUE DXT1 copy of `treea.bmp` / `tree70.bmp`
/// that no zone-WLD mesh references; the alpha-keyed DXT5 originals live in
/// qeytoqrg_obj.s3d, which is where the tree objects reference them from. Baking with
/// a blind [main, obj] archive search picked the opaque copy, so the trees came out as
/// MASK materials over a texture with no transparent texel — the alpha test discarded
/// nothing and they rendered as solid green blocks (eqoxide#688).
#[test]
#[ignore = "requires ~/eq_assets/everquest_rof2/qeytoqrg{,_obj}.s3d"]
fn masked_foliage_resolves_the_object_archives_alpha_keyed_texture() {
    let home = std::env::var("HOME").unwrap();
    let main = PathBuf::from(format!("{home}/eq_assets/everquest_rof2/qeytoqrg.s3d"));
    let obj = PathBuf::from(format!("{home}/eq_assets/everquest_rof2/qeytoqrg_obj.s3d"));
    if !main.exists() { eprintln!("skip"); return; }
    let out = tempfile::tempdir().unwrap();
    let glb = out.path().join("qeytoqrg.glb");
    eqoxide_asset_server::zone::bake_zone(&main, obj.exists().then_some(obj.as_path()), &glb).unwrap();

    let g = gltf::Gltf::open(&glb).unwrap();
    let doc = &g.document;
    let buffers = gltf::import_buffers(doc, Some(out.path()), g.blob.clone()).unwrap();
    let images = gltf::import_images(doc, Some(out.path()), &buffers).unwrap();

    // The client links meshes to textures by IMAGE NAME, so each name must be unique.
    let names: Vec<String> = doc.images().map(|i| i.name().unwrap_or("").to_string()).collect();
    let mut uniq = names.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), names.len(), "duplicate glTF image names are ambiguous client-side");

    // Every image behind a MASK material must actually have something to mask out.
    let mut checked = 0;
    for m in doc.materials().filter(|m| m.alpha_mode() == gltf::material::AlphaMode::Mask) {
        let Some(info) = m.pbr_metallic_roughness().base_color_texture() else { continue };
        let idx = info.texture().source().index();
        let raw = &images[idx];
        let clear = match raw.format {
            gltf::image::Format::R8G8B8A8 => raw.pixels.chunks_exact(4).filter(|p| p[3] < 128).count(),
            _ => 0,
        };
        assert!(
            clear > 0,
            "MASK material '{}' (image '{}') baked with no transparent texel — \
             the alpha test has nothing to discard and it renders opaque",
            m.name().unwrap_or("?"), names[idx],
        );
        checked += 1;
    }
    assert!(checked > 0, "expected qeytoqrg to bake at least one MASK material");
}
