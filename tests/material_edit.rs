mod support {
    include!("support/material_edit.rs");
}
use serde_json::{json, value::RawValue, Value};
use support::*;
#[test]
fn author_fixture_is_reader_admitted_and_editor_mapping_is_unambiguous() {
    let reference = reference();
    let decoded = eqoxide_assets::decode_static_visual(&reference, &Default::default()).unwrap();
    assert_eq!(decoded.materials.len(), 2);
    let (edited, _) = editor(&reference, [200, 17, 41, 91]);
    let (json, _) = unpack(&edited);
    assert_eq!(
        json["materials"][0]["extras"]["eqoxideAuthoring"]["id"],
        "material-000001"
    );
    assert_eq!(
        json["materials"][1]["extras"]["eqoxideAuthoring"]["id"],
        "material-000000"
    );
    assert_eq!(
        json["materials"][1]["pbrMetallicRoughness"]["roughnessFactor"],
        0.42
    );
    assert!(compact(&raw_root(&reference)["nodes"]).contains("3.25000000000000000001"));
}

use eqoxide_asset_server::material_edit::{reimport_material_edits, MaterialEditLimits};

// Catches applying editor ordinal 0 or importing editor geometry; expected values are literal.
#[test]
fn selected_identity_survives_reordering_and_preserves_original_geometry_tokens_and_bin() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let result = reimport_material_edits(
        &reference,
        &sidecar,
        &edited,
        &["material-000000".into()],
        &MaterialEditLimits::default(),
    )
    .unwrap();
    let (before, before_bin) = unpack(&reference);
    let (after, after_bin) = unpack(&result.glb);
    assert_eq!(
        after["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"],
        0.42
    );
    assert_eq!(
        after["materials"][1]["pbrMetallicRoughness"]["roughnessFactor"],
        1.0
    );
    assert_eq!(after["materials"][0]["name"], "shared name");
    let declared = before["buffers"][0]["byteLength"].as_u64().unwrap() as usize;
    assert_eq!(&after_bin[..declared], &before_bin[..declared]);
    let original_raw = raw_root(&reference);
    let output_raw = raw_root(&result.glb);
    for field in ["meshes", "nodes", "accessors", "scenes", "scene"] {
        assert_eq!(
            compact(&original_raw[field]),
            compact(&output_raw[field]),
            "changed {field}"
        );
    }
    let original_mats: Vec<Box<RawValue>> =
        serde_json::from_str(original_raw["materials"].get()).unwrap();
    let output_mats: Vec<Box<RawValue>> =
        serde_json::from_str(output_raw["materials"].get()).unwrap();
    assert_eq!(compact(&original_mats[1]), compact(&output_mats[1]));
    eqoxide_assets::decode_static_visual(&result.glb, &Default::default()).unwrap();
}

// Catches retaining the old PNG, wrong channel order, or replacing shared unselected texture data.
#[test]
fn selected_png_channels_replace_only_the_selected_material_texture() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let result = reimport_material_edits(
        &reference,
        &sidecar,
        &edited,
        &["material-000000".into()],
        &MaterialEditLimits::default(),
    )
    .unwrap();
    let (output, binary) = unpack(&result.glb);
    let pixels = |material: usize| {
        let texture = output["materials"][material]["pbrMetallicRoughness"]["baseColorTexture"]
            ["index"]
            .as_u64()
            .unwrap() as usize;
        let image = output["textures"][texture]["source"].as_u64().unwrap() as usize;
        let view = output["images"][image]["bufferView"].as_u64().unwrap() as usize;
        let range = &output["bufferViews"][view];
        let start = range.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let length = range["byteLength"].as_u64().unwrap() as usize;
        image::load_from_memory(&binary[start..start + length])
            .unwrap()
            .to_rgba8()
            .into_raw()
    };
    assert_eq!(pixels(0), [200, 17, 41, 91, 200, 17, 41, 91]);
    assert_eq!(pixels(1), [21, 93, 177, 221, 21, 93, 177, 221]);
    assert!(output.get("samplers").is_none());
}

