use eqoxide_asset_server::{
    eqg::{static_collision::*, BinaryZoneScene, ZoneMesh, ZonePlacement},
    static_collision::StaticCollisionScene,
};
use glam::Mat4;
use libeq_eqg::mesh::{MeshKind, Triangle, Vertex};
fn mesh(kind: MeshKind) -> ZoneMesh {
    ZoneMesh {
        source_name: "authored".into(),
        kind,
        version: 1,
        string_table: vec![],
        materials: vec![],
        vertices: [[1., 2., 3.], [4., 2., 4.], [1., 6., 5.]]
            .map(|position| Vertex {
                position,
                normal: [f32::NAN; 3],
                color: None,
                uv0: [f32::NAN; 2],
                uv1: None,
            })
            .into(),
        triangles: vec![Triangle {
            vertex_indices: [0, 1, 2],
            material_index: u32::MAX,
            flags: 0,
        }],
        bone_count: if kind == MeshKind::Model {
            Some(0)
        } else {
            None
        },
        uv_marker: None,
        trailing_data: vec![],
    }
}
fn source() -> BinaryZoneScene {
    BinaryZoneScene {
        archive_path: "unused".into(),
        descriptor_name: "authored".into(),
        descriptor_location: "synthetic",
        version: 1,
        string_table: b"m\0p\0".to_vec(),
        model_name_offsets: vec![Some(0), Some(0), Some(0), None],
        model_slots: vec![Some(1), Some(0), Some(1), None],
        meshes: vec![mesh(MeshKind::Terrain), mesh(MeshKind::Model)],
        placements: vec![ZonePlacement {
            model_index: Some(2),
            name_offset: 2,
            position: [10., -20., 5.],
            rotation: [0.; 3],
            scale: 2.,
            extension_data: vec![],
        }],
        regions: vec![],
        lights: vec![],
        trailing_data: vec![],
    }
}
fn owners() -> CollisionOwnerBindings {
    CollisionOwnerBindings {
        terrain_owner_id: "terrain-owner".into(),
        placements: [(
            0,
            PlacementOwner::Include {
                owner_id: "object-owner".into(),
            },
        )]
        .into(),
    }
}
#[test]
fn asymmetric_positions_winding_owners_and_shared_instance_transform() {
    let s = source();
    let b = build_static_collision(&s, &owners(), &CollisionBridgeLimits::default()).unwrap();
    let StaticCollisionScene::Available { meshes, instances } = b.scene else {
        panic!("eligible source is not known-empty")
    };
    assert_eq!(meshes[0].positions[0], [2., 1., 3.]);
    assert_eq!(meshes[0].primitives[0].indices, [0, 2, 1]);
    assert_eq!(instances[0].owner_id, "terrain-owner");
    assert_eq!(instances[0].matrix, Mat4::IDENTITY.to_cols_array_2d());
    assert_eq!(instances[1].owner_id, "object-owner");
    assert_eq!(
        Mat4::from_cols_array_2d(&instances[1].matrix)
            .transform_point3(glam::Vec3::from_array(meshes[1].positions[0]))
            .to_array(),
        [-16., 12., 11.]
    );
    assert_eq!(s.meshes[0].triangles[0].vertex_indices, [0, 1, 2]);
}

