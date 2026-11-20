use eqoxide_asset_server::editable_visual::{export_editable_visual, EditableVisualLimits};
use eqoxide_asset_server::static_scene::{
    write_static_visual_with_surface_profile, StaticAlphaMode, StaticInstance, StaticMaterial,
    StaticMaterialPolicy, StaticMaterialProfile, StaticMesh, StaticPrimitive, StaticScene,
    StaticTexture,
};
use glam::{Mat4, Vec3};
use std::io::Cursor;

fn fixture() -> Vec<u8> {
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([91, 173, 204, 150]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let scene = StaticScene {
        meshes: vec![StaticMesh {
            name: "triangle".into(),
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            normals: vec![[0., 0., 1.]; 3],
            uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
            primitives: vec![StaticPrimitive {
                indices: vec![0, 1, 2],
                material_index: 0,
                colors: None,
            }],
        }],
        materials: vec![StaticMaterial {
            name: "paint".into(),
            base_color: [0.4, 0.6, 0.8, 0.65],
            texture_index: Some(0),
            alpha_mode: StaticAlphaMode::Blend,
            double_sided: true,
        }],
        textures: vec![StaticTexture {
            name: "texture".into(),
            png_bytes: png.into_inner(),
        }],
        instances: vec![
            StaticInstance {
                mesh_index: 0,
                matrix: Mat4::IDENTITY.to_cols_array_2d(),
            },
            StaticInstance {
                mesh_index: 0,
                matrix: (Mat4::from_translation(Vec3::new(2., 3., 4.))
                    * Mat4::from_rotation_z(0.3)
                    * Mat4::from_scale(Vec3::splat(0.25)))
                .to_cols_array_2d(),
            },
        ],
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.glb");
    write_static_visual_with_surface_profile(
        &scene,
        &StaticMaterialPolicy::default(),
        StaticMaterialProfile::ScalarPbrHdrBlendV1,
        &"1".repeat(64),
        &path,
    )
    .unwrap();
    std::fs::read(path).unwrap()
}

#[test]
fn external_export_preserves_visual_content_and_removes_activation_header() {
    let source = fixture();
    let bundle = export_editable_visual(&source, &EditableVisualLimits::default()).unwrap();
    assert_eq!(bundle.files["reference.glb"], source);
    let glb = gltf::binary::Glb::from_slice(&source).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&glb.json).unwrap();
    let result: serde_json::Value = serde_json::from_slice(&bundle.files["scene.gltf"]).unwrap();
    assert_eq!(result["accessors"], original["accessors"]);
    assert_eq!(result["bufferViews"], original["bufferViews"]);
    assert_eq!(result["nodes"][1]["matrix"], original["nodes"][1]["matrix"]);
    assert_eq!(
        result["meshes"][0]["primitives"],
        original["meshes"][0]["primitives"]
    );
    assert_eq!(
        result["materials"][0]["pbrMetallicRoughness"],
        original["materials"][0]["pbrMetallicRoughness"]
    );
    assert_eq!(result["materials"][0]["alphaMode"], "BLEND");
    assert!(result["extras"].get("eqoxideAsset").is_none());
    assert_eq!(result["buffers"][0]["uri"], "scene.bin");
    let declared = original["buffers"][0]["byteLength"].as_u64().unwrap() as usize;
    assert_eq!(&bundle.files["scene.bin"], &glb.bin.unwrap()[..declared]);
    assert_eq!(result["images"][0]["uri"], "images/image-000000.png");
    assert!(result["images"][0].get("bufferView").is_none());
    assert_eq!(
        result["materials"][0]["extras"]["eqoxideAuthoring"]["id"],
        "material-000000"
    );
    assert_eq!(
        result["nodes"][1]["extras"]["eqoxideAuthoring"]["id"],
        "node-000001"
    );
    assert!(bundle.files.contains_key("authoring.json"));
}

fn repack(source: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    use std::borrow::Cow;
    let mut glb = gltf::binary::Glb::from_slice(source).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&glb.json).unwrap();
    edit(&mut json);
    glb.json = Cow::Owned(serde_json::to_vec(&json).unwrap());
    glb.to_vec().unwrap()
}

