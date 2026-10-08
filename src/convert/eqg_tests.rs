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
    }
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