fn rewrite(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut glb = gltf::binary::Glb::from_slice(bytes).unwrap();
    let mut root: Value = serde_json::from_slice(&glb.json).unwrap();
    edit(&mut root);
    glb.json = std::borrow::Cow::Owned(serde_json::to_vec(&root).unwrap());
    glb.to_vec().unwrap()
}
fn apply(
    reference: &[u8],
    sidecar: &[u8],
    edited: &[u8],
    limits: &MaterialEditLimits,
) -> anyhow::Result<eqoxide_asset_server::material_edit::ReimportedMaterialEdits> {
    reimport_material_edits(
        reference,
        sidecar,
        edited,
        &["material-000000".into()],
        limits,
    )
}

#[test]
fn ambiguous_foreign_missing_and_noncanonical_selection_refuse() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    for ids in [
        vec![],
        vec!["material-000000".into(), "material-000000".into()],
        vec!["material-0".into()],
        vec!["material-000002".into()],
    ] {
        assert!(
            reimport_material_edits(&reference, &sidecar, &edited, &ids, &Default::default())
                .is_err()
        );
    }
    for bad in [
        rewrite(&edited, |j| {
            j["materials"][1]["extras"]["eqoxideAuthoring"]["referenceSha256"] =
                json!("f".repeat(64))
        }),
        rewrite(&edited, |j| {
            j["materials"][1]["extras"]["eqoxideAuthoring"]["kind"] = json!("mesh")
        }),
        rewrite(&edited, |j| {
            j["materials"][1]["extras"].as_object_mut().unwrap().clear()
        }),
        rewrite(&edited, |j| {
            let duplicate = j["materials"][1].clone();
            j["materials"].as_array_mut().unwrap().push(duplicate)
        }),
    ] {
        assert!(apply(&reference, &sidecar, &bad, &Default::default()).is_err());
    }
    let mut changed = sidecar.clone();
    changed.push(b' ');
    assert!(apply(&reference, &changed, &edited, &Default::default()).is_err());
    let different = reference.iter().copied().chain([0]).collect::<Vec<_>>();
    assert!(apply(&different, &sidecar, &edited, &Default::default()).is_err());
}

#[test]
fn selected_unsupported_semantics_and_invalid_values_refuse() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let cases = vec![
        ("normalTexture", json!({"index":0})),
        ("occlusionTexture", json!({"index":0})),
        ("emissiveTexture", json!({"index":0})),
        ("emissiveFactor", json!([0, 0.1, 0])),
        ("doubleSided", json!(1)),
        ("alphaMode", json!("ADDITIVE")),
        ("alphaCutoff", json!(0.5)),
        ("extensions", json!({"KHR_materials_clearcoat":{}})),
        ("unknownOptional", json!(true)),
    ];
    for (field, value) in cases {
        let bad = rewrite(&edited, |j| j["materials"][1][field] = value);
        assert!(
            apply(&reference, &sidecar, &bad, &Default::default()).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("roughnessFactor", json!(-0.1)),
        ("metallicFactor", json!(1.1)),
        ("baseColorFactor", json!([1, 1, 1])),
        ("metallicRoughnessTexture", json!({"index":0})),
        ("baseColorTexture", json!({"index":0,"texCoord":1})),
        (
            "baseColorTexture",
            json!({"index":0,"extensions":{"KHR_texture_transform":{}}}),
        ),
    ] {
        let bad = rewrite(&edited, |j| {
            j["materials"][1]["pbrMetallicRoughness"][field] = value
        });
        assert!(
            apply(&reference, &sidecar, &bad, &Default::default()).is_err(),
            "{field}"
        );
    }
    for sampler in [
        json!({"magFilter":9728}),
        json!({"minFilter":9728}),
        json!({"wrapS":33071}),
        json!({"wrapT":33648}),
        json!({"minFilter":9986}),
        json!({"mystery":true}),
    ] {
        let bad = rewrite(&edited, |j| j["samplers"][0] = sampler);
        assert!(apply(&reference, &sidecar, &bad, &Default::default()).is_err());
    }
    for (field, value) in [
        ("uri", json!("file:///texture.png")),
        ("mimeType", json!("image/jpeg")),
    ] {
        let bad = rewrite(&edited, |j| j["images"][0][field] = value);
        assert!(apply(&reference, &sidecar, &bad, &Default::default()).is_err());
    }
    for field in ["byteStride", "target"] {
        let bad = rewrite(&edited, |j| {
            let view = j["images"][0]["bufferView"].as_u64().unwrap() as usize;
            j["bufferViews"][view][field] = json!(4)
        });
        assert!(apply(&reference, &sidecar, &bad, &Default::default()).is_err());
    }
    let bad = rewrite(&edited, |j| j["images"][0]["bufferView"] = json!(999));
    assert!(apply(&reference, &sidecar, &bad, &Default::default()).is_err());
    let mut bad = edited.clone();
    let (j, _) = unpack(&edited);
    let view = j["images"][0]["bufferView"].as_u64().unwrap() as usize;
    let offset = j["bufferViews"][view]["byteOffset"].as_u64().unwrap() as usize;
    let len = j["bufferViews"][view]["byteLength"].as_u64().unwrap() as usize;
    let bin_start = 28 + gltf::binary::Glb::from_slice(&edited).unwrap().json.len();
    bad[bin_start + offset + len - 1] ^= 0x80;
    assert!(apply(&reference, &sidecar, &bad, &Default::default()).is_err());
}