#[test]
fn textureless_export_omits_empty_optional_arrays() {
    let source = repack(&fixture(), |j| {
        j["images"] = serde_json::json!([]);
        j["textures"] = serde_json::json!([]);
        j["materials"][0]["pbrMetallicRoughness"]
            .as_object_mut()
            .unwrap()
            .remove("baseColorTexture");
    });
    let bundle = export_editable_visual(&source, &EditableVisualLimits::default()).unwrap();
    let j: serde_json::Value = serde_json::from_slice(&bundle.files["scene.gltf"]).unwrap();
    assert!(j.get("images").is_none());
    assert!(j.get("textures").is_none());
    assert!(j.get("extensionsUsed").is_none());
    assert!(j.get("extensionsRequired").is_none());
    assert!(!bundle.files.keys().any(|p| p.starts_with("images/")));
}

#[test]
fn output_byte_limit_charges_sidecar_and_exact_boundary() {
    let source = fixture();
    let mut limits = EditableVisualLimits::default();
    let bundle = export_editable_visual(&source, &limits).unwrap();
    let total: usize = bundle.files.values().map(Vec::len).sum();
    limits.max_output_bytes = total;
    assert!(export_editable_visual(&source, &limits).is_ok());
    limits.max_output_bytes -= 1;
    assert!(export_editable_visual(&source, &limits).is_err());
}

#[test]
fn limits_and_malformed_references_refuse() {
    let source = fixture();
    for limits in [
        EditableVisualLimits {
            max_glb_bytes: source.len() - 1,
            ..Default::default()
        },
        EditableVisualLimits {
            max_json_bytes: 1,
            ..Default::default()
        },
        EditableVisualLimits {
            max_resources: 1,
            ..Default::default()
        },
        EditableVisualLimits {
            max_single_png_bytes: 1,
            ..Default::default()
        },
        EditableVisualLimits {
            max_png_bytes: 1,
            ..Default::default()
        },
        EditableVisualLimits {
            max_output_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(export_editable_visual(&source, &limits).is_err());
    }
    assert!(export_editable_visual(b"malformed", &Default::default()).is_err());
    for source in [
        repack(&source, |j| {
            j["extras"]["eqoxideAsset"]["role"] = "collision".into();
        }),
        repack(&source, |j| {
            j["materials"][0]["normalTexture"] = serde_json::json!({"index":0});
        }),
        repack(&source, |j| {
            j["textures"][0]["source"] = 999.into();
        }),
    ] {
        assert!(export_editable_visual(&source, &Default::default()).is_err());
    }
}

#[test]
fn deterministic_files_and_sidecar_hashes_are_reference_scoped() {
    use sha2::{Digest, Sha256};
    let source = fixture();
    let first = export_editable_visual(&source, &Default::default()).unwrap();
    let second = export_editable_visual(&source, &Default::default()).unwrap();
    assert_eq!(first.files, second.files);
    let report: serde_json::Value = serde_json::from_slice(&first.files["authoring.json"]).unwrap();
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(
        report["referenceSha256"],
        hex::encode(Sha256::digest(&source))
    );
    assert!(report["files"].get("authoring.json").is_none());
    for (path, content) in &first.files {
        if path == "authoring.json" {
            continue;
        }
        assert_eq!(
            report["files"][path]["sha256"],
            hex::encode(Sha256::digest(content))
        );
        assert_eq!(report["files"][path]["bytes"], content.len());
    }
}

#[test]
fn embedded_bin_alignment_padding_stays_only_in_reference() {
    let input = fixture();
    let mut packed = gltf::binary::Glb::from_slice(&input).unwrap();
    let mut j: serde_json::Value = serde_json::from_slice(&packed.json).unwrap();
    let mut bin = packed.bin.as_ref().unwrap().to_vec();
    bin.push(0x7a);
    j["buffers"][0]["byteLength"] = bin.len().into();
    packed.bin = Some(std::borrow::Cow::Owned(bin));
    packed.json = std::borrow::Cow::Owned(serde_json::to_vec(&j).unwrap());
    let source = packed.to_vec().unwrap();
    let original = gltf::binary::Glb::from_slice(&source).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&original.json).unwrap();
    let declared = json["buffers"][0]["byteLength"].as_u64().unwrap() as usize;
    assert!(
        original.bin.as_ref().unwrap().len() > declared,
        "fixture must carry alignment padding"
    );
    let bundle = export_editable_visual(&source, &Default::default()).unwrap();
    assert_eq!(bundle.files["scene.bin"].len(), declared);
    assert_eq!(bundle.files["reference.glb"], source);
}

