use super::*;

// Change a single raw property value without consulting the production schema.
fn texture_name(mut bytes: Vec<u8>, property: &str, value: &str) -> Vec<u8> {
    let sl = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let start = if &bytes[..4] == b"EQGM" { 28 } else { 24 };
    let strings = &bytes[start..start + sl];
    let name_offset = strings
        .windows(property.len() + 1)
        .position(|w| w[..property.len()] == *property.as_bytes() && w[property.len()] == 0)
        .unwrap() as u32;
    let count =
        u32::from_le_bytes(bytes[start + sl + 12..start + sl + 16].try_into().unwrap()) as usize;
    let prop = (0..count)
        .map(|i| start + sl + 16 + i * 12)
        .find(|&i| bytes[i..i + 4] == name_offset.to_le_bytes())
        .unwrap();
    bytes[prop + 8..prop + 12].copy_from_slice(&(sl as u32).to_le_bytes());
    bytes.splice(start + sl..start + sl, value.bytes().chain([0]));
    bytes[8..12].copy_from_slice(&((sl + value.len() + 1) as u32).to_le_bytes());
    bytes
}

fn with_texture(
    shader: &str,
    props: &[(&str, u32)],
    property: &str,
    value: &str,
    extra: Option<Vec<u8>>,
) -> Fixture {
    let mut members = vec![
        (
            "terrain.ter".into(),
            texture_name(model(shader, props, true), property, value),
        ),
        (
            "object.mod".into(),
            texture_name(model(shader, props, false), property, value),
        ),
        ("Asymmetric.PNG".into(), png()),
        ("zone.zon".into(), descriptor()),
    ];
    if let Some(bytes) = extra {
        members.push((value.into(), bytes));
    }
    Fixture::members(members)
}

#[test]
fn exact_cb1_alias_bakes() {
    let f = Fixture::new(
        "Opaque_OpaqueSModelCB1Max.fx",
        &[("e_TextureDiffuse0", 2), ("e_TextureNormal0", 2)],
    );
    let r = f.load().unwrap();
    assert_eq!(r.scene.materials[0].alpha_mode, StaticAlphaMode::Opaque);
}

#[test]
fn exact_cbgg1_alias_bakes() {
    let f = Fixture::new(
        "Opaque_OpaqueSModelCBGG1Max.fx",
        &[
            ("e_TextureDiffuse0", 2),
            ("e_TextureNormal0", 2),
            ("e_fBumpiness0", 0),
            ("e_fShininess0", 0),
        ],
    );
    let r = f.load().unwrap();
    assert_eq!(r.scene.materials[0].base_color, [1.; 4]);
}

