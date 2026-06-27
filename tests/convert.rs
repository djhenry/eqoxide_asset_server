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

/// Assert that the elf GLB contains face and hair variant primitives with non-None
/// base-color textures. Face variants are tagged with eq_head_part="face" (8 variants,
/// eq_part_index 1-8); hair variants are tagged eq_head_part="hair" (7 styles).
/// Both must have a texture_idx set (i.e., `baseColorTexture.index` in the glTF JSON)
/// so the rendered character shows colored skin, not a solid/grey default.
///
/// Root cause confirmed (2026-06-27): `base_color_texture().source()` correctly
/// resolves the primary frame (frame 0) of the face material's SimpleSpriteDef to
/// the 256×128 head-skin DDS (`{race}hesk{N:02}.dds`); the secondary frame
/// (`elfhe000{N}.dds_layer`, 8×8 DXT5 face overlay) is not needed for basic
/// rendering. Hair variants are synthesized from `{race}hesk{N}1.dds` which are also
/// present in the archive. Both paths are exercised by this test.
#[test]
#[ignore]
fn elf_glb_face_and_hair_primitives_have_textures() {
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

    let mut face_found = 0u8; // bitmask of eq_part_index values 1-8 found
    let mut face_missing_tex = Vec::new();
    let mut hair_found = 0u8; // bitmask of eq_part_index values 1-7 found
    let mut hair_missing_tex = Vec::new();

    for prim in prims {
        let extras = &prim["extras"];
        let head_part = extras["eq_head_part"].as_str().unwrap_or("");
        let part_idx = extras["eq_part_index"].as_u64().unwrap_or(0) as u8;
        let mat_idx = prim["material"].as_u64().unwrap_or(0) as usize;
        let has_tex = mats[mat_idx]["pbrMetallicRoughness"]["baseColorTexture"]["index"].is_number();

        match head_part {
            "face" if part_idx >= 1 && part_idx <= 8 => {
                face_found |= 1 << (part_idx - 1);
                if !has_tex {
                    face_missing_tex.push(part_idx);
                }
            }
            "hair" if part_idx >= 1 && part_idx <= 7 => {
                hair_found |= 1 << (part_idx - 1);
                if !has_tex {
                    hair_missing_tex.push(part_idx);
                }
            }
            _ => {}
        }
    }

    // All 8 face variants must be present.
    assert_eq!(face_found, 0b1111_1111,
        "missing face variants: expected all 8, found bitmask 0b{:08b}", face_found);
    assert!(face_missing_tex.is_empty(),
        "face variants missing base-color texture: {:?}", face_missing_tex);

    // All 7 hair style variants must be present.
    assert_eq!(hair_found, 0b0111_1111,
        "missing hair styles: expected all 7, found bitmask 0b{:07b}", hair_found);
    assert!(hair_missing_tex.is_empty(),
        "hair styles missing base-color texture: {:?}", hair_missing_tex);

    // Face variant 1 (the default) must NOT carry eq_default_hidden.
    let face1 = prims.iter().find(|p| {
        p["extras"]["eq_head_part"].as_str() == Some("face")
            && p["extras"]["eq_part_index"].as_u64() == Some(1)
    }).expect("face variant 1 not found");
    assert!(
        face1["extras"]["eq_default_hidden"].is_null(),
        "face variant 1 must not have eq_default_hidden"
    );
}