fn build(s: &BinaryZoneScene, o: &CollisionOwnerBindings) -> BridgedStaticCollision {
    build_static_collision(s, o, &CollisionBridgeLimits::default()).unwrap()
}
fn refuses(s: &BinaryZoneScene, o: &CollisionOwnerBindings, needle: &str) {
    let e = build_static_collision(s, o, &CollisionBridgeLimits::default()).unwrap_err();
    assert!(format!("{e:#}").contains(needle), "{e:#}");
}
#[test]
fn collision_flags_are_independent_of_visual_attributes() {
    let mut s = source();
    s.meshes[0].triangles.extend([
        Triangle {
            vertex_indices: [0, 1, 2],
            material_index: 99,
            flags: 1,
        },
        Triangle {
            vertex_indices: [0, 1, 2],
            material_index: 99,
            flags: 2,
        },
        Triangle {
            vertex_indices: [0, 1, 2],
            material_index: 99,
            flags: 0xabcd0080,
        },
    ]);
    let b = build(&s, &owners());
    let StaticCollisionScene::Available { meshes, .. } = b.scene else {
        panic!()
    };
    assert_eq!(meshes[0].primitives[0].indices, [0, 2, 1, 0, 2, 1, 0, 2, 1]);
    assert_eq!(b.report.source_flags[&1].excluded, 1);
    assert_eq!(b.report.source_flags[&2].eligible, 1);
    assert_eq!(b.report.source_flags[&0xabcd0080].eligible, 1);
    assert!(!b.report.ownership_verified);
    assert!(!b.report.world_complete);
}
#[test]
fn reusable_slots_underground_instances_and_explicit_exclusions() {
    let mut s = source();
    s.placements.push(ZonePlacement {
        position: [-20., 30., -999.],
        ..placement()
    });
    s.placements.push(placement());
    let mut o = owners();
    o.placements.insert(
        1,
        PlacementOwner::Include {
            owner_id: "underground-owner".into(),
        },
    );
    o.placements.insert(
        2,
        PlacementOwner::Exclude {
            reason: "caller-dynamic".into(),
        },
    );
    let b = build(&s, &o);
    let StaticCollisionScene::Available { meshes, instances } = b.scene else {
        panic!()
    };
    assert_eq!(meshes.len(), 2);
    assert_eq!(instances.len(), 3);
    assert_eq!(instances[1].mesh_index, instances[2].mesh_index);
    assert_eq!(instances[2].matrix[3][2], -999.);
    assert_eq!(
        b.report.owners[3].decision,
        PlacementOwner::Exclude {
            reason: "caller-dynamic".into()
        }
    );
    assert_eq!(b.report.expanded_flags[&0].eligible, 3);
}
fn placement() -> ZonePlacement {
    ZonePlacement {
        model_index: Some(0),
        name_offset: 2,
        position: [0.; 3],
        rotation: [0.; 3],
        scale: 1.,
        extension_data: vec![],
    }
}
#[test]
fn v2_metadata_unassigned_and_terrain_records_need_no_owner_bindings() {
    let mut s = source();
    s.version = 2;
    s.placements[0].extension_data = vec![9; 4];
    s.placements.extend([
        ZonePlacement {
            model_index: None,
            ..placement()
        },
        ZonePlacement {
            model_index: Some(1),
            ..placement()
        },
    ]);
    let o = CollisionOwnerBindings {
        terrain_owner_id: "terrain-owner".into(),
        placements: Default::default(),
    };
    let b = build(&s, &o);
    let StaticCollisionScene::Available { instances, .. } = b.scene else {
        panic!()
    };
    assert_eq!(instances.len(), 1);
    assert_eq!(
        b.report
            .automatic_exclusions
            .iter()
            .map(|e| e.role)
            .collect::<Vec<_>>(),
        [
            AutomaticExclusionRole::LightingMetadata,
            AutomaticExclusionRole::Unassigned,
            AutomaticExclusionRole::RepeatedTerrain
        ]
    );
    assert_eq!(b.report.automatic_exclusions[0].extension_bytes, 4);
    let mut extra = o.clone();
    extra.placements.insert(
        0,
        PlacementOwner::Exclude {
            reason: "extra".into(),
        },
    );
    refuses(&s, &extra, "extraneous");
}
#[test]
fn owner_membership_tokens_uniqueness_and_zero_candidate_obligations() {
    let s = source();
    let mut o = owners();
    o.placements.clear();
    refuses(&s, &o, "missing owner");
    let mut o = owners();
    o.placements.insert(
        7,
        PlacementOwner::Exclude {
            reason: "extra".into(),
        },
    );
    refuses(&s, &o, "extraneous");
    for bad in ["", "BAD", "a/b", &"a".repeat(129)] {
        let mut o = owners();
        o.terrain_owner_id = bad.into();
        refuses(&s, &o, "owner identity");
    }
    let mut o = owners();
    o.placements.insert(
        0,
        PlacementOwner::Include {
            owner_id: "terrain-owner".into(),
        },
    );
    refuses(&s, &o, "duplicate owner");
    let mut s = source();
    s.meshes[1].triangles[0].flags = 1;
    refuses(&s, &o, "duplicate owner");
    let b = build(&s, &owners());
    assert!(b.report.owners[1].zero_candidates);
    assert_eq!(b.report.owners[1].instance_index, None);
    s.placements[0].scale = 0.;
    refuses(&s, &owners(), "positive scale");
    let mut o = owners();
    o.placements.insert(
        0,
        PlacementOwner::Exclude {
            reason: "NOT_VALID".into(),
        },
    );
    refuses(&source(), &o, "exclusion reason");
}
#[test]
fn known_empty_requires_complete_valid_accounting() {
    let mut s = source();
    for m in &mut s.meshes {
        m.triangles[0].flags = 1;
    }
    let b = build(&s, &owners());
    assert!(matches!(b.scene, StaticCollisionScene::AvailableEmpty));
    assert!(b.report.owners.iter().all(|o| o.zero_candidates));
    assert_eq!(b.report.work.expanded_triangles, 0);
    let mut o = owners();
    o.placements.clear();
    refuses(&s, &o, "missing owner");
    s.placements[0].extension_data = vec![0; 4];
    refuses(&s, &owners(), "v1 extension");
}
#[test]
fn malformed_unused_or_excluded_source_is_not_hidden() {
    for case in 0..13 {
        let mut s = source();
        let mut o = owners();
        match case {
            0 => s.meshes[1].triangles[0].vertex_indices[0] = 99,
            1 => {
                s.meshes[1].triangles[0].flags = 1;
                s.meshes[1].triangles[0].vertex_indices[0] = 99;
            }
            2 => {
                o.placements.insert(
                    0,
                    PlacementOwner::Exclude {
                        reason: "excluded".into(),
                    },
                );
                s.meshes[1].vertices[0].position[0] = f32::NAN;
            }
            3 => s.model_slots[0] = Some(99),
            4 => s.model_name_offsets.clear(),
            5 => s.placements[0].model_index = Some(3),
            6 => s.placements[0].model_index = Some(99),
            7 => s.placements[0].name_offset = 99,
            8 => s.meshes[1].bone_count = Some(1),
            9 => s.version = 3,
            10 => s.meshes[1].version = 4,
            11 => s.meshes[1].kind = MeshKind::Terrain,
            _ => s.meshes[0].kind = MeshKind::Model,
        };
        assert!(
            build_static_collision(&s, &o, &CollisionBridgeLimits::default()).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn required_opaque_behavior_refuses_but_declared_exclusion_retains_lengths() {
    let mut s = source();
    s.version = 2;
    s.placements.push(placement());
    let mut o = CollisionOwnerBindings {
        terrain_owner_id: "terrain-owner".into(),
        placements: [(
            1,
            PlacementOwner::Include {
                owner_id: "object-owner".into(),
            },
        )]
        .into(),
    };
    s.placements[1].extension_data = vec![0; 4];
    refuses(&s, &o, "included placement extension");
    o.placements.insert(
        1,
        PlacementOwner::Exclude {
            reason: "declared-exclusion".into(),
        },
    );
    let b = build(&s, &o);
    assert_eq!(b.report.owners[1].extension_bytes, 4);
    s.placements[1].extension_data.push(0);
    refuses(&s, &o, "incomplete extension");
    let mut s = source();
    s.trailing_data.push(0);
    refuses(&s, &owners(), "descriptor suffix");
    let mut s = source();
    s.meshes[1].trailing_data.push(0);
    refuses(&s, &owners(), "mesh suffix");
}
#[test]
fn local_and_one_instanced_world_collapse_refuse_without_filtering() {
    let mut s = source();
    s.meshes[1].triangles[0].vertex_indices = [0, 0, 1];
    refuses(&s, &owners(), "degenerate");
    let mut s = source();
    s.placements.push(ZonePlacement {
        position: [1e20; 3],
        ..placement()
    });
    let mut o = owners();
    o.placements.insert(
        1,
        PlacementOwner::Include {
            owner_id: "collapsed-owner".into(),
        },
    );
    refuses(&s, &o, "degenerate");
    assert_eq!(s.meshes[1].triangles[0].vertex_indices, [0, 1, 2]);
    let mut s = source();
    s.placements[0].scale = f32::MAX;
    assert!(build_static_collision(&s, &owners(), &CollisionBridgeLimits::default()).is_err());
}
#[test]
fn rotation_order_is_not_commuted_or_applied_twice() {
    let mut s = source();
    s.placements[0].rotation = [0.37, -0.61, 0.83];
    s.placements[0].scale = 2.25;
    let b = build(&s, &owners());
    let StaticCollisionScene::Available { meshes, instances } = b.scene else {
        panic!()
    };
    let got = Mat4::from_cols_array_2d(&instances[1].matrix)
        .transform_point3(glam::Vec3::from_array(meshes[1].positions[0]));
    // Independent scalar f64 evaluation: scale then X,Y,Z rotations then translation then X/Y swap.
    let p = &s.placements[0];
    let [mut x, mut y, mut z] = [1., 2., 3.].map(|v| v * p.scale as f64);
    let (a, c) = (p.rotation[2] as f64).sin_cos();
    (y, z) = (c * y - a * z, a * y + c * z);
    let (a, c) = (p.rotation[1] as f64).sin_cos();
    (x, z) = (c * x + a * z, -a * x + c * z);
    let (a, c) = (p.rotation[0] as f64).sin_cos();
    (x, y) = (c * x - a * y, a * x + c * y);
    for (got, want) in got.to_array().into_iter().zip([
        y + p.position[1] as f64,
        x + p.position[0] as f64,
        z + p.position[2] as f64,
    ]) {
        assert!((got as f64 - want).abs() < 1e-5);
    }
}
#[test]
fn each_work_limit_refuses_before_payload_scan_and_respects_exact_boundary() {
    let s = source();
    let o = owners();
    let b = build(&s, &o);
    for case in 0..7 {
        let mut l = CollisionBridgeLimits::default();
        match case {
            0 => l.max_input_bytes = b.report.work.input_bytes - 1,
            1 => l.max_output_bytes = b.report.work.output_bytes - 1,
            2 => l.max_resources = b.report.work.resources - 1,
            3 => l.max_vertices = 5,
            4 => l.max_indices = 5,
            5 => l.max_expanded_triangles = 1,
            _ => l.max_world_vertices = 5,
        };
        let e = build_static_collision(&s, &o, &l).unwrap_err();
        assert!(format!("{e:#}").contains("budget"), "case {case}: {e:#}");
    }
    let mut l = CollisionBridgeLimits::default();
    l.max_input_bytes = b.report.work.input_bytes;
    l.max_output_bytes = b.report.work.output_bytes;
    l.max_resources = b.report.work.resources;
    l.max_vertices = 6;
    l.max_indices = 6;
    l.max_expanded_triangles = 2;
    l.max_world_vertices = 6;
    build_static_collision(&s, &o, &l).unwrap();
    let mut hostile = source();
    hostile.meshes[0].vertices[0].position[0] = f32::NAN;
    let mut l = CollisionBridgeLimits::default();
    l.max_vertices = 0;
    let e = build_static_collision(&hostile, &o, &l).unwrap_err();
    assert!(format!("{e:#}").contains("vertex budget"), "{e:#}");
}
#[test]
fn unused_metadata_and_report_groups_are_budgeted() {
    let mut s = source();
    s.meshes.push(mesh(MeshKind::Model));
    s.meshes[2].triangles[0].flags = 0xbeef0020;
    let b = build(&s, &owners());
    assert_eq!(b.report.source_to_collision_mesh[2], None);
    assert_eq!(b.report.source_flags[&0xbeef0020].eligible, 1);
    assert!(!b.report.expanded_flags.contains_key(&0xbeef0020));
    let mut l = CollisionBridgeLimits::default();
    l.max_resources = b.report.work.resources - 1;
    assert!(build_static_collision(&s, &owners(), &l).is_err());
    s.meshes[2].materials.push(libeq_eqg::mesh::Material {
        index: 42,
        name_offset: 99,
        shader_offset: Some(99),
        properties: (0..16)
            .map(|_| libeq_eqg::mesh::Property {
                name_offset: 99,
                kind: 99,
                value: 99,
            })
            .collect(),
    });
    let b = build(&s, &owners());
    l = CollisionBridgeLimits::default();
    l.max_input_bytes = b.report.work.input_bytes - 1;
    assert!(build_static_collision(&s, &owners(), &l).is_err());
}

#[test]
fn zero_candidate_instance_diagnostics_scan_source_once() {
    let mut s = source();
    for m in &mut s.meshes {
        m.triangles[0].flags = 1;
    }
    s.meshes[1].triangles.extend((0..14).map(|_| Triangle {
        vertex_indices: [0, 1, 2],
        material_index: u32::MAX,
        flags: 1,
    }));
    let mut o = owners();
    for pi in 1..128 {
        s.placements.push(placement());
        o.placements.insert(
            pi,
            PlacementOwner::Include {
                owner_id: format!("owner-{pi}"),
            },
        );
    }
    let b = build(&s, &o);
    assert!(matches!(b.scene, StaticCollisionScene::AvailableEmpty));
    assert_eq!(b.report.work.diagnostic_triangle_visits, 16);
    assert_eq!(b.report.expanded_flags[&1].excluded, 1 + 128 * 15);
    let mut l = CollisionBridgeLimits::default();
    l.max_world_vertices = 0;
    l.max_expanded_triangles = 0;
    build_static_collision(&s, &o, &l).unwrap();
}
#[test]
fn v2_lighting_can_reference_null_slot_but_actual_even_excluded_cannot() {
    let mut s = source();
    s.version = 2;
    s.placements[0].model_index = Some(3);
    let o = CollisionOwnerBindings {
        terrain_owner_id: "terrain-owner".into(),
        placements: Default::default(),
    };
    build(&s, &o);
    s.version = 1;
    refuses(&s, &owners(), "null slot");
    let mut o = owners();
    o.placements.insert(
        0,
        PlacementOwner::Exclude {
            reason: "caller-excluded".into(),
        },
    );
    refuses(&s, &o, "null slot");
}

#[test]
fn unconsumed_regions_lights_and_raw_visual_metadata_do_not_certify_coverage() {
    let mut s = source();
    s.regions.push(libeq_eqg::zone::Region {
        name_offset: 2,
        data: [u32::MAX; 9],
    });
    s.lights.push(libeq_eqg::zone::Light {
        name_offset: 2,
        data: [u32::MAX; 7],
    });
    s.meshes[1].uv_marker = Some(u32::MAX);
    let b = build(&s, &owners());
    assert_eq!((b.report.region_count, b.report.light_count), (1, 1));
    assert!(!b.report.world_complete);
    s.regions[0].name_offset = 99;
    refuses(&s, &owners(), "string offset");
}
#[test]
fn actual_common_writer_emits_position_only_roles_and_explicit_empty() {
    use eqoxide_asset_server::static_collision::{
        write_static_collision, CollisionWriteLimits, InspectionQueryPolicy,
    };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("collision.glb");
    let b = build(&source(), &owners());
    let req = write_static_collision(
        &b.scene,
        &"a".repeat(64),
        InspectionQueryPolicy::FrontFaces,
        &out,
        &CollisionWriteLimits::default(),
    )
    .unwrap();
    assert_eq!(req.capabilities, ["static-collision-triangles-v1"]);
    let bytes = std::fs::read(&out).unwrap();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let j: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    assert_eq!(j["extras"]["eqoxideAsset"]["role"], "collision");
    assert_eq!(
        j["nodes"][0]["extras"]["eqoxideCollisionInstance"]["ownerId"],
        "terrain-owner"
    );
    assert_eq!(
        j["meshes"][0]["primitives"][0]["attributes"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["POSITION"]
    );
    assert!(j.get("materials").is_none());
    let mut invalid = source();
    invalid.placements[0].position = [1e20; 3];
    let result = build_static_collision(&invalid, &owners(), &CollisionBridgeLimits::default())
        .and_then(|b| {
            write_static_collision(
                &b.scene,
                &"a".repeat(64),
                InspectionQueryPolicy::FrontFaces,
                &out,
                &CollisionWriteLimits::default(),
            )
        });
    assert!(result.is_err());
    assert_eq!(std::fs::read(&out).unwrap(), bytes);
    let mut empty = source();
    for m in &mut empty.meshes {
        m.triangles[0].flags = 1;
    }
    let b = build(&empty, &owners());
    write_static_collision(
        &b.scene,
        &"a".repeat(64),
        InspectionQueryPolicy::BothFaces,
        &out,
        &CollisionWriteLimits::default(),
    )
    .unwrap();
    let bytes = std::fs::read(out).unwrap();
    let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 20 + len);
    let j: serde_json::Value = serde_json::from_slice(&bytes[20..]).unwrap();
    assert_eq!(
        j["extras"]["eqoxideCollision"]["availability"],
        "available-empty"
    );
    assert_eq!(j["scenes"], serde_json::json!([{}]));
    assert!(j.get("nodes").is_none());
}

#[test]
fn tiny_nonzero_candidates_are_not_dropped_by_visual_or_area_thresholds() {
    let mut s = source();
    for m in &mut s.meshes {
        for (v, p) in m
            .vertices
            .iter_mut()
            .zip([[0., 0., 0.], [1e-25, 0., 0.], [0., 1e-25, 0.]])
        {
            v.position = p;
        }
    }
    s.placements[0].position = [0.; 3];
    s.placements[0].scale = 1.;
    let b = build(&s, &owners());
    let StaticCollisionScene::Available { meshes, instances } = b.scene else {
        panic!()
    };
    assert_eq!(meshes.len(), 2);
    assert_eq!(instances.len(), 2);
}
#[test]
fn zero_candidate_owners_do_not_hide_nonfinite_or_invalid_matrices() {
    let mut s = source();
    for m in &mut s.meshes {
        m.triangles[0].flags = 1;
    }
    s.placements[0].position[1] = f32::NAN;
    refuses(&s, &owners(), "nonfinite");
    s.placements[0].position = [0.; 3];
    s.placements[0].scale = 0.;
    refuses(&s, &owners(), "positive scale");
}
