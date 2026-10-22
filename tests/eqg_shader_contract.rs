use eqoxide_asset_server::{
    eqg::{
        collision::export_collision,
        export::export_preview,
        load_binary_zone, report,
        static_collision::{build_static_collision, CollisionBridgeLimits, CollisionOwnerBindings},
        static_scene::{
            build_static_scene, BridgeLimits, MaterialBinding, ResolvedColors, ResolvedZoneVisual,
            SecondaryUvUse,
        },
        DescriptorSource,
    },
    static_collision::StaticCollisionScene,
    static_scene::{write_static_visual, StaticAlphaMode, StaticMaterial},
};
use libeq_pfs::PfsWriter;
use std::{io::Cursor, path::PathBuf};

fn words(bytes: &mut Vec<u8>, values: impl IntoIterator<Item = u32>) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}

fn fixture(shader_word: u32) -> (tempfile::TempDir, PathBuf) {
    // Offset zero is a valid empty string, distinct from the all-ones word.
    let strings = b"\0material\0shader\0";
    let mut mesh = b"EQGT".to_vec();
    words(&mut mesh, [1, strings.len() as u32, 1, 3, 2]);
    mesh.extend(strings);
    words(&mut mesh, [42, 1, shader_word, 0]);
    for position in [[0.0f32, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]] {
        words(
            &mut mesh,
            position
                .into_iter()
                .chain([0.0, 0.0, 1.0, 0.0, 0.0])
                .map(f32::to_bits),
        );
    }
    words(&mut mesh, [0, 1, 2, 0, 0, 0, 2, 1, 0, 1]);
    let mut zone = b"EQGZ".to_vec();
    words(&mut zone, [1, 12, 1, 0, 0, 0]);
    zone.extend(b"terrain.ter\0");
    words(&mut zone, [0]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("authored.eqg");
    let mut archive = PfsWriter::create(std::fs::File::create(&path).unwrap()).unwrap();
    archive.insert("zone.zon", Cursor::new(zone)).unwrap();
    archive.insert("terrain.ter", Cursor::new(mesh)).unwrap();
    archive.finish().unwrap();
    (dir, path)
}

fn resolved() -> ResolvedZoneVisual {
    ResolvedZoneVisual {
        materials: vec![StaticMaterial {
            name: "caller-resolved".into(),
            base_color: [1.; 4],
            texture_index: None,
            alpha_mode: StaticAlphaMode::Opaque,
            double_sided: false,
        }],
        textures: vec![],
        bindings: [(
            (0, 0),
            MaterialBinding {
                material_index: 0,
                colors: ResolvedColors::Unused,
                secondary_uv: SecondaryUvUse::Unused,
            },
        )]
        .into(),
    }
}

#[test]
fn shader_report_distinguishes_unresolved_wire_word_from_valid_empty_string() {
    for (word, shader, resolved) in [
        (
            u32::MAX,
            serde_json::json!({"bytes": null, "utf8": null}),
            false,
        ),
        (0, serde_json::json!({"bytes": [], "utf8": ""}), true),
        (
            10,
            serde_json::json!({"bytes": [115,104,97,100,101,114], "utf8": "shader"}),
            true,
        ),
    ] {
        let (_dir, archive) = fixture(word);
        let scene = load_binary_zone(&archive, DescriptorSource::Archive("zone.zon")).unwrap();
        let summary = report::summary(&scene);
        let material = &summary["meshes"][0]["material_details"][0];
        assert_eq!(material["shader_offset_word"], word);
        assert_eq!(material["shader"], shader);
        assert_eq!(material["shader_string_resolved"], resolved);
    }
}

#[test]
fn unresolved_shader_refuses_visual_outputs_even_with_caller_material_override() {
    let (dir, archive) = fixture(u32::MAX);
    let scene = load_binary_zone(&archive, DescriptorSource::Archive("zone.zon")).unwrap();
    for exists in [false, true] {
        for common in [false, true] {
            let output = dir.path().join("visual.glb");
            if exists {
                std::fs::write(&output, b"existing-output").unwrap();
            }
            let result = if common {
                build_static_scene(&scene, resolved(), &BridgeLimits::default()).and_then(|b| {
                    write_static_visual(&b.scene, &"a".repeat(64), &output).map(|_| ())
                })
            } else {
                export_preview(&scene, &output).map(|_| ())
            };
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains("unresolved shader"), "{error}");
            assert!(error.contains("0xffffffff"), "{error}");
            if exists {
                assert_eq!(std::fs::read(&output).unwrap(), b"existing-output");
                std::fs::remove_file(&output).unwrap();
            } else {
                assert!(!output.exists());
            }
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }
}

#[test]
fn unreferenced_shader_words_cannot_bypass_visual_refusal() {
    for unused_mesh in [false, true] {
        let (dir, archive) = fixture(10);
        let mut scene = load_binary_zone(&archive, DescriptorSource::Archive("zone.zon")).unwrap();
        let (_unresolved_dir, unresolved_archive) = fixture(u32::MAX);
        let mut unresolved =
            load_binary_zone(&unresolved_archive, DescriptorSource::Archive("zone.zon")).unwrap();
        if unused_mesh {
            let mut mesh = unresolved.meshes.remove(0);
            mesh.kind = libeq_eqg::mesh::MeshKind::Model;
            mesh.bone_count = Some(0);
            mesh.source_name = "unused.mod".into();
            scene.meshes.push(mesh);
        } else {
            scene.meshes[0]
                .materials
                .push(unresolved.meshes[0].materials.remove(0));
        }
        for common in [false, true] {
            let output = dir.path().join("existing.glb");
            std::fs::write(&output, b"preserved").unwrap();
            let result = if common {
                build_static_scene(&scene, resolved(), &BridgeLimits::default()).map(|_| ())
            } else {
                export_preview(&scene, &output).map(|_| ())
            };
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains("unresolved shader"), "{error}");
            assert!(error.contains("0xffffffff"), "{error}");
            assert_eq!(std::fs::read(&output).unwrap(), b"preserved");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        }
    }
}

