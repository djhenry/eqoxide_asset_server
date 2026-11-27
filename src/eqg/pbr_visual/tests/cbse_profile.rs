use super::*;

const CBSE: &str = "Opaque_MaxCBSE1.fx";
// Source order is independent of the schema's validation order.
const PROPERTIES: &[(&str, u32)] = &[
    ("e_fShininess0", 0),
    ("e_TextureDiffuse0", 2),
    ("e_TextureNormal0", 2),
    ("e_fEnvMapStrength0", 0),
    ("e_TextureEnvironment0", 2),
];

fn source_model(shader: &str, properties: &[(&str, u32)], terrain: bool) -> Vec<u8> {
    let mut bytes = model(shader, properties, terrain);
    let header = if terrain { 24 } else { 28 };
    let old_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut names = Vec::new();
    let normal = old_len as u32 + string(&mut names, "normal.png");
    let environment = old_len as u32 + string(&mut names, "environment.png");
    let strings_len = old_len + names.len();
    bytes.splice(header + old_len..header + old_len, names);
    bytes[8..12].copy_from_slice(&(strings_len as u32).to_le_bytes());
    for (i, (name, _)) in properties.iter().enumerate() {
        let value = match *name {
            "e_TextureNormal0" => normal,
            "e_TextureEnvironment0" => environment,
            "e_fShininess0" => 0x7fc01234,
            "e_fEnvMapStrength0" => 0xff800000,
            _ => continue,
        };
        let pos = header + strings_len + 16 + i * 12 + 8;
        bytes[pos..pos + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn distinct_png(rgba: [u8; 4]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba(rgba))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn fixture(
    shader: &str,
    properties: &[(&str, u32)],
    normal: Option<Vec<u8>>,
    environment: Option<Vec<u8>>,
) -> Fixture {
    let mut members = vec![
        ("terrain.ter".into(), source_model(shader, properties, true)),
        ("object.mod".into(), source_model(shader, properties, false)),
        ("Asymmetric.PNG".into(), png()),
    ];
    if let Some(bytes) = normal {
        members.push(("normal.png".into(), bytes));
    }
    if let Some(bytes) = environment {
        members.push(("environment.png".into(), bytes));
    }
    Fixture::members(members)
}

fn valid(shader: &str, properties: &[(&str, u32)]) -> Fixture {
    fixture(
        shader,
        properties,
        Some(distinct_png([127, 127, 255, 1])),
        Some(distinct_png([23, 45, 67, 89])),
    )
}

fn visual(f: &Fixture) -> Vec<u8> {
    let resolved = f.load().unwrap();
    let output = f._tmp.path().join("visual.glb");
    crate::static_scene::write_static_visual_with_surface_profile(
        &resolved.scene,
        &resolved.recommended_material_policy,
        crate::static_scene::StaticMaterialProfile::ScalarPbrHdrBlendV1,
        &"0".repeat(64),
        &output,
    )
    .unwrap();
    std::fs::read(output).unwrap()
}

#[test]
fn exact_cbse_has_opaque_editable_output_identical_to_cbsge_control() {
    let f = valid(CBSE, PROPERTIES);
    let bytes = visual(&f);
    assert_eq!(bytes, visual(&valid("Opaque_MaxCBSGE1.fx", PROPERTIES)));
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    assert_eq!(gltf.images().len(), 1);
    for material in gltf.materials() {
        assert_eq!(material.alpha_mode(), gltf::material::AlphaMode::Opaque);
        assert!(!material.double_sided());
        assert!(material.normal_texture().is_none());
        assert_eq!(
            material.pbr_metallic_roughness().base_color_factor(),
            [1.; 4]
        );
        assert_eq!(material.pbr_metallic_roughness().metallic_factor(), 0.);
        assert_eq!(material.pbr_metallic_roughness().roughness_factor(), 1.);
    }
    for mesh in gltf.meshes() {
        for primitive in mesh.primitives() {
            assert!(primitive.get(&gltf::Semantic::Colors(0)).is_none());
        }
    }
    #[cfg(feature = "editable-visual")]
    {
        let decoded = eqoxide_assets::decode_static_visual(&bytes, &Default::default()).unwrap();
        assert_eq!(decoded.images.len(), 1);
        crate::editable_visual::export_editable_visual(&bytes, &Default::default()).unwrap();
    }
}

#[test]
fn cbse_retains_distinct_unapplied_resources_and_raw_scalar_order() {
    let f = valid(CBSE, PROPERTIES);
    let r = f.load().unwrap();
    assert_eq!(r.preserved_resources.len(), 3);
    assert_eq!(r.scene.textures.len(), 1);
    assert_eq!(r.preserved_resources[0].bytes, png());
    assert_eq!(
        r.preserved_resources[1].bytes,
        distinct_png([127, 127, 255, 1])
    );
    assert_eq!(
        r.preserved_resources[2].bytes,
        distinct_png([23, 45, 67, 89])
    );
    assert_eq!(
        r.report
            .resources
            .iter()
            .map(|v| v.output_texture_index)
            .collect::<Vec<_>>(),
        [Some(0), None, None]
    );
    for row in &r.report.materials {
        assert_eq!(
            row.properties
                .iter()
                .map(|p| (p.name.as_str(), p.kind))
                .collect::<Vec<_>>(),
            PROPERTIES
        );
        assert_eq!(row.properties[0].value_word, 0x7fc01234);
        assert_eq!(row.properties[3].value_word, 0xff800000);
        assert_eq!(row.coverage, "OPAQUE");
        assert!(row
            .decisions
            .iter()
            .any(|d| d.contains("normal/coverage/environment sampling; raw inputs retained")));
    }
    let mut reversed = PROPERTIES.to_vec();
    reversed.reverse();
    let reversed = valid(CBSE, &reversed);
    let r = reversed.load().unwrap();
    assert_eq!(r.report.materials[0].properties[1].value_word, 0xff800000);
    assert_eq!(r.report.materials[0].properties[4].value_word, 0x7fc01234);
    assert_eq!(visual(&f), visual(&reversed));
}

#[test]
fn cbse_inventory_compatibility_never_bypasses_image_or_version_guards() {
    for (normal, environment, diagnostic) in [
        (None, Some(png()), "normal.png"),
        (Some(png()), None, "environment.png"),
        (
            Some(b"invalid normal".to_vec()),
            Some(png()),
            "validate required image",
        ),
        (
            Some(png()),
            Some(b"invalid environment".to_vec()),
            "validate required image",
        ),
    ] {
        let f = fixture(CBSE, PROPERTIES, normal, environment);
        let inventory = inventory(&f);
        assert_eq!(inventory.unsupported_required_materials, 0);
        assert_eq!(inventory.readiness, "not-evaluated");
        let error = format!("{:#}", f.load().unwrap_err());
        assert!(error.contains(diagnostic), "{error}");
    }
    let f = valid(CBSE, PROPERTIES);
    std::fs::write(&f.descriptor, sole_object_descriptor(1)).unwrap();
    let inventory = inventory(&f);
    assert_eq!(inventory.required_materials, 2);
    assert_eq!(inventory.unsupported_required_materials, 0);
    assert_eq!(inventory.readiness, "not-evaluated");
    assert!(format!("{:#}", f.load().unwrap_err()).contains("requires descriptor v2"));
}

#[test]
fn cbse_near_spellings_and_missing_environment_marker_remain_refused() {
    for shader in [
        "opaque_MaxCBSE1.fx",
        "Opaque_MaxCBSE1.FX",
        "Opaque_MaxCBSE1",
        "Opaque_MaxCBSE2.fx",
        "Opaque_MaxCBSE1.fx.extra",
    ] {
        let f = valid(shader, PROPERTIES);
        assert_eq!(inventory(&f).unsupported_required_materials, 2);
        assert!(f.load().is_err());
    }
    let mut object = source_model(CBSE, PROPERTIES, false);
    let start = object
        .windows(16)
        .position(|v| v == b"environment.png\0")
        .unwrap();
    object[start..start + 5].copy_from_slice(b"None\0");
    let f = Fixture::members(vec![
        ("terrain.ter".into(), source_model(CBSE, PROPERTIES, true)),
        ("object.mod".into(), object),
        ("Asymmetric.PNG".into(), png()),
        ("normal.png".into(), png()),
        ("environment.png".into(), png()),
    ]);
    assert!(format!("{:#}", f.load().unwrap_err()).contains("None"));
}