#[test]
fn exact_none_optional_normal_bakes_without_archive_image() {
    let (shader, props) = FAMILIES[4];
    for extra in [None, Some(b"bad image named None".to_vec())] {
        let f = with_texture(shader, props, "e_TextureNormal0", "None", extra);
        let r = f.load().unwrap();
        assert_eq!(r.preserved_resources.len(), 1);
        for row in &r.report.materials {
            assert!(matches!(
                row.normal_source_absence,
                Some(crate::eqg::pbr_visual::NormalSourceAbsence::ExactNone)
            ));
        }
        let report = serde_json::to_value(&r.report).unwrap();
        for row in report["materials"].as_array().unwrap() {
            assert_eq!(row["normal_source_absence"], "ExactNone");
            assert!(row["properties"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["name"] == "e_TextureNormal0")
                .unwrap()["resource_index"]
                .is_null());
            assert!(row["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str().unwrap().contains("source absence:")));
        }
    }
}

fn bake(f: &Fixture) -> (ResolvedEqgPbrPreview, Vec<u8>) {
    let resolved = f.load().unwrap();
    let output = f._tmp.path().join("profile.glb");
    crate::static_scene::write_static_visual_with_surface_profile(
        &resolved.scene,
        &resolved.recommended_material_policy,
        crate::static_scene::StaticMaterialProfile::ScalarPbrHdrBlendV1,
        &"0".repeat(64),
        &output,
    )
    .unwrap();
    (resolved, std::fs::read(output).unwrap())
}

#[test]
fn alias_glbs_equal_supported_controls_in_all_visual_bytes() {
    for (alias, control) in [(12, 4), (13, 9)] {
        let (r, bytes) = bake(&Fixture::new(FAMILIES[alias].0, FAMILIES[alias].1));
        let (_, reference) = bake(&Fixture::new(FAMILIES[control].0, FAMILIES[control].1));
        assert_eq!(
            bytes, reference,
            "all writer JSON and BIN visual content must match"
        );
        for row in &r.report.materials {
            assert_eq!(row.coverage, "OPAQUE");
            assert_eq!(row.base_color_factor, Some([1.; 4]));
            assert_eq!(row.double_sided, Some(false));
            assert_eq!(
                row.surface,
                Some(StaticSurfaceModel::Pbr {
                    metallic: 0.,
                    roughness: 1.
                })
            );
            assert!(!serde_json::to_value(row)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("normal_source_absence"));
        }
        let props = &r.source.meshes[0].materials[0].properties;
        for prop in props.iter().filter(|p| p.kind == 0) {
            assert_eq!(prop.value, 0x7fc01234);
        }
        #[cfg(feature = "editable-visual")]
        {
            let decoded =
                eqoxide_assets::decode_static_visual(&bytes, &Default::default()).unwrap();
            assert_eq!(
                decoded.render_profile(),
                eqoxide_assets::static_visual::StaticRenderProfile::PbrHdrBlendReferenceLight
            );
            assert_eq!(decoded.meshes.len(), 2);
            assert_eq!(decoded.instances.len(), 3);
            assert_eq!(decoded.materials.len(), 2);
            assert_eq!(decoded.images.len(), 1);
            assert_eq!(decoded.meshes[0].primitives[0].indices, [0, 2, 1]);
            assert!(decoded
                .meshes
                .iter()
                .flat_map(|m| &m.primitives)
                .all(|p| p.colors.is_none()));
            for material in &decoded.materials {
                assert_eq!(material.base_color, [1.; 4]);
                assert_eq!(
                    material.alpha_mode,
                    eqoxide_assets::static_visual::StaticAlphaMode::Opaque
                );
                assert!(!material.double_sided);
                assert_eq!(
                    material.surface,
                    eqoxide_assets::static_visual::StaticSurfaceModel::Pbr {
                        metallic: 0.,
                        roughness: 1.
                    }
                );
            }
            let pixels = image::load_from_memory(&decoded.images[0].png_bytes)
                .unwrap()
                .to_rgba8();
            assert_eq!(
                pixels.as_raw(),
                &[255, 0, 0, 191, 0, 255, 0, 192, 0, 0, 255, 193, 17, 33, 65, 255]
            );
        }
    }
}

#[test]
fn optional_normal_absence_preserves_source_and_existing_visual_content() {
    for index in [4, 8, 12, 13] {
        let (shader, props) = FAMILIES[index];
        let fixture = with_texture(shader, props, "e_TextureNormal0", "None", None);
        let archive_before = std::fs::read(&fixture.archive).unwrap();
        let descriptor_before = std::fs::read(&fixture.descriptor).unwrap();
        let original = super::super::super::load_binary_zone(
            &fixture.archive,
            DescriptorSource::Loose(&fixture.descriptor),
        )
        .unwrap();
        let (r, bytes) = bake(&fixture);
        let (_, reference) = bake(&Fixture::new(shader, props));
        assert_eq!(bytes, reference);
        assert_eq!(std::fs::read(&fixture.archive).unwrap(), archive_before);
        assert_eq!(
            std::fs::read(&fixture.descriptor).unwrap(),
            descriptor_before
        );
        for (before, after) in original.meshes.iter().zip(&r.source.meshes) {
            assert_eq!(before.string_table, after.string_table);
            assert_eq!(
                format!("{:?}", before.materials),
                format!("{:?}", after.materials)
            );
        }
        assert_eq!(r.report.resources.len(), 1);
        assert_eq!(r.report.resources[0].uses.len(), 2);
        assert!(r.report.resources[0]
            .uses
            .iter()
            .all(|u| u.role == TextureRole::Diffuse));
        assert_eq!(r.report.budget.preserved_resource_bytes, png().len());
        assert_eq!(r.report.budget.decoded_pixel_bytes, 32);
        if index == 8 {
            for material in &r.scene.materials {
                assert_eq!(
                    material.alpha_mode,
                    StaticAlphaMode::Mask {
                        cutoff: 192. / 255.
                    }
                );
            }
        }
        #[cfg(feature = "editable-visual")]
        eqoxide_assets::decode_static_visual(&bytes, &Default::default()).unwrap();
    }
}

#[test]
fn none_variants_remain_required_images_and_bad_images_refuse() {
    let (shader, props) = FAMILIES[4];
    for name in ["none", "NONE", "NoneExtra.PNG", "Normal.PNG"] {
        for image in [None, Some(b"bad image".to_vec()), Some(png())] {
            let valid = image.as_ref().is_some_and(|b| *b == png());
            let f = with_texture(shader, props, "e_TextureNormal0", name, image);
            let r = f.load();
            if valid {
                let r = r.unwrap();
                assert_eq!(r.preserved_resources.len(), 2);
                assert_eq!(r.preserved_resources[1].bytes, png());
                assert_eq!(r.preserved_resources[1].uses[0].requested_name, name);
                assert!(r
                    .report
                    .materials
                    .iter()
                    .all(|row| row.normal_source_absence.is_none()));
            } else {
                assert!(r.is_err(), "{name}");
            }
        }
    }
}

#[test]
fn none_is_not_absence_for_other_roles() {
    for (index, property) in [
        (4, "e_TextureDiffuse0"),
        (0, "e_TextureCoverage0"),
        (5, "e_TextureEnvironment0"),
    ] {
        let (shader, props) = FAMILIES[index];
        for image in [None, Some(b"bad image".to_vec()), Some(png())] {
            let valid = image.as_ref().is_some_and(|b| *b == png());
            let r = with_texture(shader, props, property, "None", image).load();
            assert_eq!(r.is_ok(), valid, "{property}: {r:?}");
            if let Ok(r) = r {
                assert!(r.preserved_resources.iter().any(|r| r.exact_name == "None"));
                assert!(r
                    .report
                    .materials
                    .iter()
                    .all(|r| r.normal_source_absence.is_none()));
            }
        }
    }
}

#[test]
fn unknown_alias_spellings_and_invalid_schemas_refuse_before_none_interpretation() {
    for shader in [
        "opaque_OpaqueSModelCB1Max.fx",
        "Opaque_OpaqueSModelCB1Max.fx.extra",
        "Opaque_OpaqueSModelCB2Max.fx",
        "Opaque_OpaqueSModelCBGG1Max.fx.extra",
    ] {
        let error = with_texture(shader, FAMILIES[12].1, "e_TextureNormal0", "None", None)
            .load()
            .unwrap_err();
        assert!(error.to_string().contains("unsupported required shader"));
    }
    for index in [12, 13] {
        let (shader, props) = FAMILIES[index];
        let mut wrong = props.to_vec();
        wrong[1].1 = 0;
        let error = Fixture::new(shader, &wrong).load().unwrap_err();
        assert!(error.to_string().contains("wrong-kind"));
        let mut duplicate = props.to_vec();
        duplicate[0] = props[1];
        let error = with_texture(shader, &duplicate, "e_TextureNormal0", "None", None)
            .load()
            .unwrap_err();
        assert!(error.to_string().contains("duplicate"));
    }
}

#[test]
fn absence_decision_reserves_report_work_and_obeys_final_json_cap() {
    let (shader, props) = FAMILIES[4];
    let f = with_texture(shader, props, "e_TextureNormal0", "None", None);
    let r = f.load().unwrap();
    let defaults = EqgPbrLimits::default();
    let base = source_budget(&r.source, &defaults).unwrap();
    let small_records = EqgPbrLimits {
        max_report_records: base.report_records + 4,
        ..defaults.clone()
    };
    assert!(load_eqg_pbr_preview(
        &f.archive,
        DescriptorSource::Loose(&f.descriptor),
        &small_records
    )
    .unwrap_err()
    .to_string()
    .contains("normal absence projection record"));
    let small_strings = EqgPbrLimits {
        max_total_name_bytes: base.report_string_bytes
            + "Asymmetric.PNG".len() * 4
            + 256
            + NORMAL_ABSENCE_DECISION.len()
            + 7,
        ..defaults.clone()
    };
    assert!(load_eqg_pbr_preview(
        &f.archive,
        DescriptorSource::Loose(&f.descriptor),
        &small_strings
    )
    .unwrap_err()
    .to_string()
    .contains("normal absence projection string"));
    let report_bytes = bounded_json(&r.report, usize::MAX).unwrap().len();
    for (cap, success) in [(report_bytes - 1, false), (report_bytes, true)] {
        let limits = EqgPbrLimits {
            max_report_json_bytes: cap,
            ..defaults.clone()
        };
        assert_eq!(
            load_eqg_pbr_preview(&f.archive, DescriptorSource::Loose(&f.descriptor), &limits)
                .is_ok(),
            success
        );
    }
    let records = EqgPbrLimits {
        max_source_records: r.report.budget.source_records - 1,
        ..defaults
    };
    assert!(
        load_eqg_pbr_preview(&f.archive, DescriptorSource::Loose(&f.descriptor), &records).is_err()
    );
}

#[test]
fn absence_does_not_bypass_geometry_normals_or_string_offsets() {
    let (shader, props) = FAMILIES[4];
    for corrupt_offset in [false, true] {
        let mut terrain = texture_name(model(shader, props, true), "e_TextureNormal0", "None");
        let sl = u32::from_le_bytes(terrain[8..12].try_into().unwrap()) as usize;
        let offset = if corrupt_offset {
            24 + sl + 16 + 12 + 8
        } else {
            24 + sl + 16 + props.len() * 12 + 12
        };
        terrain[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let f = Fixture::members(vec![
            ("terrain.ter".into(), terrain),
            ("object.mod".into(), model(shader, props, false)),
            ("Asymmetric.PNG".into(), png()),
        ]);
        assert!(f.load().is_err());
    }
}

#[test]
fn unknown_unused_none_stays_uninterpreted() {
    let (shader, props) = FAMILIES[4];
    let mut terrain = model(shader, props, true);
    let sl = u32::from_le_bytes(terrain[8..12].try_into().unwrap()) as usize;
    let mut added = b"Opaque_Unverified.fx\0None\0".to_vec();
    let none_offset = sl + "Opaque_Unverified.fx".len() + 1;
    terrain.splice(24 + sl..24 + sl, added.drain(..));
    let new_sl = sl + "Opaque_Unverified.fx".len() + 1 + 5;
    terrain[8..12].copy_from_slice(&(new_sl as u32).to_le_bytes());
    terrain[12..16].copy_from_slice(&2u32.to_le_bytes());
    let material_start = 24 + new_sl;
    let material_size = 16 + props.len() * 12;
    let mut unused = terrain[material_start..material_start + material_size].to_vec();
    unused[8..12].copy_from_slice(&(sl as u32).to_le_bytes());
    unused[16 + 12 + 8..16 + 12 + 12].copy_from_slice(&(none_offset as u32).to_le_bytes());
    terrain.splice(
        material_start + material_size..material_start + material_size,
        unused,
    );
    let f = Fixture::members(vec![
        ("terrain.ter".into(), terrain),
        ("object.mod".into(), model(shader, props, false)),
        ("Asymmetric.PNG".into(), png()),
    ]);
    let r = f.load().unwrap();
    let row = r.report.materials.iter().find(|r| !r.used).unwrap();
    assert_eq!(row.shader, "Opaque_Unverified.fx");
    assert_eq!(row.coverage, "uninterpreted-unused");
    assert!(row.normal_source_absence.is_none());
    assert!(row.decisions.is_empty());
    assert!(row.properties.iter().all(|p| p.resource_index.is_none()));
}
