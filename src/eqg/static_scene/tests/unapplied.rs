use super::*;
use crate::eqg::lighting::{
    Candidate, ConsumerScope, CountComparison, DiagnosticLimits, TargetOutcome, UnassignedReason,
};

fn lit_source() -> BinaryZoneScene {
    let mut s = source();
    s.version = 2;
    s.placements.insert(
        0,
        ZonePlacement {
            model_index: Some(0),
            name_offset: 23,
            position: [0.; 3],
            rotation: [0.; 3],
            scale: -1.,
            extension_data: vec![0x11; 12],
        },
    );
    s.placements[1].extension_data = vec![0x22; 12];
    s.placements.push(ZonePlacement {
        model_index: Some(0),
        name_offset: 23,
        position: [-11., 17., 9.],
        rotation: [-0.23, 0.49, -0.71],
        scale: 0.75,
        extension_data: vec![0x33; 12],
    });
    s.placements.push(ZonePlacement {
        model_index: None,
        name_offset: 23,
        position: [0.; 3],
        rotation: [0.; 3],
        scale: -2.,
        extension_data: vec![0x44; 8],
    });
    s.placements.push(ZonePlacement {
        model_index: Some(1),
        name_offset: 23,
        position: [0.; 3],
        rotation: [0.; 3],
        scale: 0.,
        extension_data: vec![0x55; 4],
    });
    s
}

fn preserve(s: &BinaryZoneScene) -> Result<BridgedUnappliedLightingScene<'_>> {
    build_static_scene_with_unapplied_v2_lighting(
        s,
        resolved(),
        &BridgeLimits::default(),
        &DiagnosticLimits::default(),
    )
}

// Include float bits as well as every record's Debug fields: NaN payloads and
// signed zero must survive refusal just as opaque bytes survive success.
fn snapshot(s: &BinaryZoneScene) -> (String, Vec<u32>, Vec<(*const u8, usize)>) {
    let mut bits = Vec::new();
    for p in &s.placements {
        bits.extend(
            p.position
                .iter()
                .chain(&p.rotation)
                .chain(std::iter::once(&p.scale))
                .map(|v| v.to_bits()),
        );
    }
    for m in &s.meshes {
        for v in &m.vertices {
            bits.extend(
                v.position
                    .iter()
                    .chain(&v.normal)
                    .chain(&v.uv0)
                    .chain(v.uv1.iter().flatten())
                    .map(|v| v.to_bits()),
            );
        }
    }
    (
        format!("{s:?}"),
        bits,
        s.placements
            .iter()
            .map(|p| (p.extension_data.as_ptr(), p.extension_data.len()))
            .collect(),
    )
}

fn refuses(s: &BinaryZoneScene, r: ResolvedZoneVisual, needle: &str) {
    let before = snapshot(s);
    let err = build_static_scene_with_unapplied_v2_lighting(
        s,
        r,
        &BridgeLimits::default(),
        &DiagnosticLimits::default(),
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains(needle), "{err:#}");
    assert_eq!(snapshot(s), before);
}

