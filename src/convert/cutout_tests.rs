use super::*;

#[test]
fn explicit_cutout_threshold_preserves_legacy_mask_and_texture_alpha() {
    let mut material = MaterialData {
        name: "cutout".into(),
        texture_idx: Some(0),
        base_color: [1.; 4],
        alpha_mode: AlphaMode::Cutout(64),
        anim: None,
    };
    let json = material_to_gltf(&material);
    assert_eq!(json["alphaMode"], "MASK");
    assert!((json["alphaCutoff"].as_f64().unwrap() - 64. / 255.).abs() < 1e-7);
    material.alpha_mode = AlphaMode::Cutout(192);
    let native = material_to_gltf(&material);
    assert!((native["alphaCutoff"].as_f64().unwrap() - 192. / 255.).abs() < 1e-7);
    assert_eq!(native["pbrMetallicRoughness"]["baseColorFactor"][3], 1.0);
    material.alpha_mode = AlphaMode::Masked;
    assert_eq!(material_to_gltf(&material)["alphaCutoff"], 0.5);

    let image = image::RgbaImage::from_raw(
        4,
        1,
        vec![1, 2, 3, 0, 4, 5, 6, 191, 7, 8, 9, 192, 10, 11, 12, 255],
    )
    .unwrap();
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let png = encode_texture_png(bytes.get_ref(), AlphaMode::Cutout(64), "cutout.png").unwrap();
    assert_eq!(image::load_from_memory(&png).unwrap().to_rgba8(), image);
}