#[test]
fn omitted_material_defaults_remove_texture_and_preserve_unselected_data() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let edited = rewrite(&edited, |j| {
        let tag = j["materials"][1]["extras"].clone();
        j["materials"][1] = json!({"extras":tag});
        j["materials"][0]["unsupportedUnselectedShader"] = json!(true);
    });
    let result = apply(&reference, &sidecar, &edited, &Default::default()).unwrap();
    let (j, _) = unpack(&result.glb);
    assert_eq!(
        j["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"],
        json!([1.0, 1.0, 1.0, 1.0])
    );
    assert_eq!(
        j["materials"][0]["pbrMetallicRoughness"]["metallicFactor"],
        1.0
    );
    assert_eq!(
        j["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"],
        1.0
    );
    assert!(j["materials"][0]["pbrMetallicRoughness"]
        .get("baseColorTexture")
        .is_none());
    assert_eq!(result.report.unselected_editor_material_count, 1);
}

#[test]
fn effective_revision_ignores_editor_order_names_geometry_and_single_mip_filter_variants() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let first = apply(&reference, &sidecar, &edited, &Default::default()).unwrap();
    let second_input = rewrite(&edited, |j| {
        j["materials"].as_array_mut().unwrap().swap(0, 1);
        j["materials"][0]["name"] = json!("another name");
        j["meshes"] = json!([]);
        j["nodes"] = json!([]);
        j["samplers"][0]["minFilter"] = json!(9985);
    });
    let second = apply(&reference, &sidecar, &second_input, &Default::default()).unwrap();
    assert_eq!(first.glb, second.glb);
    assert_eq!(first.report.bake_revision, second.report.bake_revision);
    assert_ne!(first.report.edited_sha256, second.report.edited_sha256);
    let changed = rewrite(&edited, |j| {
        j["materials"][1]["pbrMetallicRoughness"]["roughnessFactor"] = json!(0.43)
    });
    assert_ne!(
        first.report.bake_revision,
        apply(&reference, &sidecar, &changed, &Default::default())
            .unwrap()
            .report
            .bake_revision
    );
}

