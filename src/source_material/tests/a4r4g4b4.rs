use super::*;

fn put(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn packed(width: u32, height: u32, count: u32, words: &[u16]) -> Vec<u8> {
    let mut data = vec![0; 128];
    data[..4].copy_from_slice(b"DDS ");
    for (offset, value) in [
        (4, 124),
        (8, 0x1007),
        (12, height),
        (16, width),
        (28, count),
        (76, 32),
        (80, 0x41),
        (88, 16),
        (92, 0xf00),
        (96, 0xf0),
        (100, 0xf),
        (104, 0xf000),
        (108, 0x1000),
    ] {
        put(&mut data, offset, value);
    }
    for word in words {
        data.extend_from_slice(&word.to_le_bytes());
    }
    data
}
fn decode(data: &[u8]) -> RgbaImage {
    let result = decode_static_texture("packed", data, TextureDecodePolicy::PreserveRgba).unwrap();
    assert_eq!(&result.png_bytes[24..26], &[8, 6]);
    image::load_from_memory(&result.png_bytes)
        .unwrap()
        .into_rgba8()
}
fn refuses(data: &[u8], reason: &str) {
    let error =
        decode_static_texture("packed", data, TextureDecodePolicy::PreserveRgba).unwrap_err();
    assert!(format!("{error:#}").contains(reason), "{error:#}");
}
#[test]
fn asymmetric_little_endian_channels_alpha_rows_and_policies() {
    let data = packed(2, 2, 0, &[0x1234, 0xf08e, 0x709c, 0x0fed]);
    let expected = [
        34, 51, 68, 17, 0, 136, 238, 255, 0, 153, 204, 119, 255, 238, 221, 0,
    ];
    assert_eq!(decode(&data).as_raw(), &expected);
    for policy in [
        TextureDecodePolicy::PreserveRgba,
        TextureDecodePolicy::PaletteIndexZero,
    ] {
        let result = decode_static_texture("packed", &data, policy).unwrap();
        assert_eq!(
            image::load_from_memory(&result.png_bytes)
                .unwrap()
                .into_rgba8()
                .as_raw(),
            &expected
        );
    }
}
#[test]
fn every_packed_word_preserves_exact_normalized_channels() {
    let words: Vec<u16> = (0..=u16::MAX).collect();
    let result = decode(&packed(256, 256, 1, &words));
    assert_eq!(result.dimensions(), (256, 256));
    for (value, pixel) in result.pixels().enumerate() {
        let normalized = |shift: usize| (((value >> shift) & 15usize) * 255 / 15) as u8;
        assert_eq!(
            pixel.0,
            [normalized(8), normalized(4), normalized(0), normalized(12)],
            "word {value:#06x}"
        );
    }
}
#[test]
fn single_level_pitch_padding_and_unflagged_pitch_word() {
    let mut data = packed(1, 2, 1, &[0x1234, 0xbeef, 0x5678, 0xdead]);
    put(&mut data, 8, 0x100f);
    put(&mut data, 20, 4);
    assert_eq!(decode(&data).as_raw(), &[34, 51, 68, 17, 102, 119, 136, 85]);
    let mut tight = packed(1, 2, 1, &[0x1234, 0x5678]);
    put(&mut tight, 20, u32::MAX);
    assert_eq!(decode(&tight).as_raw(), decode(&data).as_raw());
    put(&mut tight, 8, 0x100f);
    put(&mut tight, 20, 1);
    refuses(&tight, "pitch");
}
#[test]
fn declared_mips_include_partial_nonsquare_and_missing_advisory_flags() {
    // 3x5 -> 1x2 -> 1x1. Only the base level becomes PNG.
    let words: Vec<u16> = (0..18).collect();
    let data = packed(3, 5, 3, &words);
    assert_eq!(
        decode(&data).as_raw(),
        decode(&packed(3, 5, 1, &words[..15])).as_raw()
    );
    assert_eq!(decode(&packed(3, 5, 2, &words[..17])).dimensions(), (3, 5));
    let mut axes_only = data.clone();
    put(&mut axes_only, 8, 0x6);
    put(&mut axes_only, 108, 0);
    assert_eq!(decode(&axes_only).as_raw(), decode(&data).as_raw());
    let mut explicit = data.clone();
    put(&mut explicit, 8, 0x2100f);
    put(&mut explicit, 20, 6);
    put(&mut explicit, 108, 0x401008);
    assert_eq!(decode(&explicit).as_raw(), decode(&data).as_raw());
    put(&mut explicit, 20, 8);
    refuses(&explicit, "padded");
    let mut excess = data.clone();
    put(&mut excess, 28, 4);
    refuses(&excess, "mip count");
    let mut extreme = data;
    put(&mut extreme, 28, u32::MAX);
    refuses(&extreme, "mip count");
}
#[test]
fn complete_declared_payload_is_required_without_trailing_bytes() {
    let data = packed(4, 2, 3, &[0x1234; 11]);
    for len in [128, 129, 143, 144, 145, 147, 148, 149] {
        refuses(&data[..len], "payload");
    }
    let mut trailing = data.clone();
    trailing.push(0);
    refuses(&trailing, "payload");
    // Full mip payload is checked before a decoded allocation budget failure.
    let mut huge = data;
    put(&mut huge, 16, 8192);
    put(&mut huge, 12, 2049);
    put(&mut huge, 28, 2);
    refuses(&huge, "payload");
}
#[test]
fn strict_packed_metadata_and_non_2d_forms_are_refused() {
    let good = packed(2, 1, 1, &[0x1234, 0x5678]);
    for (offset, value, reason) in [
        (4, 123, "header"),
        (76, 31, "header"),
        (8, 0x1001, "dimension flags"),
        (8, 0x1003, "dimension flags"),
        (8, 0x1005, "dimension flags"),
        (80, 0x40, "A4R4G4B4"),
        (80, 0x43, "A4R4G4B4"),
        (80, 0x20041, "A4R4G4B4"),
        (80, 0x241, "A4R4G4B4"),
        (84, 1, "DDS"),
        (80, 0x45, "DDS"),
        (92, 0xf, "A4R4G4B4"),
        (96, 0xf00, "A4R4G4B4"),
        (100, 0x1f, "A4R4G4B4"),
        (104, 0, "A4R4G4B4"),
        (104, 0xf, "A4R4G4B4"),
        (12, 0, "empty"),
        (16, 0, "empty"),
        (16, 8193, "dimensions"),
        (12, u32::MAX, "dimensions"),
        (16, u32::MAX, "dimensions"),
        (24, 2, "volume"),
        (8, 0x801007, "depth"),
        (112, 0x200, "cube"),
        (112, 0x400, "cube"),
        (112, 0x200000, "volume"),
    ] {
        let mut data = good.clone();
        put(&mut data, offset, value);
        refuses(&data, reason);
    }
    for offset in [88] {
        for value in [0, 15, 24, 32] {
            let mut data = good.clone();
            put(&mut data, offset, value);
            assert!(
                decode_static_texture("wrong bits", &data, TextureDecodePolicy::PreserveRgba)
                    .is_err()
            );
        }
    }
    for length in [4, 16, 76, 80, 127] {
        refuses(&good[..length], "header");
    }
    let mut ordinary_depth = good.clone();
    put(&mut ordinary_depth, 24, 1);
    assert_eq!(decode(&ordinary_depth).as_raw(), decode(&good).as_raw());
    put(&mut ordinary_depth, 8, 0x801007);
    refuses(&ordinary_depth, "depth");
    let mut dx10 = good.clone();
    put(&mut dx10, 84, u32::from_le_bytes(*b"DX10"));
    dx10.truncate(128);
    for value in [115, 3, 0, 2, 1] {
        dx10.extend_from_slice(&u32::to_le_bytes(value));
    }
    refuses(&dx10, "resource");
}
#[test]
fn legacy_sixteen_bit_refusal_and_existing_rgb32_output_remain_unchanged() {
    assert!(crate::convert::encode_texture_png(
        &packed(1, 1, 1, &[0x1234]),
        AlphaMode::Opaque,
        "packed"
    )
    .is_err());
    assert_eq!(
        decode_static_texture("old", &dds(), TextureDecodePolicy::PreserveRgba)
            .unwrap()
            .png_bytes,
        png(&[[201, 99, 51, 149], [18, 237, 123, 203]])
    );
}

#[test]
fn compressed_formats_ignore_unused_rgb_bit_count() {
    for variant in [1, 3, 5] {
        for dx10 in [false, true] {
            let control = compressed_dds(variant, dx10);
            let mut unused = control.clone();
            put(&mut unused, 88, 16);
            assert_eq!(decode(&unused).as_raw(), decode(&control).as_raw());
        }
    }
}