#[test]
fn unapplied_v2_preserves_each_borrowed_record_without_tint_or_target_guessing() {
    let s = lit_source();
    let before = snapshot(&s);
    error(&s, resolved(), "unresolved extension");
    let b = preserve(&s).unwrap();
    assert_eq!(snapshot(&s), before);
    assert_eq!(b.geometry.scene.meshes.len(), 2);
    assert_eq!(
        b.geometry.report.instance_sources,
        [
            InstanceSource::Terrain,
            InstanceSource::Placement(1),
            InstanceSource::Placement(2)
        ]
    );
    assert_eq!(
        b.geometry.scene.instances[1].mesh_index,
        b.geometry.scene.instances[2].mesh_index
    );
    assert_eq!(b.geometry.report.transform_visits, 9);
    assert!(b
        .geometry
        .scene
        .meshes
        .iter()
        .flat_map(|m| &m.primitives)
        .all(|p| p.colors.is_none()));
    assert_eq!(b.lighting.targets.len(), 5);
    assert!(b.lighting.resources.is_empty());
    assert_eq!(b.lighting.targets[0].placement_mesh_index, Some(0));
    assert_eq!(b.lighting.targets[0].target_mesh_index, Some(1));
    for (i, target) in b.lighting.targets.iter().enumerate() {
        assert_eq!(target.placement_ordinal, Some(i));
        let Candidate::Inline {
            record_ordinal,
            packed_bytes,
        } = target.candidate
        else {
            panic!("inline record lost")
        };
        assert_eq!(record_ordinal, i);
        assert_eq!(packed_bytes, s.placements[i].extension_data);
        assert_eq!(
            packed_bytes.as_ptr(),
            s.placements[i].extension_data.as_ptr()
        );
        if i < 3 {
            assert!(matches!(
                target.outcome,
                TargetOutcome::Words {
                    count: 3,
                    comparison: CountComparison::MatchesCurrentPoolOnly
                }
            ));
            assert_eq!(
                target.consumer_scope,
                ConsumerScope::CurrentPoolOnlyNativeContextUnresolved
            );
        } else {
            assert!(matches!(target.outcome, TargetOutcome::Unassigned(_)));
            assert_eq!(target.consumer_scope, ConsumerScope::UnassignedTarget);
        }
    }
    assert!(matches!(
        b.lighting.targets[3].outcome,
        TargetOutcome::Unassigned(UnassignedReason::NullModelSlot)
    ));
    let repeated = &b.lighting.targets[4];
    assert!(matches!(
        repeated.outcome,
        TargetOutcome::Unassigned(UnassignedReason::RepeatedTerrainPlacement)
    ));
    assert_eq!(repeated.target_mesh_index, Some(1));
    assert_eq!(repeated.source_mesh_name, Some("terrain.ter"));
    assert_eq!(repeated.current_pool_count, Some(3));
}

