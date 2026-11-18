use eqoxide_asset_server::{source_material::portable::*, static_scene::*};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;
fn encode(image: DynamicImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}
fn scene(pixels: &[[u16; 4]]) -> StaticScene {
    let raw = pixels.iter().flatten().copied().collect::<Vec<_>>();
    StaticScene {
        textures: vec![StaticTexture {
            name: "input".into(),
            png_bytes: encode(DynamicImage::ImageRgba16(
                ImageBuffer::from_raw(pixels.len() as u32, 1, raw).unwrap(),
            )),
        }],
        ..Default::default()
    }
}
fn material(alpha_mode: StaticAlphaMode) -> StaticMaterial {
    StaticMaterial {
        name: "material".into(),
        base_color: [1.; 4],
        texture_index: Some(0),
        alpha_mode,
        double_sided: false,
    }
}
#[test]
fn every_u16_code_rounds_to_the_nearest_independent_rgba_code() {
    let raw = (0..=u16::MAX)
        .flat_map(|v| [v, u16::MAX - v, v.rotate_left(5), v])
        .collect::<Vec<_>>();
    let s = StaticScene {
        textures: vec![StaticTexture {
            name: "all".into(),
            png_bytes: encode(DynamicImage::ImageRgba16(
                ImageBuffer::from_raw(256, 256, raw.clone()).unwrap(),
            )),
        }],
        ..Default::default()
    };
    let p = prepare_portable_textures(&s, &PortableTextureLimits::default()).unwrap();
    let out = image::load_from_memory(&p.textures[0].png_bytes)
        .unwrap()
        .into_rgba8();
    for (&v, &q) in raw.iter().zip(out.as_raw()) {
        let expanded = i32::from(q) * 257;
        assert!((i32::from(v) - expanded).abs() <= 128, "v={v}, q={q}");
        if v % 257 == 0 {
            assert_eq!(u16::from(q), v / 257);
        }
    }
    assert_eq!(p.report.images[0].max_channel_error, [128; 4]);
    assert_eq!(p.report.images[0].changed_samples, [65280; 4]);
    assert_eq!(p.report.images[0].nonzero_alpha_to_zero, 128);
    assert_eq!(s.textures[0].name, "all");
    assert_eq!(
        image::load_from_memory(&s.textures[0].png_bytes)
            .unwrap()
            .into_rgba16()
            .as_raw(),
        &raw
    );
}
#[test]
fn boundaries_straight_alpha_names_and_report_are_explicit() {
    let s = scene(&[
        [0, 128, 129, 128],
        [32767, 32768, 65535, 129],
        [12345, 45678, 23456, 40000],
    ]);
    let before = s.textures[0].png_bytes.clone();
    let p = prepare_portable_textures(&s, &PortableTextureLimits::default()).unwrap();
    assert_eq!(
        image::load_from_memory(&p.textures[0].png_bytes)
            .unwrap()
            .into_rgba8()
            .as_raw(),
        &[0, 0, 1, 0, 127, 128, 255, 1, 48, 178, 91, 156]
    );
    assert_eq!(p.textures[0].name, "input:png8-nearest-v1");
    assert_eq!(p.report.policy, "portable-png8-nearest-v1");
    let r = &p.report.images[0];
    assert_eq!(
        (
            r.ordinal,
            r.width,
            r.height,
            r.input_bit_depth,
            r.output_bit_depth
        ),
        (0, 3, 1, 16, 8)
    );
    assert_eq!(r.original_name, "input");
    assert_eq!(r.emitted_name, p.textures[0].name);
    assert_eq!(
        r.source_png_blake3,
        blake3::hash(&before).to_hex().to_string()
    );
    assert_eq!(
        r.emitted_png_blake3,
        blake3::hash(&p.textures[0].png_bytes).to_hex().to_string()
    );
    assert_eq!(s.textures[0].png_bytes, before);
    assert_eq!(
        p.textures[0].png_bytes,
        prepare_portable_textures(&s, &PortableTextureLimits::default())
            .unwrap()
            .textures[0]
            .png_bytes
    );
}
#[test]
fn png8_is_validated_but_retains_exact_name_and_bytes() {
    let bytes = encode(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        2,
        1,
        Rgba([12, 34, 56, 78]),
    )));
    let s = StaticScene {
        textures: vec![StaticTexture {
            name: "unchanged".into(),
            png_bytes: bytes.clone(),
        }],
        ..Default::default()
    };
    let p = prepare_portable_textures(&s, &PortableTextureLimits::default()).unwrap();
    assert_eq!(p.textures[0].name, "unchanged");
    assert_eq!(p.textures[0].png_bytes, bytes);
    assert_eq!(p.report.images[0].changed_samples, [0; 4]);
    assert_eq!(p.report.images[0].max_channel_error, [0; 4]);
}
#[test]
fn rgb16_expands_alpha_to_opaque_without_altering_rgb_quantization() {
    let bytes = encode(DynamicImage::ImageRgb16(
        ImageBuffer::from_raw(1, 1, vec![129, 32768, 65535]).unwrap(),
    ));
    let s = StaticScene {
        textures: vec![StaticTexture {
            name: "rgb".into(),
            png_bytes: bytes,
        }],
        ..Default::default()
    };
    let p = prepare_portable_textures(&s, &PortableTextureLimits::default()).unwrap();
    assert_eq!(
        image::load_from_memory(&p.textures[0].png_bytes)
            .unwrap()
            .into_rgba8()
            .as_raw(),
        &[1, 128, 255, 255]
    );
}
#[test]
fn unused_and_shared_mask_materials_refuse_lossy_alpha_but_accept_exact_alpha() {
    let mut s = scene(&[[128, 129, 32768, 128]]);
    s.materials = vec![
        material(StaticAlphaMode::Blend),
        material(StaticAlphaMode::Mask { cutoff: 0.5 }),
    ];
    assert!(prepare_portable_textures(&s, &PortableTextureLimits::default()).is_err());
    s.textures = scene(&[[128, 129, 32768, 257]]).textures;
    assert!(prepare_portable_textures(&s, &PortableTextureLimits::default()).is_ok());
    s.materials[1].alpha_mode = StaticAlphaMode::Opaque;
    s.textures = scene(&[[128, 129, 32768, 128]]).textures;
    assert!(prepare_portable_textures(&s, &PortableTextureLimits::default()).is_ok());
}
#[test]
fn invalid_material_references_and_nonfinite_factors_or_cutoffs_refuse() {
    let mut s = scene(&[[0; 4]]);
    s.materials = vec![material(StaticAlphaMode::Opaque)];
    s.materials[0].texture_index = Some(1);
    assert!(prepare_portable_textures(&s, &PortableTextureLimits::default()).is_err());
    s.materials[0].texture_index = Some(0);
    s.materials[0].base_color[2] = f32::NAN;
    assert!(prepare_portable_textures(&s, &PortableTextureLimits::default()).is_err());
    s.materials[0].base_color = [1.; 4];
    s.materials[0].alpha_mode = StaticAlphaMode::Mask {
        cutoff: f32::INFINITY,
    };
    assert!(prepare_portable_textures(&s, &PortableTextureLimits::default()).is_err());
}
#[test]
fn every_configured_budget_refuses_and_inputs_remain_unchanged() {
    let s = scene(&[[128, 129, 32768, 257], [1, 2, 3, 4]]);
    let original = s.textures[0].png_bytes.clone();
    for field in 0..10 {
        let mut l = PortableTextureLimits::default();
        match field {
            0 => l.max_resources = 0,
            1 => l.max_single_encoded_bytes = 1,
            2 => l.max_input_png_bytes = 1,
            3 => l.max_output_png_bytes = 1,
            4 => l.max_single_decoded_bytes = 1,
            5 => l.max_decoded_bytes = 1,
            6 => l.max_dimension = 1,
            7 => l.max_name_bytes = 1,
            8 => l.max_report_bytes = 1,
            9 => l.max_report_bytes = usize::MAX,
            _ => unreachable!(),
        }
        if field == 9 {
            assert!(prepare_portable_textures(&s, &l).is_ok());
        } else {
            assert!(prepare_portable_textures(&s, &l).is_err(), "budget {field}");
        }
        assert_eq!(s.textures[0].png_bytes, original);
    }
    let mut materials = s.clone();
    materials.materials = vec![material(StaticAlphaMode::Opaque); 2];
    let l = PortableTextureLimits {
        max_resources: 1,
        ..Default::default()
    };
    assert!(prepare_portable_textures(&materials, &l).is_err());
}
#[test]
fn complete_png_validation_also_applies_to_eight_bit_passthrough() {
    let valid = encode(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        1,
        1,
        Rgba([1, 2, 3, 4]),
    )));
    let mut crc = valid.clone();
    crc[29] ^= 1;
    let mut trailing = valid.clone();
    trailing.push(0);
    for bytes in [vec![], valid[..40].to_vec(), crc, trailing] {
        let s = StaticScene {
            textures: vec![StaticTexture {
                name: "bad".into(),
                png_bytes: bytes,
            }],
            ..Default::default()
        };
        assert!(prepare_portable_textures(&s, &PortableTextureLimits::default()).is_err());
    }
}

