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

/// Assert the elf GLB contains:
/// 1. Hairstyle-swappable head region primitives (N∈{1,4,5}): 8 variants H=0..7
///    each tagged `eq_hairstyle: H`.  H=0 is the bald base (no eq_default_hidden).
///    H=1..7 carry `eq_default_hidden: true`.
/// 2. Fixed ear region materials (elfhesk02 = ear tips, elfhesk06 = ear base) are
///    present in the GLB and none of their primitives carry eq_hairstyle.
///
/// This verifies the head conversion correctly emits ALL 8 head polygon groups
/// (not just the largest) and that ear geometry is no longer silently dropped.
#[test]
#[ignore]
fn elf_glb_has_ears_and_hairstyle_variants() {
    let raw = PathBuf::from(std::env::var("EQ_ASSETS")
        .unwrap_or_else(|_| format!("{}/eq_assets/EQ_Files", std::env::var("HOME").unwrap())));
    let src = raw.join("globalelf_chr.s3d");
    if !src.exists() { eprintln!("skip: {src:?} missing"); return; }
    let out = tempfile::tempdir().unwrap();
    let glb = out.path().join("elf.glb");
    eqoxide_asset_server::convert::s3d_to_glb(&src, &glb, true).unwrap();

    // Parse the GLB JSON chunk.
    let glb_data = std::fs::read(&glb).unwrap();
    let json_len = u32::from_le_bytes([glb_data[12], glb_data[13], glb_data[14], glb_data[15]]) as usize;
    let json_str = std::str::from_utf8(&glb_data[20..20 + json_len])
        .unwrap()
        .trim_end_matches(' ');
    let gltf: serde_json::Value = serde_json::from_str(json_str).unwrap();

    let prims = gltf["meshes"][0]["primitives"].as_array().unwrap();
    let mats = gltf["materials"].as_array().unwrap();

    // ── Hairstyle variant checks ──────────────────────────────────────────────
    // The three swappable regions (N=1,4,5) each produce 8 primitives H=0..7.
    // So for each H there should be at least 3 primitives with eq_hairstyle:H.
    for h in 0u8..=7 {
        let count = prims.iter().filter(|p| {
            p["extras"]["eq_hairstyle"].as_u64() == Some(h as u64)
        }).count();
        assert!(count >= 3,
            "hairstyle H={} should have ≥3 primitives (one per swappable region N=1,4,5), found {}", h, count);
    }

    // H=0 primitives must NOT carry eq_default_hidden.
    for prim in prims.iter().filter(|p| p["extras"]["eq_hairstyle"].as_u64() == Some(0)) {
        assert!(prim["extras"]["eq_default_hidden"].is_null(),
            "hairstyle H=0 primitives must not have eq_default_hidden");
    }

    // H≥1 primitives must carry eq_default_hidden:true.
    for prim in prims.iter().filter(|p| {
        p["extras"]["eq_hairstyle"].as_u64().map_or(false, |h| h >= 1)
    }) {
        assert_eq!(prim["extras"]["eq_default_hidden"].as_bool(), Some(true),
            "hairstyle H≥1 primitives must have eq_default_hidden:true");
    }

    // All hairstyle primitives must have a base-color texture.
    let missing_tex: Vec<u64> = prims.iter().filter_map(|p| {
        let h = p["extras"]["eq_hairstyle"].as_u64()?;
        let mi = p["material"].as_u64().unwrap_or(0) as usize;
        let has_tex = mats[mi]["pbrMetallicRoughness"]["baseColorTexture"]["index"].is_number();
        if !has_tex { Some(h) } else { None }
    }).collect();
    assert!(missing_tex.is_empty(),
        "hairstyle primitives missing base-color texture for H values: {:?}", missing_tex);

    // ── Ear group checks ─────────────────────────────────────────────────────
    // Fixed ear regions: N=2 (ear tips) → material "elfhesk02",
    //                    N=6 (ear base) → material "elfhesk06".
    // Each must exist in the materials list and have NO eq_hairstyle on its prims.
    for ear_mat in &["elfhesk02", "elfhesk06"] {
        let mat_idx = mats.iter().position(|m| m["name"].as_str() == Some(ear_mat))
            .unwrap_or_else(|| panic!("ear material '{}' not found in GLB", ear_mat));
        let ear_prims: Vec<&serde_json::Value> = prims.iter()
            .filter(|p| p["material"].as_u64() == Some(mat_idx as u64))
            .collect();
        assert!(!ear_prims.is_empty(),
            "no primitive found for ear material '{}'", ear_mat);
        for ep in &ear_prims {
            assert!(ep["extras"]["eq_hairstyle"].is_null(),
                "ear group '{}' must not have eq_hairstyle extras", ear_mat);
        }
    }
}