#[test]
fn unapplied_v2_geometry_matches_strict_control_through_real_writer_and_decoder() {
    let s = lit_source();
    let before = snapshot(&s);
    let b = preserve(&s).unwrap();
    let mut control = lit_source();
    for p in &mut control.placements {
        p.extension_data.clear();
    }
    let strict = build(&control);
    assert_eq!(
        format!("{:?}", b.geometry.scene),
        format!("{:?}", strict.scene)
    );
    let object = &b.geometry.scene.meshes[0];
    assert_eq!(object.positions[0], [2., 1., 3.]);
    assert_eq!(object.normals[0], [-3. / 7., -2. / 7., 6. / 7.]);
    assert_eq!(object.uvs[0], [0.25, 0.75]);
    assert_eq!(object.primitives[0].indices, [0, 2, 1]);
    for pi in [1, 2] {
        let instance = &b.geometry.scene.instances[pi];
        let got = Mat4::from_cols_array_2d(&instance.matrix)
            .transform_point3(Vec3::from_array([2., 1., 3.]));
        let expected = expected_world([1., 2., 3.], &s.placements[pi]);
        for k in 0..3 {
            assert!((got[k] as f64 - expected[k]).abs() < 1e-5);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let modern = dir.path().join("modern.glb");
    let legacy = dir.path().join("strict.glb");
    let revision = "e".repeat(64);
    crate::static_scene::write_static_visual(&b.geometry.scene, &revision, &modern).unwrap();
    crate::static_scene::write_static_visual(&strict.scene, &revision, &legacy).unwrap();
    assert_eq!(
        std::fs::read(&modern).unwrap(),
        std::fs::read(&legacy).unwrap()
    );
    let (doc, buffers, _) = gltf::import(&modern).unwrap();
    for primitive in doc.meshes().flat_map(|m| m.primitives()) {
        assert!(primitive.get(&gltf::Semantic::Colors(0)).is_none());
        let reader = primitive.reader(|b| Some(&buffers[b.index()]));
        assert_eq!(
            reader.read_positions().unwrap().next().unwrap(),
            [2., 3., -1.]
        );
        assert_eq!(
            reader
                .read_tex_coords(0)
                .unwrap()
                .into_f32()
                .next()
                .unwrap(),
            [0.25, 0.75]
        );
        assert_eq!(
            reader
                .read_indices()
                .unwrap()
                .into_u32()
                .collect::<Vec<_>>(),
            [0, 2, 1]
        );
    }
    assert!(doc
        .extensions_used()
        .all(|name| !name.to_ascii_lowercase().contains("lighting")));
    let mut changed = lit_source();
    changed.placements[2].extension_data.fill(0xfe);
    let output = preserve(&changed).unwrap();
    crate::static_scene::write_static_visual(&output.geometry.scene, &revision, &legacy).unwrap();
    assert_eq!(
        std::fs::read(&modern).unwrap(),
        std::fs::read(&legacy).unwrap()
    );
    assert_eq!(snapshot(&s), before);
}

#[test]
fn unapplied_v2_requires_each_included_nonempty_complete_current_pool() {
    for pi in [0, 1, 2] {
        for len in [0, 4, 8, 16, 11] {
            let mut s = lit_source();
            s.placements[pi].extension_data = vec![7; len];
            refuses(
                &s,
                resolved(),
                if len % 4 == 0 {
                    "current vertex pool"
                } else {
                    "word is incomplete"
                },
            );
        }
    }
    let mut s = lit_source();
    s.meshes[0].vertices.push(Vertex {
        position: [9., 8., 7.],
        normal: [0., 0., 1.],
        color: None,
        uv0: [0.; 2],
        uv1: None,
    });
    refuses(&s, resolved(), "current vertex pool");
    for pi in [3, 4] {
        let mut s = lit_source();
        s.placements[pi].extension_data.push(1);
        refuses(&s, resolved(), "word is incomplete");
    }
    let mut s = lit_source();
    s.placements.clear();
    refuses(&s, resolved(), "record zero");
    let mut s = source();
    refuses(&s, resolved(), "descriptor v2");
    s.version = 2;
    s.placements[0].extension_data = vec![7; 12];
    assert!(
        preserve(&s).is_ok(),
        "record zero still supplies terrain when its slot resolves to an object"
    );
}

#[test]
fn unapplied_v2_keeps_strict_source_resource_and_transform_guards() {
    type SourceRefusalCase = (fn(&mut BinaryZoneScene), &'static str);
    let cases: &[SourceRefusalCase] = &[
        (|s| s.meshes[0].bone_count = Some(1), "bone header"),
        (|s| s.trailing_data.push(0), "descriptor suffix"),
        (|s| s.meshes[0].trailing_data.push(0), "mesh suffix"),
        (|s| s.placements[1].scale = 0., "positive scale"),
        (
            |s| s.placements[1].scale = f32::MAX,
            "transformed positions",
        ),
        (
            |s| s.placements[3].position[0] = f32::from_bits(0x7fc01234),
            "nonfinite placement",
        ),
        (
            |s| s.placements[1].model_index = Some(99),
            "model slot out of range",
        ),
        (
            |s| s.placements[1].model_index = Some(3),
            "placed model slot is null",
        ),
        (
            |s| s.meshes[0].triangles[0].vertex_indices[0] = 3,
            "vertex out of range",
        ),
        (
            |s| s.meshes[0].triangles[0].material_index = 1,
            "material table ordinal out of range",
        ),
        (|s| s.meshes[0].triangles.clear(), "no visual primitives"),
        (
            |s| s.meshes[0].materials[0].shader_offset = None,
            "unresolved shader word",
        ),
        (|s| s.meshes[0].kind = MeshKind::Terrain, "bone header"),
        (
            |s| {
                s.meshes[1].kind = MeshKind::Model;
                s.meshes[1].bone_count = Some(0);
            },
            "missing terrain",
        ),
    ];
    for (change, needle) in cases {
        let mut s = lit_source();
        change(&mut s);
        refuses(&s, resolved(), needle);
    }
    let s = lit_source();
    let mut r = resolved();
    r.bindings.remove(&(0, 0));
    refuses(&s, r, "lacks a binding");
    let mut r = resolved();
    r.bindings.get_mut(&(0, 0)).unwrap().secondary_uv = SecondaryUvUse::Required;
    refuses(&s, r, "secondary UV");
    let mut r = resolved();
    r.materials[0].base_color[0] = f32::NAN;
    refuses(&s, r, "RGBA factor");
}

#[test]
fn unapplied_v2_refuses_rgba_even_for_unused_or_empty_bindings() {
    for unused in [false, true] {
        for colors in [vec![], vec![[0.2, 0.4, 0.6, 1.]; 3]] {
            let mut s = lit_source();
            let mut r = resolved();
            let key = if unused {
                s.meshes[0].materials.push(Material {
                    index: 777,
                    name_offset: 0,
                    shader_offset: Some(2),
                    properties: vec![],
                });
                r.bindings.insert((0, 1), binding());
                (0, 1)
            } else {
                (0, 0)
            };
            r.bindings.get_mut(&key).unwrap().colors = ResolvedColors::Rgba(colors);
            refuses(&s, r, "resolved vertex colors");
        }
    }
}

#[test]
fn unapplied_v2_bridge_and_diagnostic_limits_are_independent_and_include_exclusions() {
    let s = lit_source();
    let before = snapshot(&s);
    let bridge = BridgeLimits {
        max_vertices: 6,
        max_indices: 6,
        max_resources: 20,
        max_transform_visits: 9,
        ..BridgeLimits::default()
    };
    let diagnostic = DiagnosticLimits {
        max_targets: 5,
        max_words: 12,
        max_resources: 0,
        max_logical_records: 37,
        ..DiagnosticLimits::default()
    };
    assert!(
        build_static_scene_with_unapplied_v2_lighting(&s, resolved(), &bridge, &diagnostic).is_ok()
    );
    for field in 0..6 {
        let mut b = bridge.clone();
        match field {
            0 => b.max_vertices -= 1,
            1 => b.max_indices -= 1,
            2 => b.max_resources -= 1,
            3 => b.max_transform_visits -= 1,
            4 => b.max_input_bytes = 1,
            _ => b.max_output_bytes = 1,
        }
        assert!(
            build_static_scene_with_unapplied_v2_lighting(&s, resolved(), &b, &diagnostic).is_err()
        );
    }
    for field in 0..5 {
        let mut d = diagnostic;
        match field {
            0 => d.max_targets -= 1,
            1 => d.max_words -= 1,
            2 => d.max_logical_records -= 1,
            3 => d.max_input_bytes = 1,
            _ => d.max_name_bytes = 1,
        }
        assert!(
            build_static_scene_with_unapplied_v2_lighting(&s, resolved(), &bridge, &d).is_err()
        );
    }
    // Discover exact logical byte/name thresholds, then prove one unit less refuses.
    for field in 0..4 {
        let (mut lo, mut hi) = (0, 8192);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let mut b = bridge.clone();
            let mut d = diagnostic;
            match field {
                0 => b.max_input_bytes = mid,
                1 => b.max_output_bytes = mid,
                2 => d.max_input_bytes = mid,
                _ => d.max_name_bytes = mid,
            }
            if build_static_scene_with_unapplied_v2_lighting(&s, resolved(), &b, &d).is_ok() {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        assert!(lo > 0 && lo < 8192);
        for (value, expected) in [(lo, true), (lo - 1, false)] {
            let mut b = bridge.clone();
            let mut d = diagnostic;
            match field {
                0 => b.max_input_bytes = value,
                1 => b.max_output_bytes = value,
                2 => d.max_input_bytes = value,
                _ => d.max_name_bytes = value,
            }
            assert_eq!(
                build_static_scene_with_unapplied_v2_lighting(&s, resolved(), &b, &d).is_ok(),
                expected
            );
        }
    }
    assert_eq!(snapshot(&s), before);
    let mut more = lit_source();
    more.placements[3].extension_data.extend([1; 4]);
    assert!(
        build_static_scene_with_unapplied_v2_lighting(&more, resolved(), &bridge, &diagnostic)
            .is_err()
    );
}

#[test]
fn unapplied_v2_bounds_binding_count_before_color_policy_work() {
    let s = lit_source();
    let mut r = resolved();
    r.bindings.get_mut(&(0, 0)).unwrap().colors = ResolvedColors::Rgba(vec![]);
    let limits = BridgeLimits {
        max_resources: 0,
        ..BridgeLimits::default()
    };
    let err =
        build_static_scene_with_unapplied_v2_lighting(&s, r, &limits, &DiagnosticLimits::default())
            .unwrap_err();
    assert!(
        format!("{err:#}").contains("resource count budget exceeded"),
        "{err:#}"
    );
}