fn alter_header(mut png: Vec<u8>, width: u32, height: u32, depth: u8) -> Vec<u8> {
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    png[24] = depth;
    let crc = crc32fast::hash(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    png
}
#[test]
fn unsupported_depth_huge_dimensions_and_invalid_pixel_payload_refuse() {
    let source = scene(&[[1, 2, 3, 4]]).textures.remove(0).png_bytes;
    let mut broken = source.clone();
    let len = u32::from_be_bytes(broken[33..37].try_into().unwrap()) as usize;
    broken[41] = 0;
    let crc = crc32fast::hash(&broken[37..41 + len]);
    broken[41 + len..45 + len].copy_from_slice(&crc.to_be_bytes());
    for png in [
        alter_header(source.clone(), 1, 1, 4),
        alter_header(source.clone(), 0, 1, 16),
        alter_header(source, u32::MAX, u32::MAX, 16),
        broken,
    ] {
        let s = StaticScene {
            textures: vec![StaticTexture {
                name: "bad".into(),
                png_bytes: png,
            }],
            ..Default::default()
        };
        let l = PortableTextureLimits {
            max_dimension: u32::MAX,
            max_single_decoded_bytes: usize::MAX,
            max_decoded_bytes: usize::MAX,
            ..Default::default()
        };
        assert!(prepare_portable_textures(&s, &l).is_err());
    }
}
#[test]
fn aggregate_image_and_report_budgets_cover_each_resource() {
    let mut s = scene(&[[1, 2, 3, 4]]);
    s.textures.push(s.textures[0].clone());
    let one = s.textures[0].png_bytes.len();
    for l in [
        PortableTextureLimits {
            max_input_png_bytes: one,
            ..Default::default()
        },
        PortableTextureLimits {
            max_decoded_bytes: 8,
            ..Default::default()
        },
        PortableTextureLimits {
            max_output_png_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(prepare_portable_textures(&s, &l).is_err());
    }
    let p = prepare_portable_textures(&s, &PortableTextureLimits::default()).unwrap();
    let n = serde_json::to_vec(&p.report).unwrap().len();
    assert!(prepare_portable_textures(
        &s,
        &PortableTextureLimits {
            max_report_bytes: n,
            ..Default::default()
        }
    )
    .is_ok());
    assert!(prepare_portable_textures(
        &s,
        &PortableTextureLimits {
            max_report_bytes: n - 1,
            ..Default::default()
        }
    )
    .is_err());
}
