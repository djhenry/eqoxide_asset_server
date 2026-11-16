use super::*;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use libeq_pfs::PfsWriter;
use std::{io::Cursor, path::PathBuf};
fn words(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn encode(s: &[u8]) -> Vec<u8> {
    let key = [0x95, 0x3a, 0xc5, 0x2a, 0x95, 0x7a, 0x95, 0x6a];
    s.iter().enumerate().map(|(i, b)| b ^ key[i % 8]).collect()
}
fn table(s: &mut Vec<u8>, v: &str) -> u32 {
    let i = s.len() as i32;
    s.extend(v.as_bytes());
    s.push(0);
    (-i) as u32
}
fn document(fragments: Vec<(u32, Vec<u8>)>, strings: Vec<u8>) -> Vec<u8> {
    let mut b = words(&[
        0x54503d02,
        0x15500,
        fragments.len() as u32,
        0,
        0,
        strings.len() as u32,
        0,
    ]);
    b.extend(encode(&strings));
    for (t, v) in fragments {
        b.extend(words(&[v.len() as u32, t]));
        b.extend(v)
    }
    b
}
fn model(object: bool, mode: u32, bad_group: bool) -> Vec<u8> {
    let mut strings = vec![0];
    let meshname = table(
        &mut strings,
        if object {
            "OBJ_DMSPRITEDEF"
        } else {
            "TERRAIN_DMSPRITEDEF"
        },
    );
    let actorname = table(&mut strings, "OBJ_ACTORDEF");
    let matname = table(&mut strings, "MAT_MDF");
    let name = encode(b"tex.png\0");
    let mut bitmap = words(&[0, 0]);
    bitmap.extend((name.len() as u16).to_le_bytes());
    bitmap.extend(name);
    let sprite = words(&[0, 0, 1, 1]);
    let reference = words(&[0, 2, 0x50]);
    let material = words(&[
        matname,
        1,
        mode,
        0x112233,
        1f32.to_bits(),
        1f32.to_bits(),
        3,
    ]);
    let palette = words(&[0, 0, 1, 4]);
    let mut mesh = words(&[meshname, 0x18003, 5, 0, 0, 0]);
    for f in if object { [0f32; 3] } else { [10f32, 20., 30.] } {
        mesh.extend(f.to_le_bytes())
    }
    mesh.extend(words(&[0; 10]));
    for x in [3u16, 3, 3, 0, 1, 0, 1, 0, 0, 1] {
        mesh.extend(x.to_le_bytes())
    }
    for p in [[2i16, 4, 6], [6, 4, 6], [2, 8, 6]] {
        for x in p {
            mesh.extend(x.to_le_bytes())
        }
    }
    for uv in [[64i16, 128], [128, 128], [64, 256]] {
        for x in uv {
            mesh.extend(x.to_le_bytes())
        }
    }
    for _ in 0..3 {
        mesh.extend([0, 0, 127])
    }
    mesh.extend(0u16.to_le_bytes());
    for x in [0u16, 2, 1] {
        mesh.extend(x.to_le_bytes())
    }
    mesh.extend(if bad_group { 2u16 } else { 1u16 }.to_le_bytes());
    mesh.extend(0u16.to_le_bytes());
    let mut frags = vec![
        (3, bitmap),
        (4, sprite),
        (5, reference),
        (0x30, material),
        (0x31, palette),
        (0x36, mesh),
    ];
    if object {
        frags.push((0x2d, words(&[0, 6, 0])));
        frags.push((0x14, words(&[actorname, 0, 0, 0, 1, 0, 7, 0])))
    }
    document(frags, strings)
}
fn actors(scale: f32) -> Vec<u8> {
    let mut strings = vec![0];
    let name = table(&mut strings, "OBJ_ACTORDEF");
    let mut frags = vec![];
    for (p, s) in [([10f32, 20., 30.], scale), ([-10f32, 5., 7.], 0.005)] {
        let mut b = words(&[0, name, 0x2a, 0]);
        for x in p.into_iter().chain([128., 0., 0.]) {
            b.extend(x.to_le_bytes())
        }
        b.extend(0u32.to_le_bytes());
        b.extend(s.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        frags.push((0x15, b))
    }
    document(frags, strings)
}
fn png(color: [u8; 4]) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba(color)))
        .write_to(&mut c, ImageFormat::Png)
        .unwrap();
    c.into_inner()
}
fn archive(path: &Path, entries: Vec<(&str, Vec<u8>)>) {
    let mut p = PfsWriter::create(std::fs::File::create(path).unwrap()).unwrap();
    for (n, b) in entries {
        p.insert(n, Cursor::new(b)).unwrap();
    }
    p.finish().unwrap();
}
fn fixture(mode: u32, bad_group: bool, scale: f32) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("sample.s3d");
    let o = d.path().join("sample_obj.s3d");
    archive(
        &a,
        vec![
            ("sample.wld", model(false, mode, bad_group)),
            ("objects.wld", actors(scale)),
            ("tex.png", png([255, 0, 0, 255])),
        ],
    );
    archive(
        &o,
        vec![
            ("sample_obj.wld", model(true, mode, false)),
            ("tex.png", png([0, 0, 255, 255])),
        ],
    );
    (d, a, o)
}
#[test]
fn resolves_real_pfs_wld_materials_and_repeated_objects() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default())
        .expect("supported WLD source must resolve");
    assert_eq!(r.scene.meshes.len(), 2);
    assert_eq!(r.scene.instances.len(), 3);
    assert_eq!(
        r.scene.meshes[0].primitives[0]
            .indices
            .iter()
            .map(|i| r.scene.meshes[0].positions[*i as usize])
            .collect::<Vec<_>>(),
        [[22., 11., 33.], [24., 11., 33.], [22., 13., 33.]]
    );
    assert_eq!(r.scene.meshes[0].normals, [[0., 0., 1.]; 3]);
    assert_eq!(
        r.scene.meshes[0].primitives[0]
            .indices
            .iter()
            .map(|i| r.scene.meshes[0].uvs[*i as usize])
            .collect::<Vec<_>>(),
        [[0.25, 0.5], [0.25, 0.], [0.5, 0.5]]
    );
    assert_eq!(r.scene.materials.len(), 2);
    assert!(r
        .scene
        .meshes
        .iter()
        .flat_map(|m| &m.primitives)
        .all(|p| p.colors.is_none()));
    let m = glam::Mat4::from_cols_array_2d(&r.scene.instances[1].matrix);
    let p = m.transform_point3(glam::Vec3::from_array(r.scene.meshes[1].positions[0]));
    assert!(
        (p - glam::Vec3::new(22., 6., 36.)).abs().max_element() < 1e-4,
        "{p:?}"
    );
    assert_eq!(
        r.scene.instances[1].mesh_index,
        r.scene.instances[2].mesh_index
    );
    assert!(
        (glam::Mat4::from_cols_array_2d(&r.scene.instances[2].matrix)
            .x_axis
            .truncate()
            .length()
            - 0.005)
            .abs()
            < 1e-7
    );
}
#[test]
fn rejects_incomplete_material_groups() {
    let (_d, a, o) = fixture(0x80000001, true, 2.);
    let e = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err();
    assert!(format!("{e:#}").contains("face material groups"));
}
#[test]
fn refuses_additive_coverage_without_silent_opaque_fallback() {
    let (_d, a, o) = fixture(0x80000017, false, 2.);
    let e = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err();
    assert!(format!("{e:#}").contains("coverage"));
}
fn replace_main(a: &Path, body: Vec<u8>, extra_texture: bool) {
    let mut v = vec![
        ("sample.wld", body),
        ("objects.wld", actors(2.)),
        ("tex.png", png([255, 0, 0, 255])),
    ];
    if extra_texture {
        v.push(("TEX.PNG", png([1, 2, 3, 255])))
    }
    archive(a, v)
}
fn edit_fragment(mut b: Vec<u8>, kind: u32, edit: impl FnOnce(&mut [u8])) -> Vec<u8> {
    let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let mut p = 28 + word(&b, 20) as usize;
    let count = word(&b, 8);
    for _ in 0..count {
        let n = word(&b, p) as usize;
        if word(&b, p + 4) == kind {
            edit(&mut b[p + 8..p + 8 + n]);
            return b;
        }
        p += 8 + n;
    }
    panic!("missing fixture fragment")
}
#[test]
fn accepts_explicit_wld_terminator_but_not_arbitrary_trailing_content() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let mut b = model(false, 0x80000001, false);
    b.extend([255; 4]);
    replace_main(&a, b, false);
    load_wld_visual(&a, Some(&o), &WldVisualLimits::default())
        .expect("four-byte WLD terminator is allowed");
    let mut b = model(false, 0x80000001, false);
    b.extend([1, 2, 3]);
    replace_main(&a, b, false);
    assert!(format!(
        "{:#}",
        load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err()
    )
    .contains("trailing"));
}
#[test]
fn every_logical_budget_can_refuse_supported_input() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap();
    for n in 0..10 {
        let mut l = WldVisualLimits::default();
        match n {
            0 => l.max_archive_bytes = 0,
            1 => l.max_member_bytes = 0,
            2 => l.max_input_bytes = 0,
            3 => l.max_fragments = 0,
            4 => l.max_vertices = 0,
            5 => l.max_indices = 0,
            6 => l.max_instances = 0,
            7 => l.max_resources = 0,
            8 => l.max_transform_visits = 0,
            _ => l.max_output_bytes = 0,
        };
        let e = load_wld_visual(&a, Some(&o), &l).unwrap_err();
        assert!(format!("{e:#}").contains("limit"), "budget {n}: {e:#}")
    }
}
#[test]
fn missing_or_ambiguous_texture_never_becomes_white() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    archive(
        &a,
        vec![
            ("sample.wld", model(false, 0x80000001, false)),
            ("objects.wld", actors(2.)),
        ],
    );
    archive(&o, vec![("sample_obj.wld", model(true, 0x80000001, false))]);
    assert!(format!(
        "{:#}",
        load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err()
    )
    .contains("missing required texture"));
    replace_main(&a, model(false, 0x80000001, false), true);
    assert!(format!(
        "{:#}",
        load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err()
    )
    .contains("ambiguous"));
}
#[test]
fn selected_provider_supplies_pixels_and_uv_convention() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap();
    let p0 = image::load_from_memory(&r.scene.textures[0].png_bytes)
        .unwrap()
        .into_rgba8();
    let p1 = image::load_from_memory(&r.scene.textures[1].png_bytes)
        .unwrap()
        .into_rgba8();
    assert_eq!(p0.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(p1.get_pixel(0, 0).0, [0, 0, 255, 255]);
    assert_eq!(
        r.report
            .materials
            .iter()
            .map(|m| m.texture_provider)
            .collect::<Vec<_>>(),
        [Some("main"), Some("objects")]
    );
    assert!(r.report.materials.iter().all(|m| m.flip_v));
}
#[test]
fn keyed_mask_preserves_declared_cutoff_and_sidedness() {
    let (_d, a, o) = fixture(0x80000013, false, 2.);
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap();
    assert!(r.scene.materials.iter().all(|m| m.alpha_mode
        == crate::static_scene::StaticAlphaMode::Mask { cutoff: 0.5 }
        && m.double_sided));
}
#[test]
fn missing_model_or_static_sprite_reference_refuses() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    archive(
        &o,
        vec![
            (
                "sample_obj.wld",
                edit_fragment(model(true, 0x80000001, false), 0x14, |b| {
                    b[24..28].copy_from_slice(&1234u32.to_le_bytes())
                }),
            ),
            ("tex.png", png([0, 0, 255, 255])),
        ],
    );
    let e = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err();
    assert!(format!("{e:#}").contains("static DmSprite"));
}
#[test]
fn face_index_and_material_reference_are_checked() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let b = edit_fragment(model(false, 0x80000001, false), 0x36, |b| {
        b[137..139].copy_from_slice(&9u16.to_le_bytes())
    });
    replace_main(&a, b, false);
    assert!(format!(
        "{:#}",
        load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err()
    )
    .contains("face index"));
    let b = edit_fragment(model(false, 0x80000001, false), 0x31, |b| {
        b[12..16].copy_from_slice(&1234u32.to_le_bytes())
    });
    replace_main(&a, b, false);
    assert!(format!(
        "{:#}",
        load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err()
    )
    .contains("material reference"));
}
#[test]
fn zero_negative_and_nonfinite_scales_are_not_replaced() {
    for scale in [0., -1., f32::NAN, f32::INFINITY] {
        let (_d, a, o) = fixture(0x80000001, false, scale);
        let e = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err();
        assert!(format!("{e:#}").contains("scale"), "{e:#}")
    }
}
#[test]
fn source_lighting_is_preserved_as_provenance_not_color() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let before = std::fs::read(&a).unwrap();
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), before);
    assert!(r
        .scene
        .meshes
        .iter()
        .flat_map(|m| &m.primitives)
        .all(|p| p.colors.is_none()));
    assert_eq!(r.report.materials[0].source_rgb_pen, 0x112233);
    assert_eq!(r.scene.materials[0].base_color, [1.; 4]);
    assert!(r.report.sources.iter().all(|s| s.wld_blake3.len() == 64));
}
#[test]
fn old_scalar_profile_refuses_blend_without_replacing_existing_output() {
    let (_d, a, o) = fixture(0x80000005, false, 2.);
    let out = a.with_extension("glb");
    std::fs::write(&out, b"last-good-output").unwrap();
    let resolved = load_wld_visual(&a, Some(&o), &WldVisualLimits::default())
        .expect("ordinary source Blend is preserved independently of writer admission");
    let attempt = crate::static_scene::write_static_visual_with_material_policy(
        &resolved.scene,
        &crate::static_scene::StaticMaterialPolicy::default(),
        &resolved.report.bake_revision,
        &out,
    );
    let error = attempt.unwrap_err();
    assert!(format!("{error:#}").contains("PBR BLEND"));
    assert_eq!(std::fs::read(out).unwrap(), b"last-good-output");
}
fn duplicate_bitmap(mut b: Vec<u8>) -> Vec<u8> {
    let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let mut p = 28 + word(&b, 20) as usize;
    let n = word(&b, p) as usize;
    let first = p;
    let first_end = p + 8 + n;
    let matname = loop {
        if word(&b, p + 4) == 0x30 {
            break word(&b, p + 8);
        }
        p += 8 + word(&b, p) as usize
    };
    b[first + 8..first + 12].copy_from_slice(&matname.to_le_bytes());
    let extra = b[first..first_end].to_vec();
    let count = word(&b, 8) + 1;
    b[8..12].copy_from_slice(&count.to_le_bytes());
    b.extend(extra);
    b
}
#[test]
fn duplicate_unreferenced_bitmap_names_do_not_ambiguate_index_references() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    replace_main(&a, duplicate_bitmap(model(false, 0x80000001, false)), false);
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default())
        .expect("positive index references remain unambiguous");
    assert_eq!(r.scene.meshes.len(), 2)
}
#[test]
fn long_zero_suffix_is_not_a_terminator() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let mut b = model(false, 0x80000001, false);
    b.extend([0; 32]);
    replace_main(&a, b, false);
    let e = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err();
    assert!(format!("{e:#}").contains("trailing"))
}
#[test]
fn opaque_standard_without_texture_uses_explicit_modern_default() {
    let (_d, a, o) = fixture(0x53, false, 2.);
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default())
        .expect("solid untextured standard material is supported");
    assert!(r.scene.textures.is_empty());
    assert!(r.scene.materials.iter().all(|m| m.texture_index.is_none()
        && m.base_color == [1.; 4]
        && m.alpha_mode == crate::static_scene::StaticAlphaMode::Opaque));
}
#[test]
fn standard_trans_texture_and_wireframe_are_not_made_opaque() {
    for mode in [(3 << 7) | 0x53, 0x52, 0x51] {
        let (_d, a, o) = fixture(mode, false, 2.);
        let e = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err();
        assert!(format!("{e:#}").contains("coverage"))
    }
}
fn dds() -> Vec<u8> {
    let mut b = vec![0; 128];
    b[..4].copy_from_slice(b"DDS ");
    for (i, v) in [
        (4, 124u32),
        (8, 0x81007),
        (12, 2),
        (16, 2),
        (20, 16),
        (76, 32),
        (80, 0x41),
        (88, 32),
        (92, 0xff0000),
        (96, 0xff00),
        (100, 0xff),
        (104, 0xff000000),
        (108, 0x1000),
    ] {
        b[i..i + 4].copy_from_slice(&v.to_le_bytes())
    }
    b.extend([3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]);
    b
}
#[test]
fn different_texture_containers_share_one_selected_provider_policy() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    archive(
        &o,
        vec![
            ("sample_obj.wld", model(true, 0x80000001, false)),
            ("tex.png", dds()),
        ],
    );
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap();
    assert!(r.report.materials[0].flip_v && !r.report.materials[1].flip_v);
    let uv = r.scene.meshes[1].primitives[0]
        .indices
        .iter()
        .map(|i| r.scene.meshes[1].uvs[*i as usize])
        .collect::<Vec<_>>();
    assert_eq!(uv, [[0.25, 0.5], [0.25, 1.], [0.5, 0.5]]);
    assert_eq!(
        image::load_from_memory(&r.scene.textures[1].png_bytes)
            .unwrap()
            .into_rgba8()
            .get_pixel(0, 0)
            .0,
        [1, 2, 3, 4]
    );
}
fn map_fragment(mut b: Vec<u8>, kind: u32, edit: impl FnOnce(Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let mut p = 28 + word(&b, 20) as usize;
    for _ in 0..word(&b, 8) {
        let n = word(&b, p) as usize;
        if word(&b, p + 4) == kind {
            let v = edit(b[p + 8..p + 8 + n].to_vec());
            b[p..p + 4].copy_from_slice(&(v.len() as u32).to_le_bytes());
            b.splice(p + 8..p + 8 + n, v);
            return b;
        }
        p += 8 + n
    }
    panic!("missing fixture fragment")
}
#[test]
fn zero_area_exclusion_is_limited_to_coincident_source_positions() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let b = map_fragment(model(false, 0x80000001, false), 0x36, |mut b| {
        b[84..86].copy_from_slice(&2u16.to_le_bytes());
        b.splice(
            143..143,
            [0u16, 0, 0, 1].into_iter().flat_map(u16::to_le_bytes),
        );
        b[151..153].copy_from_slice(&2u16.to_le_bytes());
        b
    });
    replace_main(&a, b, false);
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap();
    assert_eq!(r.report.coincident_source_faces_excluded, 1);
    assert_eq!(r.report.meshes[0].source_faces, 2);
    assert_eq!(r.scene.meshes[0].primitives[0].indices.len(), 3);
    let b = edit_fragment(model(false, 0x80000001, false), 0x36, |b| {
        b[108..110].copy_from_slice(&8i16.to_le_bytes());
        b[110..112].copy_from_slice(&4i16.to_le_bytes())
    });
    replace_main(&a, b, false);
    assert!(format!(
        "{:#}",
        load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err()
    )
    .contains("other zero-area"));
}
#[test]
fn source_clockwise_faces_become_ccw_in_server_axes() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap();
    for m in &r.scene.meshes {
        for p in &m.primitives {
            for t in p.indices.chunks_exact(3) {
                let [a, b, c] =
                    [t[0], t[1], t[2]].map(|i| glam::Vec3::from_array(m.positions[i as usize]));
                let n = glam::Vec3::from_array(m.normals[t[0] as usize]);
                assert!((b - a).cross(c - a).dot(n) > 0.);
            }
        }
    }
}
#[test]
fn named_material_reference_requires_a_unique_typed_target() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let mut b = model(false, 0x80000001, false);
    let mut name = 0;
    let _ = edit_fragment(b.clone(), 0x30, |x| {
        name = u32::from_le_bytes(x[..4].try_into().unwrap())
    });
    b = edit_fragment(b, 0x31, |x| x[12..16].copy_from_slice(&name.to_le_bytes()));
    replace_main(&a, b.clone(), false);
    load_wld_visual(&a, Some(&o), &WldVisualLimits::default())
        .expect("unique named material target");
    let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let mut p = 28 + word(&b, 20) as usize;
    while word(&b, p + 4) != 0x30 {
        p += 8 + word(&b, p) as usize
    }
    let extra = b[p..p + 8 + word(&b, p) as usize].to_vec();
    let count = word(&b, 8) + 1;
    b[8..12].copy_from_slice(&count.to_le_bytes());
    b.extend(extra);
    replace_main(&a, b, false);
    assert!(format!(
        "{:#}",
        load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err()
    )
    .contains("ambiguous named"));
}
#[test]
fn sole_static_lod_metadata_does_not_become_animation_or_duplicate_geometry() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let b = map_fragment(model(true, 0x80000001, false), 0x14, |mut b| {
        b[12..16].copy_from_slice(&1u32.to_le_bytes());
        b.splice(
            24..24,
            [1u32, 0, 1000f32.to_bits()]
                .into_iter()
                .flat_map(u32::to_le_bytes),
        );
        b
    });
    archive(
        &o,
        vec![("sample_obj.wld", b), ("tex.png", png([0, 0, 255, 255]))],
    );
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default())
        .expect("one static sprite has no alternative LOD to select");
    assert_eq!(r.scene.instances.len(), 3);
    assert_eq!(r.scene.meshes.len(), 2);
}
#[test]
fn alternate_sprite_list_does_not_render_overlapping_lods() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let b = map_fragment(model(true, 0x80000001, false), 0x14, |mut b| {
        b[16..20].copy_from_slice(&2u32.to_le_bytes());
        b.splice(28..28, 7u32.to_le_bytes());
        b
    });
    archive(
        &o,
        vec![("sample_obj.wld", b), ("tex.png", png([0, 0, 255, 255]))],
    );
    let e = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err();
    assert!(format!("{e:#}").contains("single static sprite"));
}
#[test]
fn animated_textures_meshes_and_zero_normals_refuse() {
    for (kind, offset, value, needle) in [
        (4, 4, 8u32, "animation"),
        (0x36, 12, 1, "animated"),
        (0x36, 126, 0, "normal"),
    ] {
        let (_d, a, o) = fixture(0x80000001, false, 2.);
        let b = edit_fragment(model(false, 0x80000001, false), kind, |b| {
            if offset == 126 {
                b[offset..offset + 3].fill(0)
            } else {
                b[offset..offset + 4].copy_from_slice(&value.to_le_bytes())
            }
        });
        replace_main(&a, b, false);
        let e = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap_err();
        assert!(format!("{e:#}").contains(needle), "{e:#}")
    }
}
#[test]
fn full_actor_rotation_matches_independent_source_equations() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let rotations = [37f32, 61., -19.];
    let bytes = edit_fragment(actors(2.), 0x15, |b| {
        for (i, v) in rotations.iter().enumerate() {
            b[28 + i * 4..32 + i * 4].copy_from_slice(&v.to_le_bytes())
        }
    });
    archive(
        &a,
        vec![
            ("sample.wld", model(false, 0x80000001, false)),
            ("objects.wld", bytes),
            ("tex.png", png([255, 0, 0, 255])),
        ],
    );
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap();
    let [a, b, c] = rotations.map(|r| f64::from(r) * std::f64::consts::TAU / 512.);
    let (x, y, z) = (1f64, 2f64, 3f64);
    let (s, cx) = c.sin_cos();
    let (y, z) = (cx * y - s * z, s * y + cx * z);
    let (s, cy) = (-b).sin_cos();
    let (x, z) = (cy * x + s * z, -s * x + cy * z);
    let (s, cz) = a.sin_cos();
    let (x, y) = (cz * x - s * y, s * x + cz * y);
    let expected = glam::DVec3::new(20. + 2. * y, 10. + 2. * x, 30. + 2. * z);
    let matrix = glam::Mat4::from_cols_array_2d(&r.scene.instances[1].matrix);
    let got = matrix
        .transform_point3(glam::Vec3::new(2., 1., 3.))
        .as_dvec3();
    assert!(
        (got - expected).abs().max_element() < 2e-5,
        "{got:?} != {expected:?}"
    );
}

