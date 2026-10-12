use super::*;
use crate::static_scene::{
    write_static_visual, StaticInstance, StaticMesh, StaticPrimitive, StaticScene,
};
use glam::Mat4;
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use std::io::Cursor;

const REVISION: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn material(mode: AlphaMode) -> SourceMaterial {
    SourceMaterial {
        name: "source tint".into(),
        base_color: [0.8, 0.5, 0.2, 0.6],
        texture_index: Some(0),
        alpha_mode: mode,
        double_sided: false,
        texture_sequence: None,
    }
}
fn png(pixels: &[[u8; 4]]) -> Vec<u8> {
    let image = RgbaImage::from_raw(
        pixels.len() as u32,
        1,
        pixels.iter().flatten().copied().collect(),
    )
    .unwrap();
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}
fn paletted_bmp(rle: bool) -> Vec<u8> {
    let offset = 14 + 40 + 256 * 4;
    let body: Vec<u8> = if rle {
        vec![1, 0, 1, 1, 0, 0, 0, 1]
    } else {
        vec![0, 1, 0, 0]
    };
    let mut data = vec![0u8; offset + body.len()];
    data[..2].copy_from_slice(b"BM");
    for (offset, value) in [
        (2, data.len() as u32),
        (10, 1078),
        (14, 40),
        (18, 2),
        (22, 1),
        (30, if rle { 1 } else { 0 }),
        (34, body.len() as u32),
        (46, 256),
    ] {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    data[26..28].copy_from_slice(&1u16.to_le_bytes());
    data[28..30].copy_from_slice(&8u16.to_le_bytes());
    data[54..58].copy_from_slice(&[51, 99, 201, 0]);
    data[58..62].copy_from_slice(&[123, 237, 18, 0]);
    data[offset..].copy_from_slice(&body);
    data
}
fn dds() -> Vec<u8> {
    let mut data = vec![0; 128];
    data[..4].copy_from_slice(b"DDS ");
    for (offset, value) in [
        (4, 124),
        (8, 0x81007),
        (12, 1),
        (16, 2),
        (20, 8),
        (76, 32),
        (80, 0x41),
        (88, 32),
        (92, 0xff0000),
        (96, 0xff00),
        (100, 0xff),
        (104, 0xff000000),
        (108, 0x1000),
    ] {
        data[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
    }
    data.extend_from_slice(&[51, 99, 201, 149, 123, 237, 18, 203]);
    data
}
fn scene(mat: crate::static_scene::StaticMaterial, texture: StaticTexture) -> StaticScene {
    StaticScene {
        meshes: vec![StaticMesh {
            name: "triangle".into(),
            positions: vec![[-2., 0., 0.], [2., 0., 0.], [0., 0., 3.]],
            normals: vec![[0., -1., 0.]; 3],
            uvs: vec![[0., 0.], [1., 0.], [0.5, 1.]],
            primitives: vec![StaticPrimitive {
                indices: vec![0, 1, 2],
                material_index: 0,
                colors: None,
            }],
        }],
        materials: vec![mat],
        textures: vec![texture],
        instances: vec![StaticInstance {
            mesh_index: 0,
            matrix: Mat4::IDENTITY.to_cols_array_2d(),
        }],
    }
}
#[test]
fn legacy_opacity_png_bytes_remain_exact_for_existing_rounding_and_clamping() {
    let pixels = [[201, 99, 51, 149], [18, 237, 123, 203]];
    for blob in [png(&pixels), dds()] {
        for (permille, alpha) in [
            (250, [37, 51]),
            (500, [75, 102]),
            (750, [112, 152]),
            (1500, [224, 255]),
        ] {
            let mut expected = pixels;
            expected[0][3] = alpha[0];
            expected[1][3] = alpha[1];
            assert_eq!(
                crate::convert::encode_texture_png(&blob, AlphaMode::Blend(permille), "source")
                    .unwrap(),
                png(&expected)
            );
        }
    }
}
#[test]
fn legacy_masked_bmp_fallback_and_cutout_bytes_remain_unchanged() {
    assert_eq!(
        crate::convert::encode_texture_png(&paletted_bmp(false), AlphaMode::Masked, "indexed")
            .unwrap(),
        png(&[[201, 99, 51, 0], [18, 237, 123, 255]])
    );
    for blob in [paletted_bmp(false), paletted_bmp(true)] {
        assert_eq!(
            crate::convert::encode_texture_png(&blob, AlphaMode::Cutout(96), "indexed").unwrap(),
            png(&[[201, 99, 51, 255], [18, 237, 123, 255]])
        );
    }
    assert_eq!(
        crate::convert::encode_texture_png(&paletted_bmp(true), AlphaMode::Masked, "rle").unwrap(),
        png(&[[201, 99, 51, 255], [18, 237, 123, 255]])
    );
}
#[test]
fn blend_permille_multiplies_supplied_factor_alpha_once_without_changing_tint_or_sides() {
    for (permille, expected) in [(250, 0.15), (500, 0.3), (750, 0.45)] {
        let source = material(AlphaMode::Blend(permille));
        let result = normalize_material(&source).unwrap();
        assert_eq!(result.name, source.name);
        assert_eq!(result.texture_index, Some(0));
        assert!(!result.double_sided);
        assert_eq!(&result.base_color[..3], &source.base_color[..3]);
        assert!((result.base_color[3] - expected).abs() < 1e-6);
        assert_eq!(result.alpha_mode, StaticAlphaMode::Blend);
    }
    let mut source = material(AlphaMode::Blend(1000));
    source.double_sided = true;
    source.texture_index = None;
    let result = normalize_material(&source).unwrap();
    assert_eq!(result.base_color, source.base_color);
    assert!(result.double_sided);
    assert_eq!(result.texture_index, None);
    source.alpha_mode = AlphaMode::Blend(0);
    assert_eq!(normalize_material(&source).unwrap().base_color[3], 0.);
}
#[test]
fn opaque_and_cutouts_preserve_rgba_with_declared_nondefault_thresholds() {
    for (mode, expected) in [
        (AlphaMode::Opaque, StaticAlphaMode::Opaque),
        (AlphaMode::Masked, StaticAlphaMode::Mask { cutoff: 0.5 }),
        (AlphaMode::Cutout(0), StaticAlphaMode::Mask { cutoff: 0. }),
        (
            AlphaMode::Cutout(96),
            StaticAlphaMode::Mask { cutoff: 96. / 255. },
        ),
        (
            AlphaMode::Cutout(192),
            StaticAlphaMode::Mask {
                cutoff: 192. / 255.,
            },
        ),
        (AlphaMode::Cutout(255), StaticAlphaMode::Mask { cutoff: 1. }),
    ] {
        let source = material(mode);
        let result = normalize_material(&source).unwrap();
        assert_eq!(result.base_color, source.base_color);
        assert_eq!(result.alpha_mode, expected);
    }
}
#[test]
fn invalid_factors_opacity_and_unsupported_effects_fail_instead_of_fallback() {
    for bad in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        for channel in 0..4 {
            let mut source = material(AlphaMode::Opaque);
            source.base_color[channel] = bad;
            assert!(normalize_material(&source).is_err());
        }
    }
    for mode in [
        AlphaMode::Blend(1001),
        AlphaMode::Blend(u16::MAX),
        AlphaMode::Additive,
    ] {
        assert!(normalize_material(&material(mode)).is_err());
    }
    for sequence in [(0, vec![]), (100, vec!["one.png".into()])] {
        let mut source = material(AlphaMode::Opaque);
        source.texture_sequence = Some(sequence);
        assert!(normalize_material(&source).is_err());
    }
}
#[test]
fn texture_policy_is_explicit_and_common_png_dds_alpha_never_scales() {
    let original = [[201, 99, 51, 149], [18, 237, 123, 203]];
    for blob in [png(&original), dds()] {
        for policy in [
            TextureDecodePolicy::PreserveRgba,
            TextureDecodePolicy::PaletteIndexZero,
        ] {
            let result = decode_static_texture("source", &blob, policy).unwrap();
            assert_eq!(
                image::load_from_memory(&result.png_bytes)
                    .unwrap()
                    .into_rgba8()
                    .as_raw(),
                &original.into_iter().flatten().collect::<Vec<_>>()
            );
        }
    }
    let blob = paletted_bmp(false);
    let ordinary =
        decode_static_texture("ordinary", &blob, TextureDecodePolicy::PreserveRgba).unwrap();
    let keyed =
        decode_static_texture("keyed", &blob, TextureDecodePolicy::PaletteIndexZero).unwrap();
    assert_eq!(
        image::load_from_memory(&ordinary.png_bytes)
            .unwrap()
            .into_rgba8()
            .as_raw(),
        &[201, 99, 51, 255, 18, 237, 123, 255]
    );
    assert_eq!(
        image::load_from_memory(&keyed.png_bytes)
            .unwrap()
            .into_rgba8()
            .as_raw(),
        &[201, 99, 51, 0, 18, 237, 123, 255]
    );
}
#[test]
fn common_preserves_sixteen_bit_png_channels_and_png_bit_depth() {
    let pixels = [20001u16, 30003, 50005, 32895, 65535, 1, 17, 32896];
    let image: image::ImageBuffer<Rgba<u16>, Vec<u16>> =
        image::ImageBuffer::from_raw(2, 1, pixels.to_vec()).unwrap();
    let mut input = Cursor::new(Vec::new());
    DynamicImage::ImageRgba16(image)
        .write_to(&mut input, image::ImageFormat::Png)
        .unwrap();
    let texture = decode_static_texture(
        "precise",
        input.get_ref(),
        TextureDecodePolicy::PreserveRgba,
    )
    .unwrap();
    assert_eq!(texture.png_bytes[24], 16);
    assert_eq!(texture.png_bytes[25], 6);
    assert_eq!(
        image::load_from_memory(&texture.png_bytes)
            .unwrap()
            .into_rgba16()
            .as_raw(),
        &pixels
    );
}
#[test]
fn unsupported_keyed_bmp_and_invalid_images_are_explicit_errors() {
    assert!(decode_static_texture(
        "rle",
        &paletted_bmp(true),
        TextureDecodePolicy::PaletteIndexZero
    )
    .is_err());
    let mut short_dib = paletted_bmp(false);
    short_dib[14..18].copy_from_slice(&12u32.to_le_bytes());
    assert!(
        decode_static_texture("dib", &short_dib, TextureDecodePolicy::PaletteIndexZero).is_err()
    );
    for blob in [
        vec![1, 2, 3],
        dds()[..128].to_vec(),
        png(&[[1, 2, 3, 4]])[..40].to_vec(),
    ] {
        assert!(decode_static_texture("bad", &blob, TextureDecodePolicy::PreserveRgba).is_err());
    }
}
#[test]
fn source_texture_limits_are_applied_before_custom_decode_allocation() {
    for mut blob in [dds(), paletted_bmp(false)] {
        if blob.starts_with(b"DDS ") {
            blob[16..20].copy_from_slice(&8193u32.to_le_bytes());
        } else {
            blob[18..22].copy_from_slice(&8193i32.to_le_bytes());
        }
        assert!(
            decode_static_texture("oversize", &blob, TextureDecodePolicy::PaletteIndexZero)
                .is_err()
        );
    }
    let image = RgbaImage::new(8193, 1);
    let mut wide = Cursor::new(Vec::new());
    image.write_to(&mut wide, image::ImageFormat::Png).unwrap();
    assert!(
        decode_static_texture("wide", wide.get_ref(), TextureDecodePolicy::PreserveRgba).is_err()
    );
    // Encoded input is tiny; expansion to RGBA is 72 MiB and must fail before allocation.
    let image = image::GrayImage::new(8192, 2304);
    let mut compressed = Cursor::new(Vec::new());
    image
        .write_to(&mut compressed, image::ImageFormat::Png)
        .unwrap();
    assert!(compressed.get_ref().len() < 1024 * 1024);
    assert!(decode_static_texture(
        "expanded",
        compressed.get_ref(),
        TextureDecodePolicy::PreserveRgba
    )
    .is_err());
}
#[test]
fn actual_common_glb_keeps_texture_alpha_and_only_one_material_opacity_factor() {
    let input = png(&[[201, 99, 51, 149], [18, 237, 123, 203]]);
    let dir = tempfile::tempdir().unwrap();
    for (permille, alpha) in [(250, 0.15), (500, 0.3), (750, 0.45)] {
        let normalized = normalize_material(&material(AlphaMode::Blend(permille))).unwrap();
        let texture =
            decode_static_texture("input", &input, TextureDecodePolicy::PreserveRgba).unwrap();
        let path = dir.path().join("source.glb");
        write_static_visual(&scene(normalized, texture), REVISION, &path).unwrap();
        let (document, _, images) = gltf::import(&path).unwrap();
        let actual = document.materials().next().unwrap();
        let color = actual.pbr_metallic_roughness().base_color_factor();
        assert_eq!(&color[..3], &[0.8, 0.5, 0.2]);
        assert!((color[3] - alpha).abs() < 1e-6);
        assert_eq!(actual.alpha_mode(), gltf::material::AlphaMode::Blend);
        assert!(!actual.double_sided());
        assert_eq!(images[0].pixels, vec![201, 99, 51, 149, 18, 237, 123, 203]);
    }
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut output = (data.len() as u32).to_be_bytes().to_vec();
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    output.extend_from_slice(&crc32fast::hash(&output[4..]).to_be_bytes());
    output
}
#[test]
fn incomplete_trailing_crc_damaged_and_animated_png_cannot_be_flattened_to_static() {
    let original = png(&[[201, 99, 51, 149]]);
    let mut animation = original[..33].to_vec();
    let mut control = 1u32.to_be_bytes().to_vec();
    control.extend_from_slice(&0u32.to_be_bytes());
    animation.extend(png_chunk(b"acTL", &control));
    let mut frame = Vec::new();
    for n in [0u32, 1, 1, 0, 0] {
        frame.extend_from_slice(&n.to_be_bytes());
    }
    frame.extend_from_slice(&1u16.to_be_bytes());
    frame.extend_from_slice(&10u16.to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    animation.extend(png_chunk(b"fcTL", &frame));
    animation.extend_from_slice(&original[33..]);
    let decoder = image::codecs::png::PngDecoder::new(Cursor::new(&animation)).unwrap();
    assert!(decoder.is_apng().unwrap());
    assert!(image::load_from_memory(&animation).is_ok());
    let mut trailing = original.clone();
    trailing.extend_from_slice(b"junk");
    let mut damaged = original.clone();
    damaged[29] ^= 1;
    for blob in [
        animation,
        trailing,
        damaged,
        original[..original.len() - 12].to_vec(),
    ] {
        assert!(
            decode_static_texture("container", &blob, TextureDecodePolicy::PreserveRgba).is_err()
        );
    }
}
#[test]
fn custom_decoder_preflights_dimensions_and_rgba_size_from_small_headers() {
    let mut wide_dds = dds();
    wide_dds[16..20].copy_from_slice(&8193u32.to_le_bytes());
    wide_dds[20..24].copy_from_slice(&(8193u32 * 4).to_le_bytes());
    wide_dds.resize(128 + 8193 * 4, 0);
    let mut wide_bmp = paletted_bmp(false);
    wide_bmp[18..22].copy_from_slice(&8193i32.to_le_bytes());
    wide_bmp.resize(1078 + 8196, 0);
    let file_size = wide_bmp.len() as u32;
    wide_bmp[2..6].copy_from_slice(&file_size.to_le_bytes());
    for blob in [wide_dds, wide_bmp] {
        let error = decode_static_texture("wide", &blob, TextureDecodePolicy::PaletteIndexZero)
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("texture dimensions exceed static limit"),
            "{error:#}"
        );
    }
    // These small headers request more than 64 MiB of RGBA. The allocation check
    // must precede payload inspection and allocation in both custom decoders.
    let mut huge_dds = dds();
    huge_dds[16..20].copy_from_slice(&8192u32.to_le_bytes());
    huge_dds[12..16].copy_from_slice(&2049u32.to_le_bytes());
    let mut huge_bmp = paletted_bmp(false);
    huge_bmp[18..22].copy_from_slice(&8192i32.to_le_bytes());
    huge_bmp[22..26].copy_from_slice(&2049i32.to_le_bytes());
    for blob in [huge_dds, huge_bmp] {
        let error = decode_static_texture("expanded", &blob, TextureDecodePolicy::PaletteIndexZero)
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("decoded RGBA allocation exceeds static limit"),
            "{error:#}"
        );
    }
}
#[test]
fn encoded_input_budget_fails_before_format_guessing() {
    let payload = vec![0; 64 * 1024 * 1024 + 1];
    let error =
        decode_static_texture("encoded", &payload, TextureDecodePolicy::PreserveRgba).unwrap_err();
    assert!(
        error.to_string().contains("encoded byte limit"),
        "{error:#}"
    );
}
#[test]
fn actual_common_glb_preserves_sixteen_bit_source_image_samples() {
    let values = [20001u16, 30003, 50005, 32895];
    let image: image::ImageBuffer<Rgba<u16>, Vec<u16>> =
        image::ImageBuffer::from_raw(1, 1, values.to_vec()).unwrap();
    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgba16(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let texture =
        decode_static_texture("precise", png.get_ref(), TextureDecodePolicy::PreserveRgba).unwrap();
    let mat = normalize_material(&material(AlphaMode::Blend(500))).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("precise.glb");
    write_static_visual(&scene(mat, texture), REVISION, &path).unwrap();
    let (_, _, images) = gltf::import(path).unwrap();
    assert_eq!(images[0].format, gltf::image::Format::R16G16B16A16);
    let actual: Vec<u16> = images[0]
        .pixels
        .chunks_exact(2)
        .map(|bytes| u16::from_ne_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(actual, values);
}

fn compressed_dds(variant: u32, dx10: bool) -> Vec<u8> {
    let color = if variant == 1 {
        vec![255, 255, 0, 0, 0, 0, 0, 0]
    } else {
        vec![8, 66, 0, 0, 0, 0, 0, 0]
    };
    let block = match variant {
        1 => color,
        3 => [vec![0x88; 8], color].concat(),
        5 => [vec![128, 0, 0, 0, 0, 0, 0, 0], color].concat(),
        _ => unreachable!(),
    };
    let mut data = vec![0u8; 128];
    data[..4].copy_from_slice(b"DDS ");
    for (offset, value) in [
        (4, 124),
        (8, 0x81007),
        (12, 4),
        (16, 4),
        (20, block.len() as u32),
        (76, 32),
        (80, 4),
        (108, 0x1000),
    ] {
        data[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
    }
    data[84..88].copy_from_slice(if dx10 {
        b"DX10"
    } else if variant == 1 {
        b"DXT1"
    } else if variant == 3 {
        b"DXT3"
    } else {
        b"DXT5"
    });
    if dx10 {
        let format = match variant {
            1 => 71,
            3 => 74,
            5 => 77,
            _ => unreachable!(),
        };
        for word in [format, 3, 0, 1, 0] {
            data.extend_from_slice(&u32::to_le_bytes(word));
        }
    }
    data.extend_from_slice(&block);
    data
}
#[test]
fn compressed_cube_volume_and_dx10_arrays_cannot_be_flattened_to_one_static_image() {
    let mut cube = compressed_dds(1, false);
    let block = cube[128..].to_vec();
    cube[108..112].copy_from_slice(&0x1008u32.to_le_bytes());
    cube[112..116].copy_from_slice(&0xfe00u32.to_le_bytes());
    for _ in 0..5 {
        cube.extend_from_slice(&block);
    }
    let mut volume = compressed_dds(1, false);
    volume[8..12].copy_from_slice(&0x881007u32.to_le_bytes());
    volume[24..28].copy_from_slice(&2u32.to_le_bytes());
    volume[108..112].copy_from_slice(&0x1008u32.to_le_bytes());
    volume[112..116].copy_from_slice(&0x200000u32.to_le_bytes());
    volume.extend_from_slice(&block);
    let mut array = compressed_dds(1, true);
    array[140..144].copy_from_slice(&2u32.to_le_bytes());
    array.extend_from_slice(&block);
    let mut dx_cube = compressed_dds(1, true);
    dx_cube[136..140].copy_from_slice(&4u32.to_le_bytes());
    for _ in 0..5 {
        dx_cube.extend_from_slice(&block);
    }
    for blob in [cube, volume, array, dx_cube] {
        assert_eq!(image::load_from_memory(&blob).unwrap().dimensions(), (4, 4));
        assert!(crate::convert::encode_texture_png(&blob, AlphaMode::Opaque, "legacy").is_ok());
        assert!(
            decode_static_texture("multi-image", &blob, TextureDecodePolicy::PreserveRgba).is_err()
        );
    }
    let mut declared_3d = compressed_dds(1, true);
    declared_3d[132..136].copy_from_slice(&4u32.to_le_bytes());
    assert!(image::load_from_memory(&declared_3d).is_ok());
    assert!(
        decode_static_texture("dimension", &declared_3d, TextureDecodePolicy::PreserveRgba)
            .is_err()
    );
}
#[test]
fn dx10_bc2_and_bc3_preserve_straight_partial_alpha_but_reject_other_conventions() {
    for variant in [3, 5] {
        for mode in [0u32, 1] {
            let mut blob = compressed_dds(variant, true);
            blob[144..148].copy_from_slice(&mode.to_le_bytes());
            let original = image::load_from_memory(&blob).unwrap().into_rgba8();
            assert_eq!(
                original.get_pixel(0, 0)[3],
                if variant == 3 { 136 } else { 128 }
            );
            let output =
                decode_static_texture("straight", &blob, TextureDecodePolicy::PreserveRgba)
                    .unwrap();
            assert_eq!(
                image::load_from_memory(&output.png_bytes)
                    .unwrap()
                    .into_rgba8(),
                original
            );
        }
        for mode in [2u32, 3, 4] {
            let mut blob = compressed_dds(variant, true);
            blob[144..148].copy_from_slice(&mode.to_le_bytes());
            let original = image::load_from_memory(&blob).unwrap().into_rgba8();
            assert!(original.get_pixel(0, 0)[3] < 255);
            assert!(decode_static_texture(
                "alpha convention",
                &blob,
                TextureDecodePolicy::PreserveRgba
            )
            .is_err());
        }
        let mut reserved = compressed_dds(variant, true);
        reserved[144..148].copy_from_slice(&8u32.to_le_bytes());
        assert!(decode_static_texture(
            "reserved alpha",
            &reserved,
            TextureDecodePolicy::PreserveRgba
        )
        .is_err());
    }
}