#[test]
fn valid_empty_and_named_shader_strings_still_convert() {
    for word in [0, 10] {
        let (dir, archive) = fixture(word);
        let scene = load_binary_zone(&archive, DescriptorSource::Archive("zone.zon")).unwrap();
        let preview = dir.path().join("preview.glb");
        let exported = export_preview(&scene, &preview).unwrap();
        assert_eq!(exported.materials, 1);
        assert_eq!(gltf::import(&preview).unwrap().0.meshes().count(), 1);
        let bridged = build_static_scene(&scene, resolved(), &BridgeLimits::default()).unwrap();
        let common = dir.path().join("common.glb");
        write_static_visual(&bridged.scene, &"a".repeat(64), &common).unwrap();
        assert_eq!(gltf::import(&common).unwrap().0.meshes().count(), 1);
    }
}

#[test]
fn collision_geometry_and_flag_eligibility_do_not_depend_on_shader_word() {
    for word in [u32::MAX, 0, 10] {
        let (dir, archive) = fixture(word);
        let mut scene = load_binary_zone(&archive, DescriptorSource::Archive("zone.zon")).unwrap();
        for material in [0, u32::MAX, 99] {
            for triangle in &mut scene.meshes[0].triangles {
                triangle.material_index = material;
            }
            let output = dir.path().join("collision.glb");
            let exported = export_collision(&scene, &output).unwrap();
            assert_eq!(exported.emitted_triangles, 1);
            assert_eq!(exported.placement_expanded_flags[&0].emitted, 1);
            assert_eq!(exported.placement_expanded_flags[&1].excluded, 1);
            let owners = CollisionOwnerBindings {
                terrain_owner_id: "authored-terrain".into(),
                placements: Default::default(),
            };
            let bridged =
                build_static_collision(&scene, &owners, &CollisionBridgeLimits::default()).unwrap();
            let StaticCollisionScene::Available { meshes, instances } = bridged.scene else {
                panic!("expected geometry");
            };
            assert_eq!(meshes.len(), 1);
            assert_eq!(instances.len(), 1);
            assert_eq!(
                meshes[0].positions,
                [[0., 0., 0.], [0., 2., 0.], [3., 0., 0.]]
            );
            assert_eq!(meshes[0].primitives[0].indices, [0, 2, 1]);
        }
    }
}
