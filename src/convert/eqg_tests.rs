use super::parse_eqg_model;

fn word(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend(value.to_le_bytes());
}

struct Fixture {
    bytes: Vec<u8>,
    vertex: usize,
    triangle: usize,
    property: usize,
    texture: usize,
    shader: usize,
}

fn fixture(version: u32, model: bool) -> Fixture {
    let strings = b"mat\0shader\0TextureDiffuse0\0BOAT.DDS\0unknown\0";
    let mut bytes = if model { b"EQGM" } else { b"EQGT" }.to_vec();
    for value in [version, strings.len() as u32, 1, 2, 1] {
        word(&mut bytes, value);
    }
    if model {
        word(&mut bytes, 1);
    }
    let strings_start = bytes.len();
    bytes.extend(strings);
    let shader = bytes.len() + 8;
    for value in [0, 0, 4, 2, 11, 2, 27, 36, 77, u32::MAX] {
        word(&mut bytes, value);
    }
    let vertex = bytes.len();
    for position in [[1.0f32, 2.0, 3.0], [4.0, 5.0, 6.0]] {
        for value in position.into_iter().chain([0.0, 0.0, 1.0]) {
            word(&mut bytes, value.to_bits());
        }
        if version == 3 {
            word(&mut bytes, 0xff008080);
        }
        for value in [0.25f32, 0.75] {
            word(&mut bytes, value.to_bits());
        }
        if version == 3 {
            for value in [f32::NAN, f32::INFINITY] {
                word(&mut bytes, value.to_bits());
            }
        }
    }
    let triangle = bytes.len();
    for value in [0, 1, 0, 0, 0x80] {
        word(&mut bytes, value);
    }
    if version == 2 {
        word(&mut bytes, if model { 1 } else { 2 });
        for _ in 0..4 {
            word(&mut bytes, f32::NAN.to_bits());
        }
    }
    // Static conversion intentionally does not interpret skeletal suffixes.
    bytes.extend([0xab, 0xcd]);
    Fixture {
        bytes,
        vertex,
        triangle,
        property: strings_start + 11,
        texture: strings_start + 27,
        shader,
    }
}

#[test]
fn eqg_adapter_refuses_unresolved_shader_before_diffuse_fallback() {
    for model in [false, true] {
        let mut input = fixture(1, model);
        input.bytes[input.shader..input.shader + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let error = match parse_eqg_model(&input.bytes) {
            Err(error) => format!("{error:#}"),
            Ok(_) => panic!("unresolved shader was accepted"),
        };
        assert!(error.contains("unresolved shader"), "{error}");
        assert!(error.contains("0xffffffff"), "{error}");
    }
}

fn assert_unused_second_material_shader_is_refused(model: bool) {
    let mut input = fixture(1, model);
    let second_material = input.bytes[input.shader - 8..input.vertex].to_vec();
    assert_eq!(second_material.len(), 40);
    input
        .bytes
        .splice(input.vertex..input.vertex, second_material);
    input.bytes[12..16].copy_from_slice(&2u32.to_le_bytes());

    let mesh = parse_eqg_model(&input.bytes).unwrap();
    assert_eq!(
        mesh.mat_textures,
        [Some("boat.dds".into()), Some("boat.dds".into())]
    );
    assert_eq!(mesh.positions, [[1., 2., 3.], [4., 5., 6.]]);
    assert_eq!(mesh.normals, [[0., 0., 1.]; 2]);
    assert_eq!(mesh.uvs, [[0.25, 0.75]; 2]);
    assert_eq!(mesh.tris, [(0, 1, 0, 0)]);

    let second_shader = input.vertex + 8;
    input.bytes[second_shader..second_shader + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let error = match parse_eqg_model(&input.bytes) {
        Err(error) => format!("{error:#}"),
        Ok(_) => panic!("unresolved shader on unused second material was accepted"),
    };
    assert!(
        error.contains("EQG material 1 has unresolved shader word 0xffffffff"),
        "{error}"
    );
}

#[test]
fn eqgt_adapter_refuses_unresolved_shader_on_unused_second_material() {
    assert_unused_second_material_shader_is_refused(false);
}

#[test]
fn eqgm_adapter_refuses_unresolved_shader_on_unused_second_material() {
    assert_unused_second_material_shader_is_refused(true);
}

#[test]
fn eqg_adapter_keeps_valid_empty_shader_string_distinct() {
    let mut input = fixture(1, true);
    input.bytes[input.shader..input.shader + 4].copy_from_slice(&0u32.to_le_bytes());
    let string_table_start = 28;
    input.bytes[string_table_start] = 0;
    let mesh = parse_eqg_model(&input.bytes).unwrap();
    assert_eq!(mesh.mat_textures, [Some("boat.dds".into())]);
    assert_eq!(mesh.positions, [[1., 2., 3.], [4., 5., 6.]]);
}

#[test]
fn eqg_adapter_preserves_rendered_geometry_across_versions() {
    for version in 1..=3 {
        for model in [false, true] {
            let input = fixture(version, model);
            let mesh = parse_eqg_model(&input.bytes).unwrap();
            assert_eq!(mesh.positions, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
            assert_eq!(mesh.normals, [[0.0, 0.0, 1.0]; 2]);
            assert_eq!(mesh.uvs, [[0.25, 0.75]; 2]);
            assert_eq!(mesh.tris, [(0, 1, 0, 0)]);
            assert_eq!(mesh.mat_textures, [Some("boat.dds".into())]);
        }
    }
}

#[test]
fn eqg_adapter_rejects_unrenderable_references_and_headers() {
    let input = fixture(3, true);
    for (offset, value) in [
        (4, 4),
        (input.triangle, 2),
        (input.triangle + 12, 1),
        (input.triangle + 12, u32::MAX),
    ] {
        let mut bytes = input.bytes.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            parse_eqg_model(&bytes).is_err(),
            "accepted word {value} at {offset}"
        );
    }
    assert!(parse_eqg_model(&input.bytes[..input.triangle + 19]).is_err());
}

#[test]
fn eqg_adapter_rejects_nonfinite_rendered_attributes() {
    for (attribute, offset) in [("position", 0), ("normal", 12), ("UV", 24)] {
        let mut input = fixture(1, true);
        input.bytes[input.vertex + offset..input.vertex + offset + 4]
            .copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert!(
            parse_eqg_model(&input.bytes).is_err(),
            "accepted nonfinite {attribute}"
        );
    }
}

#[test]
fn eqg_adapter_rejects_non_utf8_lookup_strings() {
    for texture in [false, true] {
        let mut input = fixture(1, true);
        let offset = if texture {
            input.texture
        } else {
            input.property
        };
        input.bytes[offset] = 0xff;
        assert!(parse_eqg_model(&input.bytes).is_err());
    }
}