#[test]
fn legacy_implicit_unlit_is_not_exported_as_portable_pbr() {
    let source = repack(&fixture(), |j| {
        j["extras"]["eqoxideAsset"]["requirements"]["capabilities"] =
            serde_json::json!(["static-visual-v1"]);
        j.as_object_mut().unwrap().remove("extensionsUsed");
        j.as_object_mut().unwrap().remove("extensionsRequired");
    });
    assert!(export_editable_visual(&source, &Default::default()).is_err());
}

#[test]
fn high_precision_png_refuses_without_changing_original() {
    use std::borrow::Cow;
    let source = fixture();
    let mut glb = gltf::binary::Glb::from_slice(&source).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&glb.json).unwrap();
    let view = json["images"][0]["bufferView"].as_u64().unwrap() as usize;
    let mut png = Cursor::new(Vec::new());
    image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
        1,
        1,
        image::Rgba([65535, 12345, 54321, 30000]),
    )
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let mut bin = glb.bin.as_ref().unwrap().to_vec();
    let offset = bin.len();
    let png = png.into_inner();
    bin.extend_from_slice(&png);
    json["bufferViews"][view]["byteOffset"] = offset.into();
    json["bufferViews"][view]["byteLength"] = png.len().into();
    json["buffers"][0]["byteLength"] = bin.len().into();
    glb.bin = Some(Cow::Owned(bin));
    glb.json = Cow::Owned(serde_json::to_vec(&json).unwrap());
    let source = glb.to_vec().unwrap();
    eqoxide_assets::decode_static_visual(&source, &Default::default()).unwrap();
    let before = source.clone();
    let error = export_editable_visual(&source, &Default::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("eight-bit"), "{error}");
    assert_eq!(source, before);
}

#[test]
fn unchanged_numeric_tokens_do_not_pass_through_float_reserialization() {
    use serde_json::value::RawValue;
    use std::collections::BTreeMap;
    let source = repack(&fixture(), |j| {
        j["nodes"][1]["matrix"] = serde_json::json!([
            0.2796982,
            0,
            0,
            0,
            0,
            0.2796982,
            0,
            0,
            0,
            0,
            0.2796982,
            0,
            -0.10848464071750641,
            3,
            4,
            1
        ]);
    });
    let source = repack(&source, |j| {
        j["nodes"][1]["matrix"][12] = serde_json::json!(-0.10848464071750641);
    });
    let glb = gltf::binary::Glb::from_slice(&source).unwrap();
    let original: BTreeMap<String, Box<RawValue>> = serde_json::from_slice(&glb.json).unwrap();
    let bundle = export_editable_visual(&source, &Default::default()).unwrap();
    let result: BTreeMap<String, Box<RawValue>> =
        serde_json::from_slice(&bundle.files["scene.gltf"]).unwrap();
    let before: Vec<BTreeMap<String, Box<RawValue>>> =
        serde_json::from_str(original["nodes"].get()).unwrap();
    let after: Vec<BTreeMap<String, Box<RawValue>>> =
        serde_json::from_str(result["nodes"].get()).unwrap();
    let tokens = |raw: &RawValue| {
        raw.get()
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect::<String>()
    };
    assert!(tokens(&before[1]["matrix"]).contains("-0.10848464071750641"));
    assert_eq!(tokens(&before[1]["matrix"]), tokens(&after[1]["matrix"]));
}

#[test]
fn report_names_the_optional_collections_actually_omitted() {
    let source = repack(&fixture(), |j| {
        j["images"] = serde_json::json!([]);
        j["textures"] = serde_json::json!([]);
        j["materials"][0]["pbrMetallicRoughness"]
            .as_object_mut()
            .unwrap()
            .remove("baseColorTexture");
    });
    let original = gltf::binary::Glb::from_slice(&source).unwrap();
    let j: serde_json::Value = serde_json::from_slice(&original.json).unwrap();
    let expected: Vec<_> = j
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::Array(a) if a.is_empty() => Some(k.as_str()),
            _ => None,
        })
        .collect();
    let bundle = export_editable_visual(&source, &Default::default()).unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&bundle.files["authoring.json"]).unwrap();
    assert_eq!(
        report["omittedEmptyCollections"],
        serde_json::json!(expected)
    );
}