#[test]
fn exact_selected_resource_and_output_report_boundaries_apply() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let (j, _) = unpack(&edited);
    let image_length = {
        let view = j["images"][0]["bufferView"].as_u64().unwrap() as usize;
        j["bufferViews"][view]["byteLength"].as_u64().unwrap() as usize
    };
    let resource_count = j
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_array)
        .map(Vec::len)
        .max()
        .unwrap();
    let first = apply(&reference, &sidecar, &edited, &Default::default()).unwrap();
    let report_length = serde_json::to_vec_pretty(&first.report).unwrap().len() + 1;
    let mut limits = MaterialEditLimits {
        max_edited_glb_bytes: edited.len(),
        max_edited_json_bytes: gltf::binary::Glb::from_slice(&edited).unwrap().json.len(),
        max_resources: resource_count,
        max_single_png_bytes: image_length,
        max_single_decoded_png_bytes: 8,
        max_selected_png_bytes: image_length,
        max_selected_decoded_png_bytes: 8,
        max_dimension: 2,
        max_output_glb_bytes: first.glb.len(),
        max_report_bytes: report_length,
        ..Default::default()
    };
    assert!(apply(&reference, &sidecar, &edited, &limits).is_ok());
    for field in 0..11 {
        let mut small = limits.clone();
        match field {
            0 => small.max_edited_glb_bytes -= 1,
            1 => small.max_edited_json_bytes -= 1,
            2 => small.max_resources -= 1,
            3 => small.max_single_png_bytes -= 1,
            4 => small.max_single_decoded_png_bytes -= 1,
            5 => small.max_selected_png_bytes -= 1,
            6 => small.max_selected_decoded_png_bytes -= 1,
            7 => small.max_dimension -= 1,
            8 => small.max_output_glb_bytes -= 1,
            9 => small.max_report_bytes -= 1,
            _ => small.reference.max_glb_bytes = reference.len() - 1,
        };
        assert!(
            apply(&reference, &sidecar, &edited, &small).is_err(),
            "limit case {field}"
        );
    }
    limits.max_resources = 0;
    assert!(apply(&reference, &sidecar, &edited, &limits).is_err());
}

#[test]
fn selected_image_ordinals_charge_before_content_dedup_and_output_dedups_payload() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let edited = rewrite(&edited, |j| {
        let image = j["images"][0].clone();
        j["images"].as_array_mut().unwrap().push(image);
        j["textures"]
            .as_array_mut()
            .unwrap()
            .push(json!({"source":1}));
        j["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index":1});
    });
    let ids = vec!["material-000001".into(), "material-000000".into()];
    let mut limits = MaterialEditLimits {
        max_selected_decoded_png_bytes: 15,
        ..Default::default()
    };
    assert!(reimport_material_edits(&reference, &sidecar, &edited, &ids, &limits).is_err());
    limits.max_selected_decoded_png_bytes = 16;
    let output = reimport_material_edits(&reference, &sidecar, &edited, &ids, &limits).unwrap();
    let (j, _) = unpack(&output.glb);
    assert_eq!(j["images"].as_array().unwrap().len(), 2);
    assert_eq!(
        j["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"],
        j["materials"][1]["pbrMetallicRoughness"]["baseColorTexture"]
    );
}

#[test]
fn lowered_editor_resource_limit_does_not_restrict_retained_original_views() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let (mut j, bin) = unpack(&edited);
    let old = j["images"][0]["bufferView"].as_u64().unwrap() as usize;
    let view = j["bufferViews"][old].clone();
    let offset = view["byteOffset"].as_u64().unwrap() as usize;
    let len = view["byteLength"].as_u64().unwrap() as usize;
    let image = bin[offset..offset + len].to_vec();
    j["bufferViews"] = json!([{"buffer":0,"byteOffset":0,"byteLength":len}]);
    j["images"][0]["bufferView"] = json!(0);
    j["buffers"][0]["byteLength"] = json!(len);
    for key in ["meshes", "nodes", "accessors"] {
        j[key] = json!([])
    }
    let editor = gltf::binary::Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: 0,
        },
        json: std::borrow::Cow::Owned(serde_json::to_vec(&j).unwrap()),
        bin: Some(std::borrow::Cow::Owned(image)),
    }
    .to_vec()
    .unwrap();
    let limits = MaterialEditLimits {
        max_resources: 2,
        ..Default::default()
    };
    let result = apply(&reference, &sidecar, &editor, &limits).unwrap();
    let (j, _) = unpack(&result.glb);
    assert!(j["bufferViews"].as_array().unwrap().len() > 2);
}

#[test]
fn shared_selected_image_ordinal_charges_once_and_shares_output_texture() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let ids = vec!["material-000001".into(), "material-000000".into()];
    let limits = MaterialEditLimits {
        max_selected_decoded_png_bytes: 8,
        ..Default::default()
    };
    let output = reimport_material_edits(&reference, &sidecar, &edited, &ids, &limits).unwrap();
    let (j, _) = unpack(&output.glb);
    assert_eq!(j["images"].as_array().unwrap().len(), 2);
    assert_eq!(
        j["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"],
        j["materials"][1]["pbrMetallicRoughness"]["baseColorTexture"]
    );
}

