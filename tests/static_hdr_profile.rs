use eqoxide_asset_server::static_scene::{
    write_static_visual_with_material_policy, write_static_visual_with_surface_profile,
    StaticAlphaMode, StaticInstance, StaticMaterial, StaticMaterialPolicy, StaticMaterialProfile,
    StaticMesh, StaticPrimitive, StaticScene, StaticSurfaceModel,
};
use serde_json::Value;

const REV: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn scene(alpha_mode: StaticAlphaMode) -> StaticScene {
    StaticScene {
        meshes: vec![StaticMesh {
            name: "triangle".into(),
            positions: vec![[0., 0., 0.], [2., 0., 0.], [0., 3., 0.]],
            normals: vec![[0., 0., 1.]; 3],
            uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
            primitives: vec![StaticPrimitive {
                indices: vec![0, 1, 2],
                material_index: 0,
                colors: Some(vec![[0.8, 0.7, 0.6, 0.5]; 3]),
            }],
        }],
        materials: vec![StaticMaterial {
            name: "surface".into(),
            base_color: [0.4, 0.5, 0.6, 0.35],
            texture_index: None,
            alpha_mode,
            double_sided: true,
        }],
        textures: vec![],
        instances: vec![StaticInstance {
            mesh_index: 0,
            matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
        }],
    }
}
fn chunks(path: &std::path::Path) -> (Value, Vec<u8>) {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[..4], b"glTF");
    let n = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
    (
        serde_json::from_slice(&b[20..20 + n]).unwrap(),
        b[28 + n..].to_vec(),
    )
}
fn caps() -> Vec<&'static str> {
    vec![
        "pbr-metallic-roughness-blend-v1",
        "pbr-metallic-roughness-factors-v1",
        "static-visual-v1",
        "vertex-rgba-v1",
    ]
}
#[test]
fn hdr_profile_preserves_straight_rgba_and_explicit_requirements() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("hdr.glb");
    let s = scene(StaticAlphaMode::Blend);
    let report = write_static_visual_with_surface_profile(
        &s,
        &StaticMaterialPolicy::default(),
        StaticMaterialProfile::ScalarPbrHdrBlendV1,
        REV,
        &p,
    )
    .unwrap();
    assert_eq!(report.requirements.reader_version, 2);
    assert_eq!(report.requirements.capabilities, caps());
    let (j, _) = chunks(&p);
    let material = &j["materials"][0];
    assert_eq!(material["alphaMode"], "BLEND");
    for (actual, expected) in material["pbrMetallicRoughness"]["baseColorFactor"]
        .as_array()
        .unwrap()
        .iter()
        .zip(s.materials[0].base_color)
    {
        assert!((actual.as_f64().unwrap() - f64::from(expected)).abs() < 1e-7);
    }
    assert_eq!(
        j["extras"]["eqoxideAsset"]["requirements"]["capabilities"],
        serde_json::json!(caps())
    );
    assert_eq!(
        j["materials"][0]["pbrMetallicRoughness"]["metallicFactor"],
        0.0
    );
    assert_eq!(
        j["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"],
        1.0
    );
}
#[test]
fn historical_profile_refuses_lit_blend_and_preserves_previous_file() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("old.glb");
    std::fs::write(&p, b"last-good").unwrap();
    let s = scene(StaticAlphaMode::Blend);
    let policy = StaticMaterialPolicy::default();
    let e = write_static_visual_with_material_policy(&s, &policy, REV, &p).unwrap_err();
    assert!(e.to_string().contains("material 0 PBR BLEND"));
    assert_eq!(std::fs::read(&p).unwrap(), b"last-good");
    assert!(write_static_visual_with_surface_profile(
        &s,
        &policy,
        StaticMaterialProfile::ScalarPbrV1,
        REV,
        &p
    )
    .is_err());
    assert_eq!(std::fs::read(&p).unwrap(), b"last-good");
}
#[test]
fn hdr_requirement_remains_explicit_for_all_unlit_or_opaque_content() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("surface.glb");
    for surface in [
        StaticSurfaceModel::Pbr {
            metallic: 0.,
            roughness: 1.,
        },
        StaticSurfaceModel::Unlit,
    ] {
        let policy = StaticMaterialPolicy {
            default: surface,
            ..StaticMaterialPolicy::default()
        };
        let s = scene(StaticAlphaMode::Opaque);
        let r = write_static_visual_with_surface_profile(
            &s,
            &policy,
            StaticMaterialProfile::ScalarPbrHdrBlendV1,
            REV,
            &p,
        )
        .unwrap();
        assert_eq!(r.requirements.capabilities, caps());
        let (j, _) = chunks(&p);
        assert_eq!(j["materials"][0]["alphaMode"], "OPAQUE");
        if surface == StaticSurfaceModel::Unlit {
            assert_eq!(
                j["extensionsRequired"],
                serde_json::json!(["KHR_materials_unlit"])
            );
        }
    }
}
#[test]
fn choosing_hdr_does_not_change_geometry_or_vertex_rgba_payload() {
    let d = tempfile::tempdir().unwrap();
    let old = d.path().join("old.glb");
    let new = d.path().join("hdr.glb");
    let s = scene(StaticAlphaMode::Opaque);
    let policy = StaticMaterialPolicy::default();
    let r = write_static_visual_with_material_policy(&s, &policy, REV, &old).unwrap();
    assert!(!r
        .requirements
        .capabilities
        .iter()
        .any(|c| c == "pbr-metallic-roughness-blend-v1"));
    write_static_visual_with_surface_profile(
        &s,
        &policy,
        StaticMaterialProfile::ScalarPbrHdrBlendV1,
        REV,
        &new,
    )
    .unwrap();
    let (a, bin_a) = chunks(&old);
    let (b, bin_b) = chunks(&new);
    assert_eq!(bin_a, bin_b);
    assert_eq!(a["materials"], b["materials"]);
    assert_eq!(a["meshes"], b["meshes"]);
    assert_eq!(a["nodes"], b["nodes"]);
}
#[test]
fn hdr_invalid_unused_material_policy_preserves_previous_file() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("old.glb");
    let mut s = scene(StaticAlphaMode::Blend);
    s.materials.push(s.materials[0].clone());
    for value in [f32::NAN, -0.1, 1.1] {
        let mut policy = StaticMaterialPolicy::default();
        policy.overrides.insert(
            1,
            StaticSurfaceModel::Pbr {
                metallic: value,
                roughness: 0.7,
            },
        );
        std::fs::write(&p, b"last-good").unwrap();
        assert!(write_static_visual_with_surface_profile(
            &s,
            &policy,
            StaticMaterialProfile::ScalarPbrHdrBlendV1,
            REV,
            &p
        )
        .is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"last-good");
    }
}
