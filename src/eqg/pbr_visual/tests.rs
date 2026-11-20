use super::*;
use crate::{
    eqg::DescriptorSource,
    static_scene::{StaticAlphaMode, StaticSurfaceModel},
};
use libeq_pfs::PfsWriter;
use std::{io::Cursor, path::PathBuf};

// These fixtures author source bytes independently of the resolver's schema table.
const FAMILIES: &[(&str, &[(&str, u32)])] = &[
    (
        "Opaque_MPLFull2UV.fx",
        &[
            ("e_TextureCoverage0", 2),
            ("e_TextureDiffuse0", 2),
            ("e_TextureNormal0", 2),
            ("e_fShininess0", 0),
        ],
    ),
    (
        "Opaque_MPLBump.fx",
        &[
            ("e_TextureCoverage0", 2),
            ("e_TextureDiffuse0", 2),
            ("e_TextureNormal0", 2),
            ("e_fCoverageScale0", 0),
        ],
    ),
    ("Opaque_MPLBasic.fx", &[("e_TextureDiffuse0", 2)]),
    (
        "Opaque_MPLSB.fx",
        &[
            ("e_TextureCoverage0", 2),
            ("e_TextureDiffuse0", 2),
            ("e_TextureNormal0", 2),
            ("e_fCoverageScale0", 0),
            ("e_fShininess0", 0),
        ],
    ),
    (
        "Opaque_MaxCB1.fx",
        &[("e_TextureDiffuse0", 2), ("e_TextureNormal0", 2)],
    ),
    (
        "Opaque_MaxCBSGE1.fx",
        &[
            ("e_TextureDiffuse0", 2),
            ("e_TextureEnvironment0", 2),
            ("e_TextureNormal0", 2),
            ("e_fEnvMapStrength0", 0),
            ("e_fShininess0", 0),
        ],
    ),
    ("Chroma_MaxC1.fx", &[("e_TextureDiffuse0", 2)]),
    ("Opaque_MaxC1.fx", &[("e_TextureDiffuse0", 2)]),
    (
        "Chroma_MaxCB1.fx",
        &[("e_TextureDiffuse0", 2), ("e_TextureNormal0", 2)],
    ),
    (
        "Opaque_MaxCBSG1.fx",
        &[
            ("e_TextureDiffuse0", 2),
            ("e_TextureNormal0", 2),
            ("e_fShininess0", 0),
        ],
    ),
    (
        "Opaque_MaxCBS1.fx",
        &[
            ("e_TextureDiffuse0", 2),
            ("e_TextureNormal0", 2),
            ("e_fShininess0", 0),
        ],
    ),
    (
        "Opaque_MaxWater.fx",
        &[
            ("e_TextureDiffuse0", 2),
            ("e_TextureNormal0", 2),
            ("e_fFresnelBias", 0),
            ("e_fFresnelPower", 0),
            ("e_fReflectionAmount", 0),
            ("e_fReflectionColor", 3),
            ("e_fSlide1X", 0),
            ("e_fSlide1Y", 0),
            ("e_fSlide2X", 0),
            ("e_fSlide2Y", 0),
            ("e_fWaterColor1", 3),
            ("e_fWaterColor2", 3),
        ],
    ),
];
fn word(b: &mut Vec<u8>, n: u32) {
    b.extend(n.to_le_bytes());
}
fn string(b: &mut Vec<u8>, s: &str) -> u32 {
    let o = b.len() as u32;
    b.extend(s.as_bytes());
    b.push(0);
    o
}
fn png() -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image::RgbaImage::from_raw(
        2,
        2,
        vec![
            255, 0, 0, 191, 0, 255, 0, 192, 0, 0, 255, 193, 17, 33, 65, 255,
        ],
    )
    .unwrap()
    .write_to(&mut out, image::ImageFormat::Png)
    .unwrap();
    out.into_inner()
}
fn model(shader: &str, props: &[(&str, u32)], terrain: bool) -> Vec<u8> {
    let mut strings = Vec::new();
    let name = string(&mut strings, "material");
    let sh = string(&mut strings, shader);
    let texture = string(&mut strings, "Asymmetric.PNG");
    let properties: Vec<_> = props
        .iter()
        .map(|(n, k)| {
            (
                string(&mut strings, n),
                *k,
                if *k == 2 { texture } else { 0x7fc01234 },
            )
        })
        .collect();
    let mut b = if terrain {
        b"EQGT".to_vec()
    } else {
        b"EQGM".to_vec()
    };
    for n in [3, strings.len() as u32, 1, 3, 2] {
        word(&mut b, n)
    }
    if !terrain {
        word(&mut b, 0)
    }
    b.extend(strings);
    for n in [77, name, sh, properties.len() as u32] {
        word(&mut b, n)
    }
    for (n, k, v) in properties {
        for n in [n, k, v] {
            word(&mut b, n)
        }
    }
    for (i, p) in [[0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]]
        .iter()
        .enumerate()
    {
        for n in p.iter().chain([0., 0., 1.].iter()) {
            word(&mut b, n.to_bits())
        }
        word(&mut b, 0x01020304 + i as u32);
        for n in [[0f32, 0.], [1., 0.], [0., 1.]][i] {
            word(&mut b, n.to_bits())
        }
        word(&mut b, 0x7fc01234);
        word(&mut b, 0xff800000);
    }
    for mat in [0, u32::MAX] {
        for n in [0, 1, 2, mat, 3] {
            word(&mut b, n)
        }
    }
    b
}
fn descriptor() -> Vec<u8> {
    let mut strings = Vec::new();
    let ter = string(&mut strings, "terrain.ter");
    let obj = string(&mut strings, "object.mod");
    let nm = string(&mut strings, "instance");
    let mut b = b"EQGZ".to_vec();
    for n in [2, strings.len() as u32, 4, 5, 0, 0] {
        word(&mut b, n)
    }
    b.extend(strings);
    for n in [ter, obj, obj, u32::MAX] {
        word(&mut b, n)
    }
    for (i, slot) in [1, 1, 2, 0, u32::MAX].into_iter().enumerate() {
        for n in [slot, nm] {
            word(&mut b, n)
        }
        for f in [i as f32 * 5., 2., 3., 0.2, 0.3, 0.4, 1.5] {
            word(&mut b, f.to_bits())
        }
        word(&mut b, 3);
        for n in [0xdeadbeef, i as u32, 0xff001122] {
            word(&mut b, n)
        }
    }
    b
}
struct Fixture {
    _tmp: tempfile::TempDir,
    archive: PathBuf,
    descriptor: PathBuf,
}
impl Fixture {
    fn new(shader: &str, props: &[(&str, u32)]) -> Self {
        Self::members(vec![
            ("terrain.ter".into(), model(shader, props, true)),
            ("object.mod".into(), model(shader, props, false)),
            ("Asymmetric.PNG".into(), png()),
            ("zone.zon".into(), descriptor()),
        ])
    }
    fn members(members: Vec<(String, Vec<u8>)>) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("zone.eqg");
        let desc = tmp.path().join("zone.zon");
        let mut p = PfsWriter::create(std::fs::File::create(&archive).unwrap()).unwrap();
        for (n, b) in members {
            p.insert(n, Cursor::new(b)).unwrap();
        }
        p.finish().unwrap();
        std::fs::write(&desc, descriptor()).unwrap();
        Self {
            _tmp: tmp,
            archive,
            descriptor: desc,
        }
    }
    fn load(&self) -> anyhow::Result<ResolvedEqgPbrPreview> {
        load_eqg_pbr_preview(
            &self.archive,
            DescriptorSource::Loose(&self.descriptor),
            &EqgPbrLimits::default(),
        )
    }
}
#[test]
fn every_family_preserves_samples_source_and_repeated_placements() {
    for (shader, props) in FAMILIES {
        let f = Fixture::new(shader, props);
        let r = f.load().unwrap();
        assert_eq!(r.scene.meshes.len(), 2);
        assert_eq!(r.scene.instances.len(), 3);
        assert_eq!(r.scene.materials.len(), 2);
        assert_eq!(r.scene.textures.len(), 1);
        assert_eq!(r.preserved_resources.len(), 1);
        assert_eq!(r.preserved_resources[0].bytes, png());
        assert_eq!(
            r.preserved_resources[0].uses.len(),
            props.iter().filter(|(_, k)| *k == 2).count() * 2
        );
        assert_eq!(r.source.meshes[0].vertices[0].color, Some(0x01020304));
        assert_eq!(
            r.source.meshes[0].vertices[0].uv1.unwrap()[0].to_bits(),
            0x7fc01234
        );
        assert_eq!(
            r.source.placements[1].extension_words().next(),
            Some(0xdeadbeef)
        );
        let im = image::load_from_memory(&r.scene.textures[0].png_bytes)
            .unwrap()
            .to_rgba8();
        assert_eq!(
            im.as_raw(),
            &[255, 0, 0, 191, 0, 255, 0, 192, 0, 0, 255, 193, 17, 33, 65, 255]
        );
        for m in &r.scene.meshes {
            assert!(m.primitives.iter().all(|p| p.colors.is_none()));
            assert_eq!(m.uvs, [[0., 0.], [1., 0.], [0., 1.]]);
            assert_eq!(m.primitives[0].indices, [0, 2, 1]);
        }
        for m in &r.scene.materials {
            if *shader == "Opaque_MaxWater.fx" {
                assert_eq!(m.base_color, [1., 1., 1., 0.55]);
                assert_eq!(m.alpha_mode, StaticAlphaMode::Blend);
                assert!(m.double_sided);
            } else {
                assert_eq!(m.base_color, [1.; 4]);
                assert!(!m.double_sided);
                assert_eq!(
                    m.alpha_mode,
                    if shader.starts_with("Chroma_") {
                        StaticAlphaMode::Mask {
                            cutoff: 192. / 255.,
                        }
                    } else {
                        StaticAlphaMode::Opaque
                    }
                );
            }
        }
        assert_eq!(
            r.recommended_material_policy.default,
            StaticSurfaceModel::Pbr {
                metallic: 0.,
                roughness: 1.
            }
        );
        if *shader == "Opaque_MaxWater.fx" {
            assert_eq!(r.recommended_material_policy.overrides.len(), 2);
            for s in r.recommended_material_policy.overrides.values() {
                assert_eq!(
                    *s,
                    StaticSurfaceModel::Pbr {
                        metallic: 0.,
                        roughness: 0.2
                    }
                );
            }
        }
        let report = serde_json::to_value(&r.report).unwrap();
        assert_eq!(report["lighting"].as_array().unwrap().len(), 5);
        assert_eq!(report["materials"].as_array().unwrap().len(), 2);
    }
}
#[test]
fn mandatory_property_sets_refuse_missing_extra_duplicate_wrong_kind_and_unknown_shader() {
    for (sh, props) in FAMILIES {
        for i in 0..props.len() {
            let mut missing = props.to_vec();
            missing.remove(i);
            assert!(
                Fixture::new(sh, &missing).load().is_err(),
                "missing {sh}/{i}"
            );
            let mut wrong = props.to_vec();
            wrong[i].1 = if wrong[i].1 == 2 { 0 } else { 2 };
            assert!(Fixture::new(sh, &wrong).load().is_err(), "wrong {sh}/{i}");
            let mut dup = props.to_vec();
            dup.push(props[i]);
            assert!(Fixture::new(sh, &dup).load().is_err(), "duplicate {sh}/{i}");
        }
        let mut extra = props.to_vec();
        extra.push(("unknown", 0));
        assert!(Fixture::new(sh, &extra).load().is_err());
    }
    assert!(Fixture::new("Opaque_Unknown.fx", FAMILIES[2].1)
        .load()
        .is_err());
}
#[test]
fn archive_and_logical_and_report_budgets_refuse() {
    let f = Fixture::new(FAMILIES[0].0, FAMILIES[0].1);
    let defaults = EqgPbrLimits::default();
    macro_rules! reject {
        ($field:ident) => {{
            let mut limits = defaults.clone();
            limits.$field = 0;
            assert!(
                load_eqg_pbr_preview(&f.archive, DescriptorSource::Loose(&f.descriptor), &limits)
                    .is_err(),
                stringify!($field)
            );
        }};
    }
    reject!(max_archive_bytes);
    reject!(max_descriptor_bytes);
    reject!(max_member_bytes);
    reject!(max_archive_uncompressed_bytes);
    reject!(max_directory_bytes);
    reject!(max_archive_members);
    reject!(max_name_bytes);
    reject!(max_total_name_bytes);
    reject!(max_preserved_resource_bytes);
    reject!(max_source_records);
    reject!(max_report_records);
    reject!(max_report_json_bytes);
    reject!(max_normalized_png_bytes);
    reject!(max_decoded_pixel_bytes);
    let mut invalid = defaults;
    invalid.max_archive_bytes = usize::MAX;
    assert!(
        load_eqg_pbr_preview(&f.archive, DescriptorSource::Loose(&f.descriptor), &invalid).is_err()
    );
}
#[test]
fn archive_selector_identity_determinism_and_content_binding() {
    let f = Fixture::new(FAMILIES[0].0, FAMILIES[0].1);
    let a = f.load().unwrap();
    let b = f.load().unwrap();
    assert_eq!(a.report.conversion_revision, b.report.conversion_revision);
    let member = load_eqg_pbr_preview(
        &f.archive,
        DescriptorSource::Archive("ZONE.ZON"),
        &EqgPbrLimits::default(),
    )
    .unwrap();
    assert_ne!(
        a.report.conversion_revision,
        member.report.conversion_revision
    );
    let other = f._tmp.path().join("renamed.zon");
    std::fs::copy(&f.descriptor, &other).unwrap();
    let renamed = load_eqg_pbr_preview(
        &f.archive,
        DescriptorSource::Loose(&other),
        &EqgPbrLimits::default(),
    )
    .unwrap();
    assert_eq!(
        a.report.conversion_revision,
        renamed.report.conversion_revision
    );
}
#[test]
fn all_members_preflight_and_missing_or_malformed_resources_refuse() {
    let sh = FAMILIES[0];
    for resources in [
        vec![],
        vec![("Asymmetric.PNG".into(), b"bad".to_vec())],
        vec![
            ("Asymmetric.PNG".into(), png()),
            ("asymmetric.png".into(), png()),
        ],
    ] {
        let mut m = vec![
            ("terrain.ter".into(), model(sh.0, sh.1, true)),
            ("object.mod".into(), model(sh.0, sh.1, false)),
        ];
        m.extend(resources);
        assert!(Fixture::members(m).load().is_err());
    }
    let mut m = vec![
        ("terrain.ter".into(), model(sh.0, sh.1, true)),
        ("object.mod".into(), model(sh.0, sh.1, false)),
        ("Asymmetric.PNG".into(), png()),
    ];
    m.push(("unused".into(), vec![0; 8192]));
    let f = Fixture::members(m);
    let l = EqgPbrLimits {
        max_member_bytes: 4096,
        ..EqgPbrLimits::default()
    };
    assert!(load_eqg_pbr_preview(&f.archive, DescriptorSource::Loose(&f.descriptor), &l).is_err());
}
#[test]
fn observed_source_changes_are_refused_on_handle_and_reopened_path() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("input");
    std::fs::write(&p, b"first").unwrap();
    let mut s = archive::Snapshot::open(&p, 16).unwrap();
    std::fs::write(&p, b"other").unwrap();
    assert!(s.verify().is_err());
    std::fs::write(&p, b"first").unwrap();
    let mut s = archive::Snapshot::open(&p, 16).unwrap();
    let replacement = t.path().join("replacement");
    std::fs::write(&replacement, b"other").unwrap();
    std::fs::rename(&replacement, &p).unwrap();
    assert!(s.verify().is_err());
}
#[test]
fn auxiliary_images_are_validated_and_preserved_separately() {
    let (sh, props) = FAMILIES[0];
    let ter = model(sh, props, true);
    let obj = model(sh, props, false);
    // Replace only the normal property string value with another same-length resource.
    fn normal(mut b: Vec<u8>) -> Vec<u8> {
        let sl = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        let start = 24 + if &b[..4] == b"EQGM" { 4 } else { 0 };
        let strings = &b[start..start + sl];
        let target = strings
            .windows(15)
            .position(|w| w == b"Asymmetric.PNG\0")
            .unwrap();
        let old = (target as u32).to_le_bytes();
        let new_offset = sl as u32;
        let prop_start = start + sl + 16;
        let prop_index = 2;
        assert_eq!(
            &b[prop_start + prop_index * 12 + 8..prop_start + prop_index * 12 + 12],
            &old
        );
        b[prop_start + prop_index * 12 + 8..prop_start + prop_index * 12 + 12]
            .copy_from_slice(&new_offset.to_le_bytes());
        b.splice(start + sl..start + sl, b"Normal.PNG\0".iter().copied());
        b[8..12].copy_from_slice(&((sl + 11) as u32).to_le_bytes());
        b
    }
    for bytes in [b"bad".to_vec(), png()] {
        let f = Fixture::members(vec![
            ("terrain.ter".into(), normal(ter.clone())),
            ("object.mod".into(), normal(obj.clone())),
            ("Asymmetric.PNG".into(), png()),
            ("Normal.PNG".into(), bytes.clone()),
        ]);
        let r = f.load();
        if bytes == b"bad" {
            assert!(r.is_err());
        } else {
            let r = r.unwrap();
            assert_eq!(r.preserved_resources.len(), 2);
            assert_eq!(r.scene.textures.len(), 1);
            assert_eq!(r.report.budget.decoded_pixel_bytes, 64);
        }
    }
}
#[test]
fn strict_profile_refuses_blend_and_glb_has_editable_pbr_without_colors() {
    let f = Fixture::new(FAMILIES[11].0, FAMILIES[11].1);
    let r = f.load().unwrap();
    let output = f._tmp.path().join("water.glb");
    assert!(
        crate::static_scene::write_static_visual_with_material_policy(
            &r.scene,
            &r.recommended_material_policy,
            &"0".repeat(64),
            &output
        )
        .is_err()
    );
    crate::static_scene::write_static_visual_with_surface_profile(
        &r.scene,
        &r.recommended_material_policy,
        crate::static_scene::StaticMaterialProfile::ScalarPbrHdrBlendV1,
        &"0".repeat(64),
        &output,
    )
    .unwrap();
    let bytes = std::fs::read(output).unwrap();
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    for m in gltf.materials() {
        assert_eq!(m.alpha_mode(), gltf::material::AlphaMode::Blend);
        assert_eq!(m.pbr_metallic_roughness().roughness_factor(), 0.2);
        assert_eq!(m.pbr_metallic_roughness().base_color_factor()[3], 0.55);
    }
    for m in gltf.meshes() {
        for p in m.primitives() {
            assert!(p.get(&gltf::Semantic::Colors(0)).is_none());
        }
    }
}
fn replace_image(bytes: Vec<u8>) -> Fixture {
    let (sh, props) = FAMILIES[2];
    Fixture::members(vec![
        ("terrain.ter".into(), model(sh, props, true)),
        ("object.mod".into(), model(sh, props, false)),
        ("Asymmetric.PNG".into(), bytes),
    ])
}
#[test]
fn sixteen_bit_png_depth_and_dds_rgb32_row_orientation_are_preserved() {
    let samples: Vec<u16> = vec![
        65535, 0, 23, 49151, 0, 65535, 47, 49152, 0, 0, 65535, 49153, 1234, 5678, 9012, 65535,
    ];
    let im =
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(2, 2, samples.clone()).unwrap();
    let mut out = Cursor::new(Vec::new());
    im.write_to(&mut out, image::ImageFormat::Png).unwrap();
    let r = replace_image(out.into_inner()).load().unwrap();
    let decoded = image::load_from_memory(&r.scene.textures[0].png_bytes).unwrap();
    assert!(matches!(decoded, image::DynamicImage::ImageRgba16(_)));
    assert_eq!(decoded.to_rgba16().as_raw(), &samples);
    let mut d = b"DDS ".to_vec();
    for n in [124, 0x100f, 2, 2, 8, 0, 0] {
        word(&mut d, n)
    }
    for _ in 0..11 {
        word(&mut d, 0)
    }
    for n in [
        32, 0x41, 0, 32, 0xff, 0xff00, 0xff0000, 0xff000000, 0x1000, 0, 0, 0, 0,
    ] {
        word(&mut d, n)
    }
    assert_eq!(d.len(), 128);
    d.extend([
        255, 0, 0, 191, 0, 255, 0, 192, 0, 0, 255, 193, 17, 33, 65, 255,
    ]);
    let r = replace_image(d).load().unwrap();
    assert_eq!(
        image::load_from_memory(&r.scene.textures[0].png_bytes)
            .unwrap()
            .to_rgba8()
            .as_raw(),
        &[255, 0, 0, 191, 0, 255, 0, 192, 0, 0, 255, 193, 17, 33, 65, 255]
    );
}
#[test]
fn duplicate_declared_ids_keep_table_ordinals_and_unknown_unused_semantics_are_retained() {
    fn extra(mut b: Vec<u8>, unknown: bool) -> Vec<u8> {
        let sl = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        let start = 24 + if &b[..4] == b"EQGM" { 4 } else { 0 };
        let record = start + sl;
        let props = u32::from_le_bytes(b[record + 12..record + 16].try_into().unwrap()) as usize;
        let end = record + 16 + props * 12;
        let mut addition = if unknown {
            let mut a = Vec::new();
            for n in [77, 0, sl as u32, 0] {
                word(&mut a, n)
            }
            a
        } else {
            b[record..end].to_vec()
        };
        b.splice(end..end, addition.drain(..));
        b[12..16].copy_from_slice(&2u32.to_le_bytes());
        if unknown {
            let sh = b"UnknownUnused.fx\0";
            b.splice(record..record, sh.iter().copied());
            b[8..12].copy_from_slice(&((sl + sh.len()) as u32).to_le_bytes());
        } else {
            let n = b.len();
            b[n - 8..n - 4].copy_from_slice(&1u32.to_le_bytes());
        }
        b
    }
    for unknown in [false, true] {
        let (sh, props) = FAMILIES[2];
        let f = Fixture::members(vec![
            ("terrain.ter".into(), extra(model(sh, props, true), unknown)),
            ("object.mod".into(), extra(model(sh, props, false), unknown)),
            ("Asymmetric.PNG".into(), png()),
        ]);
        let r = f.load().unwrap();
        assert_eq!(r.report.materials.len(), 4);
        assert_eq!(r.scene.materials.len(), if unknown { 2 } else { 4 });
        for mi in 0..2 {
            assert_eq!(
                r.source.meshes[mi]
                    .materials
                    .iter()
                    .map(|m| m.index)
                    .collect::<Vec<_>>(),
                [77, 77]
            );
            let rows: Vec<_> = r
                .report
                .materials
                .iter()
                .filter(|m| m.source_mesh_index == mi)
                .collect();
            assert_eq!(rows[0].source_material_ordinal, 0);
            assert_eq!(rows[1].source_material_ordinal, 1);
            assert_eq!(rows[1].used, !unknown);
            if unknown {
                assert_eq!(rows[1].shader, "UnknownUnused.fx");
                assert_eq!(rows[1].coverage, "uninterpreted-unused");
            } else {
                assert_ne!(rows[0].output_material_index, rows[1].output_material_index);
            }
        }
    }
}
#[test]
fn malformed_descriptor_mesh_transform_suffix_and_unsafe_selected_names_refuse() {
    let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
    for bytes in [
        b"bad".to_vec(),
        {
            let mut b = descriptor();
            b.push(7);
            b
        },
        {
            let mut b = descriptor();
            let n = b.len();
            b[n - 20..n - 16].copy_from_slice(&0x7fc01234u32.to_le_bytes());
            b
        },
    ] {
        std::fs::write(&f.descriptor, bytes).unwrap();
        assert!(f.load().is_err());
    }
    let (sh, props) = FAMILIES[2];
    for mut ter in [
        b"bad".to_vec(),
        {
            let mut b = model(sh, props, true);
            b.push(7);
            b
        },
        {
            let mut b = model(sh, props, true);
            let p = b.windows(14).position(|b| b == b"Asymmetric.PNG").unwrap();
            b[p] = b'/';
            b
        },
    ] {
        let f = Fixture::members(vec![
            ("terrain.ter".into(), std::mem::take(&mut ter)),
            ("object.mod".into(), model(sh, props, false)),
            ("Asymmetric.PNG".into(), png()),
        ]);
        assert!(f.load().is_err());
    }
}

#[test]
fn world_bounds_use_indexed_vertices_and_actual_instances() {
    use crate::static_scene::{StaticInstance, StaticMesh, StaticPrimitive};
    let s = StaticScene {
        meshes: vec![StaticMesh {
            name: "bounds".into(),
            positions: vec![[1., 2., 3.], [4., 5., 6.], [-2., 0., 1.], [99999.; 3]],
            normals: vec![],
            uvs: vec![],
            primitives: vec![StaticPrimitive {
                indices: vec![0, 1, 2],
                material_index: 0,
                colors: None,
            }],
        }],
        instances: vec![StaticInstance {
            mesh_index: 0,
            matrix: glam::Mat4::from_translation(glam::Vec3::new(10., 20., 30.)).to_cols_array_2d(),
        }],
        ..StaticScene::default()
    };
    let b = report::indexed_bounds(&s).unwrap();
    assert_eq!(b.min, [8., 20., 31.]);
    assert_eq!(b.max, [14., 25., 36.]);
}