#[test]
fn ordinary_blend_factors_preserve_rgb_and_source_texture_alpha() {
    for (mode, factor) in [(0x80000009, 0.25), (0x80000005, 0.5), (0x8000000a, 0.75)] {
        let (_d, a, o) = fixture(mode, false, 2.);
        archive(
            &a,
            vec![
                ("sample.wld", model(false, mode, false)),
                ("objects.wld", actors(2.)),
                ("tex.png", png([91, 73, 42, 77])),
            ],
        );
        let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default())
            .expect("ordinary WLD Blend intent is representable");
        assert_eq!(r.scene.materials[0].base_color, [1., 1., 1., factor]);
        assert_eq!(
            r.scene.materials[0].alpha_mode,
            crate::static_scene::StaticAlphaMode::Blend
        );
        assert_eq!(
            image::load_from_memory(&r.scene.textures[0].png_bytes)
                .unwrap()
                .into_rgba8()
                .get_pixel(0, 0)
                .0,
            [91, 73, 42, 77]
        );
    }
}
#[test]
fn authored_source_pipeline_emits_the_common_pbr_artifact() {
    let (_d, a, o) = fixture(0x80000001, false, 2.);
    let r = load_wld_visual(&a, Some(&o), &WldVisualLimits::default()).unwrap();
    let path = a.with_extension("glb");
    let policy = crate::static_scene::write_static_visual_with_material_policy(
        &r.scene,
        &crate::static_scene::StaticMaterialPolicy::default(),
        &r.report.bake_revision,
        &path,
    )
    .unwrap();
    assert!(policy
        .requirements
        .capabilities
        .iter()
        .any(|s| s == "pbr-metallic-roughness-factors-v1"));
    let b = std::fs::read(path).unwrap();
    let g = gltf::Gltf::from_slice(&b).unwrap();
    assert_eq!(g.document.meshes().len(), 2);
    assert_eq!(g.document.nodes().len(), 3);
    assert!(g
        .document
        .materials()
        .all(|m| m.pbr_metallic_roughness().metallic_factor() == 0.
            && m.pbr_metallic_roughness().roughness_factor() == 1.));
}
