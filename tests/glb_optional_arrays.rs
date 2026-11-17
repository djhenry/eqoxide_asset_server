#[allow(dead_code)]
#[path = "../examples/static_pbr_fixture.rs"]
mod fixture;

use eqoxide_asset_server::static_scene::{
    write_static_visual, write_static_visual_with_material_policy, StaticTexture,
};
use serde_json::Value;
fn json(path: &std::path::Path) -> Value {
    let b = std::fs::read(path).unwrap();
    let n = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&b[20..20 + n]).unwrap()
}
#[test]
fn untextured_glb_omits_optional_collections_for_both_visual_profiles() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("no-images.glb");
    let (s, policy) = fixture::fixture_scene();
    assert!(s.textures.is_empty());
    for modern in [false, true] {
        if modern {
            write_static_visual_with_material_policy(&s, &policy, &"a".repeat(64), &p).unwrap();
        } else {
            write_static_visual(&s, &"a".repeat(64), &p).unwrap();
        }
        let j = json(&p);
        assert!(
            !j.as_object().unwrap().contains_key("images"),
            "glTF images must be omitted when empty"
        );
        assert!(
            !j.as_object().unwrap().contains_key("textures"),
            "glTF textures must be omitted when empty"
        );
    }
}
#[test]
fn textured_glb_keeps_nonempty_image_and_texture_declarations() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("image.glb");
    let (mut s, policy) = fixture::fixture_scene();
    let mut bytes = std::io::Cursor::new(vec![]);
    image::RgbaImage::from_pixel(2, 1, image::Rgba([32, 64, 128, 192]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    s.textures.push(StaticTexture {
        name: "color".into(),
        png_bytes: bytes.into_inner(),
    });
    s.materials[0].texture_index = Some(0);
    write_static_visual_with_material_policy(&s, &policy, &"a".repeat(64), &p).unwrap();
    let j = json(&p);
    assert_eq!(j["images"].as_array().unwrap().len(), 1);
    assert_eq!(j["textures"].as_array().unwrap().len(), 1);
    assert_eq!(
        j["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"],
        0
    );
}
