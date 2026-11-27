mod source_profiles;

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
    (
        "Opaque_OpaqueSModelCB1Max.fx",
        &[("e_TextureDiffuse0", 2), ("e_TextureNormal0", 2)],
    ),
    (
        "Opaque_OpaqueSModelCBGG1Max.fx",
        &[
            ("e_TextureDiffuse0", 2),
            ("e_TextureNormal0", 2),
            ("e_fBumpiness0", 0),
            ("e_fShininess0", 0),
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
fn sole_object_descriptor(version: u32) -> Vec<u8> {
    let mut strings = Vec::new();
    let ter = string(&mut strings, "terrain.ter");
    let obj = string(&mut strings, "object.mod");
    let nm = string(&mut strings, "instance");
    let mut b = b"EQGZ".to_vec();
    for n in [version, strings.len() as u32, 3, 1, 0, 0] {
        word(&mut b, n);
    }
    b.extend(strings);
    for n in [ter, obj, obj, 2, nm] {
        word(&mut b, n);
    }
    for f in [0f32, 0., 0., 0., 0., 0., 1.] {
        word(&mut b, f.to_bits());
    }
    if version == 2 {
        word(&mut b, 0);
    }
    b
}
#[test]
fn inventory_selection_includes_v1_sole_placement_zero_but_excludes_v2_metadata() {
    let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
    for version in [1, 2] {
        std::fs::write(&f.descriptor, sole_object_descriptor(version)).unwrap();
        let source = load_binary_zone(&f.archive, DescriptorSource::Loose(&f.descriptor)).unwrap();
        let used = used_materials(&source).unwrap();
        assert_eq!(used.contains(&(0, 0)), true);
        assert_eq!(
            used.contains(&(1, 0)),
            version == 1,
            "descriptor v{version}"
        );
        assert_eq!(used.len(), if version == 1 { 2 } else { 1 });
    }
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

fn inventory(f: &Fixture) -> EqgPbrMaterialInventory {
    inspect_eqg_pbr_materials(
        &f.archive,
        DescriptorSource::Loose(&f.descriptor),
        &EqgPbrLimits::default(),
    )
    .unwrap()
}
fn null_shader(mut b: Vec<u8>) -> Vec<u8> {
    let sl = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    let start = if &b[..4] == b"EQGM" { 28 } else { 24 };
    b[start + sl + 8..start + sl + 12].copy_from_slice(&u32::MAX.to_le_bytes());
    b
}
#[test]
fn inventory_collects_all_required_gaps_and_preserves_null_unknown_and_typed_words() {
    let f = Fixture::members(vec![
        (
            "terrain.ter".into(),
            model("UnknownOne.fx", &[("raw", 3)], true),
        ),
        (
            "object.mod".into(),
            null_shader(model("UnknownTwo.fx", &[("raw", 0)], false)),
        ),
    ]);
    let r = inventory(&f);
    assert_eq!(r.readiness, "not-evaluated");
    assert_eq!(r.required_materials, 2);
    assert_eq!(r.unsupported_required_materials, 2);
    assert_eq!(r.materials.len(), 2);
    assert_eq!(r.materials[0].shader.as_deref(), Some("UnknownOne.fx"));
    assert_eq!(r.materials[1].shader, None);
    for row in &r.materials {
        assert!(row.required);
        assert!(matches!(
            row.schema,
            MaterialSchemaAssessment::Unsupported { .. }
        ));
        assert_eq!(row.source_declared_id, 77);
        assert_eq!(row.properties[0].value_word, 0x7fc01234);
        assert_eq!(row.properties[0].string_value, None);
    }
}
#[test]
fn inventory_unused_null_and_unknown_are_uninterpreted_and_repeated_slots_deduplicate() {
    for null in [false, true] {
        let mut obj = model("UnknownUnused.fx", &[("unknown", 2)], false);
        if null {
            obj = null_shader(obj);
        }
        let f = Fixture::members(vec![
            (
                "terrain.ter".into(),
                model(FAMILIES[2].0, FAMILIES[2].1, true),
            ),
            ("object.mod".into(), obj),
        ]);
        std::fs::write(&f.descriptor, sole_object_descriptor(2)).unwrap();
        let r = inventory(&f);
        assert_eq!(r.materials.len(), 2);
        assert_eq!(r.model_slots, [Some(0), Some(1), Some(1)]);
        assert_eq!(r.required_materials, 1);
        let row = &r.materials[1];
        assert!(!row.required);
        assert!(matches!(
            row.schema,
            MaterialSchemaAssessment::UninterpretedUnused
        ));
        assert!(row.properties[0].archive_lookup.is_none());
    }
}
#[test]
fn inventory_matching_schemas_and_member_presence_do_not_evaluate_readiness_or_decode() {
    let f = Fixture::members(vec![
        (
            "terrain.ter".into(),
            model(FAMILIES[2].0, FAMILIES[2].1, true),
        ),
        (
            "object.mod".into(),
            model(FAMILIES[2].0, FAMILIES[2].1, false),
        ),
        ("asymmetric.png".into(), b"not an image".to_vec()),
    ]);
    let r = inventory(&f);
    assert_eq!(r.unsupported_required_materials, 0);
    assert_eq!(r.required_materials, 2);
    assert_eq!(r.readiness, "not-evaluated");
    assert_eq!(r.materials.len(), 2);
    for m in &r.materials {
        assert!(matches!(m.schema, MaterialSchemaAssessment::Compatible));
        assert_eq!(
            m.properties[0].string_value.as_deref(),
            Some("Asymmetric.PNG")
        );
        assert!(
            matches!(&m.properties[0].archive_lookup, Some(ArchiveMemberLookup::Present { exact_name, advertised_bytes }) if exact_name == "asymmetric.png" && *advertised_bytes == 12)
        );
    }
}
#[test]
fn inventory_missing_and_unsafe_texture_names_are_facts_without_schema_short_circuit() {
    let (shader, props) = FAMILIES[2];
    for unsafe_name in [false, true] {
        let mut ter = model(shader, props, true);
        if unsafe_name {
            let i = ter
                .windows(14)
                .position(|b| b == b"Asymmetric.PNG")
                .unwrap();
            ter[i] = b'/';
        }
        let f = Fixture::members(vec![
            ("terrain.ter".into(), ter),
            ("object.mod".into(), model("Unknown.fx", props, false)),
        ]);
        let r = inventory(&f);
        assert_eq!(r.unsupported_required_materials, 1);
        let lookup = r.materials[0].properties[0]
            .archive_lookup
            .as_ref()
            .unwrap();
        assert!(if unsafe_name {
            matches!(lookup, ArchiveMemberLookup::UnsafeName)
        } else {
            matches!(lookup, ArchiveMemberLookup::Missing)
        });
        assert!(matches!(
            r.materials[1].properties[0].archive_lookup,
            Some(ArchiveMemberLookup::Missing)
        ));
    }
}
#[test]
fn inventory_reuses_exact_typed_set_policy_and_keeps_duplicate_properties() {
    let cases = [
        vec![],
        vec![("e_TextureDiffuse0", 0)],
        vec![("unknown", 2)],
        vec![("e_TextureDiffuse0", 2), ("e_TextureDiffuse0", 2)],
    ];
    for props in cases {
        let r = inventory(&Fixture::new(FAMILIES[2].0, &props));
        assert_eq!(r.unsupported_required_materials, 2);
        for m in r.materials {
            assert_eq!(m.properties.len(), props.len());
        }
    }
}
#[test]
fn inventory_limits_and_case_ambiguity_refuse() {
    let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
    macro_rules! refuse {
        ($field:ident) => {{
            let mut l = EqgPbrLimits::default();
            l.$field = 0;
            assert!(
                inspect_eqg_pbr_materials(&f.archive, DescriptorSource::Loose(&f.descriptor), &l)
                    .is_err(),
                stringify!($field)
            );
        }};
    }
    refuse!(max_archive_bytes);
    refuse!(max_descriptor_bytes);
    refuse!(max_member_bytes);
    refuse!(max_archive_uncompressed_bytes);
    refuse!(max_directory_bytes);
    refuse!(max_archive_members);
    refuse!(max_name_bytes);
    refuse!(max_total_name_bytes);
    refuse!(max_source_records);
    refuse!(max_report_records);
    refuse!(max_report_json_bytes);
    let f = Fixture::members(vec![
        (
            "terrain.ter".into(),
            model(FAMILIES[2].0, FAMILIES[2].1, true),
        ),
        (
            "object.mod".into(),
            model(FAMILIES[2].0, FAMILIES[2].1, false),
        ),
        ("Asymmetric.PNG".into(), png()),
        ("asymmetric.png".into(), png()),
    ]);
    assert!(inspect_eqg_pbr_materials(
        &f.archive,
        DescriptorSource::Loose(&f.descriptor),
        &EqgPbrLimits::default()
    )
    .is_err());
}
#[test]
fn inventory_identity_is_deterministic_path_independent_and_content_bound() {
    let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
    let a = inventory(&f);
    assert_eq!(a.inventory_revision, inventory(&f).inventory_revision);
    let p = f._tmp.path().join("renamed.eqg");
    let d = f._tmp.path().join("renamed.zon");
    std::fs::copy(&f.archive, &p).unwrap();
    std::fs::copy(&f.descriptor, &d).unwrap();
    let b = inspect_eqg_pbr_materials(&p, DescriptorSource::Loose(&d), &EqgPbrLimits::default())
        .unwrap();
    assert_eq!(a.inventory_revision, b.inventory_revision);
    let archive = inspect_eqg_pbr_materials(
        &p,
        DescriptorSource::Archive("ZONE.ZON"),
        &EqgPbrLimits::default(),
    )
    .unwrap();
    assert_ne!(a.inventory_revision, archive.inventory_revision);
    std::fs::write(&d, sole_object_descriptor(1)).unwrap();
    let c = inspect_eqg_pbr_materials(&p, DescriptorSource::Loose(&d), &EqgPbrLimits::default())
        .unwrap();
    assert_ne!(a.inventory_revision, c.inventory_revision);
    assert_eq!(c.required_materials, 2);
    assert!(
        load_eqg_pbr_preview(&p, DescriptorSource::Loose(&d), &EqgPbrLimits::default()).is_err()
    );
    let public =
        String::from_utf8(bounded_json(&c, EqgPbrLimits::default().max_report_json_bytes).unwrap())
            .unwrap();
    assert!(!public.contains(&f._tmp.path().display().to_string()));
}

#[test]
fn inventory_material_keys_are_mesh_and_ordinal_even_with_duplicate_declared_ids() {
    fn two_materials(mut b: Vec<u8>) -> Vec<u8> {
        let sl = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        let start = if &b[..4] == b"EQGM" { 28 } else { 24 };
        let record = start + sl;
        let addition = b[record..record + 28].to_vec();
        b.splice(record + 28..record + 28, addition);
        b[12..16].copy_from_slice(&2u32.to_le_bytes());
        let n = b.len();
        b[n - 8..n - 4].copy_from_slice(&1u32.to_le_bytes());
        b
    }
    let f = Fixture::members(vec![
        (
            "terrain.ter".into(),
            two_materials(model(FAMILIES[2].0, FAMILIES[2].1, true)),
        ),
        (
            "object.mod".into(),
            two_materials(model(FAMILIES[2].0, FAMILIES[2].1, false)),
        ),
    ]);
    let r = inventory(&f);
    assert_eq!(r.required_materials, 4);
    assert_eq!(r.materials.len(), 4);
    let keys: Vec<_> = r
        .materials
        .iter()
        .map(|m| {
            assert_eq!(m.source_declared_id, 77);
            assert!(m.required);
            (m.source_mesh_index, m.source_material_ordinal)
        })
        .collect();
    assert_eq!(keys, [(0, 0), (0, 1), (1, 0), (1, 1)]);
}
#[test]
fn inventory_model_and_archive_content_hashes_change_with_raw_property_word() {
    let f = Fixture::new(FAMILIES[0].0, FAMILIES[0].1);
    let a = inventory(&f);
    let mut changed = model(FAMILIES[0].0, FAMILIES[0].1, true);
    let sl = u32::from_le_bytes(changed[8..12].try_into().unwrap()) as usize;
    let value = 24 + sl + 16 + 3 * 12 + 8;
    changed[value..value + 4].copy_from_slice(&0xff800000u32.to_le_bytes());
    let g = Fixture::members(vec![
        ("terrain.ter".into(), changed),
        (
            "object.mod".into(),
            model(FAMILIES[0].0, FAMILIES[0].1, false),
        ),
        ("Asymmetric.PNG".into(), png()),
        ("zone.zon".into(), descriptor()),
    ]);
    let b = inventory(&g);
    assert_ne!(a.inventory_revision, b.inventory_revision);
    assert_ne!(a.archive_blake3, b.archive_blake3);
    assert_ne!(a.models[0].blake3, b.models[0].blake3);
    assert_eq!(a.models[1].blake3, b.models[1].blake3);
    assert_eq!(b.materials[0].properties[3].value_word, 0xff800000);
}

#[test]
fn inventory_final_json_bound_includes_revision_and_ignores_inapplicable_visual_budgets() {
    let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
    let mut l = EqgPbrLimits {
        max_preserved_resource_bytes: 0,
        max_normalized_png_bytes: 0,
        max_decoded_pixel_bytes: 0,
        max_glb_bytes: 0,
        ..EqgPbrLimits::default()
    };
    let r =
        inspect_eqg_pbr_materials(&f.archive, DescriptorSource::Loose(&f.descriptor), &l).unwrap();
    let size = bounded_json(&r, l.max_report_json_bytes).unwrap().len();
    assert_eq!(r.budget.preserved_resource_bytes, 0);
    assert_eq!(r.budget.decoded_pixel_bytes, 0);
    l.max_report_json_bytes = size;
    assert!(
        inspect_eqg_pbr_materials(&f.archive, DescriptorSource::Loose(&f.descriptor), &l).is_ok()
    );
    l.max_report_json_bytes = size - 1;
    assert!(
        inspect_eqg_pbr_materials(&f.archive, DescriptorSource::Loose(&f.descriptor), &l).is_err()
    );
}

#[test]
fn inventory_refuses_multiple_distinct_terrain_meshes_before_assigning_required_facts() {
    let mut desc = sole_object_descriptor(2);
    let pos = desc.windows(10).position(|b| b == b"object.mod").unwrap();
    desc[pos..pos + 10].copy_from_slice(b"second.ter");
    let f = Fixture::members(vec![
        (
            "terrain.ter".into(),
            model(FAMILIES[2].0, FAMILIES[2].1, true),
        ),
        (
            "second.ter".into(),
            model("OtherTerrain.fx", FAMILIES[2].1, true),
        ),
    ]);
    std::fs::write(&f.descriptor, desc).unwrap();
    let error = inspect_eqg_pbr_materials(
        &f.archive,
        DescriptorSource::Loose(&f.descriptor),
        &EqgPbrLimits::default(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("exactly one terrain mesh"),
        "{error:#}"
    );
}
#[test]
fn inventory_repeated_model_slots_for_one_terrain_are_unambiguous() {
    let f = Fixture::members(vec![(
        "terrain.ter".into(),
        model(FAMILIES[2].0, FAMILIES[2].1, true),
    )]);
    let mut strings = Vec::new();
    let terrain = string(&mut strings, "terrain.ter");
    let mut descriptor = b"EQGZ".to_vec();
    for n in [2, strings.len() as u32, 3, 0, 0, 0] {
        word(&mut descriptor, n);
    }
    descriptor.extend(strings);
    for _ in 0..3 {
        word(&mut descriptor, terrain);
    }
    std::fs::write(&f.descriptor, descriptor).unwrap();
    let r = inventory(&f);
    assert_eq!(r.model_slots, [Some(0), Some(0), Some(0)]);
    assert_eq!(r.models.len(), 1);
    assert_eq!(r.required_materials, 1);
}
#[test]
fn inventory_unselected_triangle_material_references_are_uninterpreted_until_required() {
    let mut obj = model("UnknownUnused.fx", FAMILIES[2].1, false);
    let n = obj.len();
    obj[n - 28..n - 24].copy_from_slice(&17u32.to_le_bytes());
    let f = Fixture::members(vec![
        (
            "terrain.ter".into(),
            model(FAMILIES[2].0, FAMILIES[2].1, true),
        ),
        ("object.mod".into(), obj),
    ]);
    std::fs::write(&f.descriptor, sole_object_descriptor(2)).unwrap();
    let r = inventory(&f);
    assert_eq!(r.required_materials, 1);
    assert!(!r.materials[1].required);
    assert!(matches!(
        r.materials[1].schema,
        MaterialSchemaAssessment::UninterpretedUnused
    ));
    std::fs::write(&f.descriptor, sole_object_descriptor(1)).unwrap();
    let error = inspect_eqg_pbr_materials(
        &f.archive,
        DescriptorSource::Loose(&f.descriptor),
        &EqgPbrLimits::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("invalid required material reference"),
        "{error:#}"
    );
}

#[test]
fn selected_source_assembly_reads_each_exact_model_once_in_slot_order() {
    let terrain = model(FAMILIES[2].0, FAMILIES[2].1, true);
    let object = model("UnknownObject.fx", FAMILIES[2].1, false);
    struct CountingMembers {
        terrain: Vec<u8>,
        object: Vec<u8>,
        reads: Vec<String>,
    }
    impl crate::eqg::zon::ZoneMembers for CountingMembers {
        fn exact(&self, requested: &str) -> Result<String> {
            Ok(match requested.to_ascii_lowercase().as_str() {
                "terrain.ter" => "TERRAIN.TER".into(),
                "object.mod" => "Object.MOD".into(),
                _ => panic!("unexpected request"),
            })
        }
        fn read(&mut self, exact: &str) -> Result<Vec<u8>> {
            assert!(
                !self.reads.contains(&exact.to_owned()),
                "duplicate model read"
            );
            self.reads.push(exact.to_owned());
            Ok(if exact == "TERRAIN.TER" {
                self.terrain.clone()
            } else {
                self.object.clone()
            })
        }
    }
    let mut members = CountingMembers {
        terrain,
        object,
        reads: Vec::new(),
    };
    let mut desc = descriptor();
    let strings = u32::from_le_bytes(desc[8..12].try_into().unwrap()) as usize;
    let alias = b"OBJECT.MOD\0";
    desc.splice(28 + strings..28 + strings, alias.iter().copied());
    let enlarged = strings + alias.len();
    desc[8..12].copy_from_slice(&(enlarged as u32).to_le_bytes());
    desc[28 + enlarged + 8..28 + enlarged + 12].copy_from_slice(&(strings as u32).to_le_bytes());
    let source = crate::eqg::zon::assemble_binary_zone(
        Path::new("unopened.eqg"),
        "selected.zon".into(),
        "archive",
        &desc,
        &mut members,
    )
    .unwrap();
    assert_eq!(members.reads, ["TERRAIN.TER", "Object.MOD"]);
    assert_eq!(source.model_slots, [Some(0), Some(1), Some(1), None]);
    assert_eq!(source.meshes[0].source_name, "TERRAIN.TER");
    assert_eq!(source.meshes[1].source_name, "Object.MOD");
    assert_eq!(source.descriptor_name, "selected.zon");
}

#[test]
fn selected_source_assembly_retains_handles_and_bytes_but_refuses_replaced_paths() {
    for archived_descriptor in [false, true] {
        let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
        let l = EqgPbrLimits::default();
        let mut archive = archive::Archive::open(&f.archive, &l).unwrap();
        let selector = if archived_descriptor {
            DescriptorSource::Archive("ZONE.ZON")
        } else {
            DescriptorSource::Loose(&f.descriptor)
        };
        let (bytes, _, mut loose, selected) = read_descriptor(&mut archive, selector, &l).unwrap();
        let descriptor_read_calls = archive.read_calls;
        let replacement = Fixture::new("Replacement.fx", FAMILIES[2].1);
        std::fs::rename(&replacement.archive, &f.archive).unwrap();
        std::fs::rename(&replacement.descriptor, &f.descriptor).unwrap();
        std::fs::write(&f.descriptor, b"replaced descriptor").unwrap();
        let (source, identities) = inventory::assemble_source(
            &f.archive,
            selector,
            &bytes,
            selected.as_deref(),
            &mut archive,
            &l,
        )
        .unwrap();
        assert_eq!(source.model_slots, [Some(0), Some(1), Some(1), None]);
        assert_eq!(identities.len(), 2);
        assert_eq!(
            archive.read_calls - descriptor_read_calls,
            2,
            "each distinct source model must be read once, including its hash"
        );
        for (i, terrain) in [true, false].into_iter().enumerate() {
            assert_eq!(identities[i].mesh_index, i);
            assert_eq!(identities[i].exact_name, source.meshes[i].source_name);
            assert_eq!(
                identities[i].blake3,
                hash(&model(FAMILIES[2].0, FAMILIES[2].1, terrain))
            );
        }
        assert!(format!("{:#}", archive.snapshot.verify().unwrap_err()).contains("pathname"));
        if let Some(snapshot) = &mut loose {
            assert!(format!("{:#}", snapshot.verify().unwrap_err()).contains("pathname"));
        }
    }
}

#[test]
fn selected_source_assembly_applies_member_read_cap() {
    let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
    let mut l = EqgPbrLimits::default();
    let mut archive = archive::Archive::open(&f.archive, &l).unwrap();
    let selector = DescriptorSource::Loose(&f.descriptor);
    let (bytes, _, _, selected) = read_descriptor(&mut archive, selector, &l).unwrap();
    l.max_member_bytes = 1;
    let error = inventory::assemble_source(
        &f.archive,
        selector,
        &bytes,
        selected.as_deref(),
        &mut archive,
        &l,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("selected member byte limit exceeded"));
}

#[test]
fn inventory_bad_string_offsets_are_structural_even_for_unused_unknown_rows() {
    for required in [false, true] {
        for field in [8usize, 16, 24] {
            let mut object = model("Unknown.fx", FAMILIES[2].1, false);
            let strings = u32::from_le_bytes(object[8..12].try_into().unwrap()) as usize;
            let offset = 28 + strings + field;
            object[offset..offset + 4].copy_from_slice(&0xffff_fffeu32.to_le_bytes());
            let f = Fixture::members(vec![
                (
                    "terrain.ter".into(),
                    model(FAMILIES[2].0, FAMILIES[2].1, true),
                ),
                ("object.mod".into(), object),
            ]);
            std::fs::write(
                &f.descriptor,
                sole_object_descriptor(if required { 1 } else { 2 }),
            )
            .unwrap();
            let error = inspect_eqg_pbr_materials(
                &f.archive,
                DescriptorSource::Loose(&f.descriptor),
                &EqgPbrLimits::default(),
            )
            .unwrap_err();
            assert!(
                format!("{error:#}").contains("parse model geometry"),
                "{error:#}"
            );
        }
    }
}

#[test]
fn preview_and_existing_bridge_refuse_multiple_distinct_terrains() {
    let mut desc = sole_object_descriptor(2);
    let pos = desc.windows(10).position(|b| b == b"object.mod").unwrap();
    desc[pos..pos + 10].copy_from_slice(b"second.ter");
    let f = Fixture::members(vec![
        (
            "terrain.ter".into(),
            model(FAMILIES[2].0, FAMILIES[2].1, true),
        ),
        (
            "second.ter".into(),
            model(FAMILIES[2].0, FAMILIES[2].1, true),
        ),
        ("Asymmetric.PNG".into(), png()),
    ]);
    std::fs::write(&f.descriptor, desc).unwrap();
    let preview_error = f.load().unwrap_err();
    let diagnostic = format!("{preview_error:#}");
    assert!(
        diagnostic.contains("requires exactly one terrain mesh")
            || diagnostic.contains("multiple distinct terrain meshes"),
        "{diagnostic}"
    );
    let source = load_binary_zone(&f.archive, DescriptorSource::Loose(&f.descriptor)).unwrap();
    let error = crate::eqg::static_scene::build_static_scene_with_unapplied_v2_lighting(
        &source,
        Default::default(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("multiple distinct terrain meshes"));
}

#[test]
fn source_budget_precharges_rows_properties_names_shaders_and_kind2_strings() {
    let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
    let mut source = load_binary_zone(&f.archive, DescriptorSource::Loose(&f.descriptor)).unwrap();
    let l = EqgPbrLimits::default();
    let before = source_budget(&source, &l).unwrap();
    let material = libeq_eqg::mesh::parse(&model(FAMILIES[2].0, FAMILIES[2].1, true))
        .unwrap()
        .materials
        .remove(0);
    // material (8), Opaque_MPLBasic.fx (18), e_TextureDiffuse0 (17), Asymmetric.PNG (14).
    let resolved_bytes = 8 + 18 + 17 + 14;
    let properties = 1;
    source.meshes[0].materials.push(material);
    let after = source_budget(&source, &l).unwrap();
    assert_eq!(
        after.report_records - before.report_records,
        1 + 8 + properties
    );
    assert_eq!(
        after.report_string_bytes - before.report_string_bytes,
        1024 + resolved_bytes * 3
    );
    let mut limited = l.clone();
    limited.max_report_records = after.report_records - 1;
    assert!(source_budget(&source, &limited).is_err());
    limited = l;
    limited.max_total_name_bytes = after.report_string_bytes - 1;
    assert!(source_budget(&source, &limited).is_err());
}

#[test]
fn selected_source_assembly_precharges_streamed_model_identities() {
    let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
    for (records, strings, diagnostic) in [
        (0, 4096, "inventory model record"),
        (1, 4096, "inventory model record"),
        (100, 0, "inventory model string"),
        (100, 11 + 64, "inventory model string"),
        (100, 148, "inventory model string"),
    ] {
        let mut l = EqgPbrLimits::default();
        let mut archive = archive::Archive::open(&f.archive, &l).unwrap();
        let selector = DescriptorSource::Loose(&f.descriptor);
        let (bytes, _, _, selected) = read_descriptor(&mut archive, selector, &l).unwrap();
        l.max_report_records = records;
        l.max_total_name_bytes = strings;
        let error = inventory::assemble_source(
            &f.archive,
            selector,
            &bytes,
            selected.as_deref(),
            &mut archive,
            &l,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains(diagnostic), "{error:#}");
    }
    let mut l = EqgPbrLimits::default();
    let mut archive = archive::Archive::open(&f.archive, &l).unwrap();
    let selector = DescriptorSource::Loose(&f.descriptor);
    let (bytes, _, _, selected) = read_descriptor(&mut archive, selector, &l).unwrap();
    l.max_report_records = 2;
    l.max_total_name_bytes = 149;
    let (_, models) = inventory::assemble_source(
        &f.archive,
        selector,
        &bytes,
        selected.as_deref(),
        &mut archive,
        &l,
    )
    .unwrap();
    assert_eq!(models.len(), 2);
}

#[test]
fn selected_source_assembly_checks_identity_caps_before_member_reads() {
    let f = Fixture::new(FAMILIES[2].0, FAMILIES[2].1);
    for (records, strings, diagnostic) in [
        (0, 4096, "inventory model record"),
        (100, 74, "inventory model string"),
    ] {
        let mut l = EqgPbrLimits::default();
        let mut archive = archive::Archive::open(&f.archive, &l).unwrap();
        let selector = DescriptorSource::Loose(&f.descriptor);
        let (bytes, _, _, selected) = read_descriptor(&mut archive, selector, &l).unwrap();
        let descriptor_read_calls = archive.read_calls;
        l.max_member_bytes = 1;
        l.max_report_records = records;
        l.max_total_name_bytes = strings;
        let error = inventory::assemble_source(
            &f.archive,
            selector,
            &bytes,
            selected.as_deref(),
            &mut archive,
            &l,
        )
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains(diagnostic), "{message}");
        assert!(!message.contains("selected member byte limit"), "{message}");
        assert_eq!(
            archive.read_calls, descriptor_read_calls,
            "identity preflight must precede the member read"
        );
    }
}