#[test]
fn rgba_integer_and_float_lanes_have_the_same_effective_revision() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let integer = rewrite(&edited, |j| {
        j["materials"][1]["pbrMetallicRoughness"]["baseColorFactor"] = json!([1, 1, 1, 1]);
    });
    let float = rewrite(&edited, |j| {
        j["materials"][1]["pbrMetallicRoughness"]["baseColorFactor"] = json!([1.0, 1.0, 1.0, 1.0]);
    });
    let a = apply(&reference, &sidecar, &integer, &Default::default()).unwrap();
    let b = apply(&reference, &sidecar, &float, &Default::default()).unwrap();
    assert_eq!(a.report.bake_revision, b.report.bake_revision);
    assert_eq!(a.glb, b.glb);
    assert_ne!(a.report.edited_sha256, b.report.edited_sha256);
}

#[allow(dead_code)]
mod png_fixture {
    include!("support/png_stream.rs");
}

fn editor_with_png(edited: &[u8], png: &[u8]) -> Vec<u8> {
    let (mut root, mut binary) = unpack(edited);
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    let start = binary.len();
    binary.extend_from_slice(png);
    let view = root["bufferViews"].as_array().unwrap().len();
    root["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"buffer":0,"byteOffset":start,"byteLength":png.len()}));
    root["images"][0] = json!({"bufferView":view,"mimeType":"image/png"});
    root["buffers"][0]["byteLength"] = json!(binary.len());
    gltf::binary::Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: 0,
        },
        json: std::borrow::Cow::Owned(serde_json::to_vec(&root).unwrap()),
        bin: Some(std::borrow::Cow::Owned(binary)),
    }
    .to_vec()
    .unwrap()
}

#[test]
fn png_extra_inflated_scanline_bytes_refuse_without_replacing_image_meaning() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let mut raw = vec![0; 18];
    raw.extend_from_slice(b"JUNK");
    let image = png_fixture::png(2, 2, 8, 6, 0, &raw, &[]);
    assert!(
        image::load_from_memory(&image).is_ok(),
        "discriminates permissive pixel decoding"
    );
    let edited = editor_with_png(&edited, &image);
    assert!(apply(&reference, &sidecar, &edited, &Default::default()).is_err());
}

#[test]
fn png_legal_after_zlib_compressed_padding_remains_accepted() {
    let reference = reference();
    let (edited, sidecar) = editor(&reference, [200, 17, 41, 91]);
    let image = png_fixture::png(2, 2, 8, 6, 0, &[0; 18], &[0, 0, 0, 0]);
    let edited = editor_with_png(&edited, &image);
    let output = apply(&reference, &sidecar, &edited, &Default::default()).unwrap();
    eqoxide_assets::decode_static_visual(&output.glb, &Default::default()).unwrap();
}

#[test]
fn reference_export_refuses_extra_inflated_png_bytes_before_edit_admission() {
    let mut raw = vec![0; 18];
    raw.extend_from_slice(b"JUNK");
    let png = png_fixture::png(2, 2, 8, 6, 0, &raw, &[]);
    let malformed_reference = editor_with_png(&reference(), &png);
    assert!(
        eqoxide_assets::decode_static_visual(&malformed_reference, &Default::default()).is_ok()
    );
    assert!(
        eqoxide_asset_server::editable_visual::export_editable_visual(
            &malformed_reference,
            &Default::default()
        )
        .is_err()
    );
}

#[test]
fn reference_export_preserves_legal_compressed_png_padding() {
    let png = png_fixture::png(2, 2, 8, 6, 0, &[0; 18], &[0; 4]);
    let reference = editor_with_png(&reference(), &png);
    let bundle = eqoxide_asset_server::editable_visual::export_editable_visual(
        &reference,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(bundle.files["images/image-000000.png"], png);
    assert_eq!(bundle.files["reference.glb"], reference);
}
