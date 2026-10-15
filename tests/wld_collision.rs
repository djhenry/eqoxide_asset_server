#[path = "common/wld_fixture.rs"]
mod fixture;
use eqoxide_asset_server::{static_collision::*, wld_collision::*};
use fixture::*;

fn normalize(doc: &libeq_wld::parser::WldDoc) -> NormalizedWldTerrain {
    normalize_selected_wld_terrain(
        WldTerrainInput {
            document: doc,
            source_context: "authored-context",
            selections: &[WldTerrainSelection {
                fragment_slot: 1,
                owner_id: "owner-a",
            }],
        },
        &WldCollisionLimits::default(),
    )
    .unwrap()
}
#[test]
fn selected_slot_axes_center_winding_and_old_new_uvs() {
    for new in [false, true] {
        let mesh = MeshFixture::default();
        let doc = document(&[(0x9999, vec![]), (0x36, mesh.bytes(new))], new);
        let normalized = normalize(&doc);
        let StaticCollisionScene::Available { meshes, instances } = normalized.scene else {
            panic!("expected geometry")
        };
        assert_eq!(
            meshes[0].positions,
            [[-18., 9., 0.], [-18., 13., 0.], [-15., 9., 0.]]
        );
        assert_eq!(meshes[0].primitives[0].indices, [0, 1, 2]);
        assert_eq!(instances[0].owner_id, "owner-a");
        assert_eq!(normalized.report.coverage.unknown, 1);
    }
}
#[test]
fn exact_source_coincidence_excludes_only_that_candidate() {
    let mut mesh = MeshFixture::default();
    mesh.positions.push(mesh.positions[0]);
    mesh.faces
        .extend([(0, [0, 3, 2]), (0, [0, 0, 1]), (0x10, [0, 3, 2])]);
    let doc = document(&[(0x9999, vec![]), (0x36, mesh.bytes(false))], false);
    let n = normalize(&doc);
    let r = &n.report.owners[0];
    assert_eq!(
        r.faces,
        WldFaceCounts {
            source_faces: 4,
            source_flag_excluded: 1,
            raw_candidates: 3,
            coincident_source_positions_excluded: 2,
            retained_faces: 1
        }
    );
    let StaticCollisionScene::Available { meshes, .. } = n.scene else {
        panic!("missing geometry")
    };
    assert_eq!(meshes[0].positions.len(), 4);
    assert_eq!(meshes[0].primitives[0].indices, [0, 1, 2]);
}

fn run(mesh: &MeshFixture) -> Result<NormalizedWldTerrain, WldCollisionError> {
    let doc = document(&[(0x36, mesh.bytes(false))], false);
    normalize_selected_wld_terrain(
        WldTerrainInput {
            document: &doc,
            source_context: "ctx",
            selections: &[WldTerrainSelection {
                fragment_slot: 0,
                owner_id: "owner-a",
            }],
        },
        &WldCollisionLimits::default(),
    )
}
#[test]
fn flags_exponents_and_nonvisual_fields_are_separate() {
    for flags in [1, 3, 0x18003, 0x14003] {
        for exponent in [0, 15] {
            let m = MeshFixture {
                flags,
                exponent,
                ..MeshFixture::default()
            };
            let n = run(&m).unwrap();
            let r = &n.report.owners[0];
            assert_eq!(r.raw_mesh_flags, flags);
            assert_eq!(r.exponent, exponent);
            assert_eq!(r.fragment3, WldSourceReference::Name(-4));
            assert_eq!(r.fragment4, WldSourceReference::Index(123456));
            let StaticCollisionScene::Available { meshes, .. } = n.scene else {
                panic!("empty")
            };
            let expected = if exponent == 0 {
                [-16., 8., -3.]
            } else {
                [-19.999878, 9.999939, 2.999817]
            };
            assert_eq!(meshes[0].positions[0], expected);
        }
    }
    for exponent in [16, 31, 32, u16::MAX] {
        assert!(matches!(
            run(&MeshFixture {
                exponent,
                ..MeshFixture::default()
            }),
            Err(WldCollisionError::UnsupportedRequiredSource { .. })
        ));
    }
    for flags in [0, 0x18002, 0x18007, 0x80018003] {
        assert!(matches!(
            run(&MeshFixture {
                flags,
                ..MeshFixture::default()
            }),
            Err(WldCollisionError::UnsupportedRequiredSource { .. })
        ));
    }
    for center in [
        [f32::NAN, 0., 0.],
        [0., f32::INFINITY, 0.],
        [0., 0., f32::NEG_INFINITY],
    ] {
        assert!(matches!(
            run(&MeshFixture {
                center,
                ..MeshFixture::default()
            }),
            Err(WldCollisionError::InvalidGeometry { .. })
        ));
    }
    for animation in [-1, 1, i32::MAX] {
        assert!(matches!(
            run(&MeshFixture {
                animation,
                ..MeshFixture::default()
            }),
            Err(WldCollisionError::UnsupportedRequiredSource { .. })
        ));
    }
}
#[test]
fn source_and_reconstructed_collinearity_refuse_without_small_area_cutoff() {
    let raw_collinear = MeshFixture {
        positions: vec![[0, 0, 0], [1, 1, 0], [2, 2, 0]],
        exponent: 0,
        center: [16777216., 0., 0.],
        ..MeshFixture::default()
    };
    assert!(matches!(
        run(&raw_collinear),
        Err(WldCollisionError::InvalidGeometry { .. })
    ));
    let reconstructed = MeshFixture {
        positions: vec![[0, 0, 0], [1, 0, 0], [0, 1, 0]],
        exponent: 0,
        center: [16777216., 16777216., 0.],
        ..MeshFixture::default()
    };
    assert!(matches!(
        run(&reconstructed),
        Err(WldCollisionError::CommonGeometryRefusal { .. })
    ));
    let tiny = MeshFixture {
        exponent: 15,
        center: [0.; 3],
        positions: vec![[0, 0, 0], [1, 0, 0], [0, 1, 0]],
        ..MeshFixture::default()
    };
    assert_eq!(run(&tiny).unwrap().report.faces.retained_faces, 1);
}
#[test]
fn excluded_indexes_are_still_required_and_flag_precedes_coincidence() {
    for flags in [0, 0x10, u16::MAX] {
        for indices in [[0, 1, 65535], [65535, 1, 2], [0, 65535, 2], [0, 0, 65535]] {
            let m = MeshFixture {
                faces: vec![(flags, indices)],
                ..MeshFixture::default()
            };
            assert!(
                matches!(run(&m), Err(WldCollisionError::InvalidGeometry { .. })),
                "{flags}/{indices:?}"
            );
        }
    }
    let n = run(&MeshFixture {
        faces: vec![(0x10, [0, 0, 1])],
        ..MeshFixture::default()
    })
    .unwrap();
    assert!(matches!(n.scene, StaticCollisionScene::AvailableEmpty));
    assert_eq!(
        n.report.faces,
        WldFaceCounts {
            source_faces: 1,
            source_flag_excluded: 1,
            ..WldFaceCounts::default()
        }
    );
}
#[test]
fn known_empty_requires_valid_nonempty_fully_examined_selection() {
    for m in [
        MeshFixture {
            faces: vec![],
            ..MeshFixture::default()
        },
        MeshFixture {
            faces: vec![(0x10, [0, 1, 2])],
            ..MeshFixture::default()
        },
        MeshFixture {
            faces: vec![(0, [0, 0, 2])],
            ..MeshFixture::default()
        },
        MeshFixture {
            positions: vec![],
            faces: vec![],
            ..MeshFixture::default()
        },
    ] {
        let n = run(&m).unwrap();
        assert!(matches!(n.scene, StaticCollisionScene::AvailableEmpty));
        assert_eq!(n.report.owners.len(), 1);
        assert_eq!(n.usage.decoded_vertices, m.positions.len());
        assert_eq!(n.report.faces.source_faces, m.faces.len());
        assert!(n.usage.owned_payload_bytes > 0 && n.usage.resource_entries > 0);
        assert!(
            !n.report.ownership_verified
                && !n.report.world_complete
                && !n.report.full_native_query_preparation_applied
        );
        assert_eq!(n.report.normalization_policy, NORMALIZATION_POLICY);
    }
    let doc = document(&[], false);
    assert!(matches!(
        normalize_selected_wld_terrain(
            WldTerrainInput {
                document: &doc,
                source_context: "ctx",
                selections: &[]
            },
            &WldCollisionLimits::default()
        ),
        Err(WldCollisionError::InvalidSelection(_))
    ));
}
#[test]
fn unknown_slot_identity_duplicates_and_coverage() {
    let m = MeshFixture::default();
    let doc = document(
        &[
            (0x36, vec![]),
            (0x36, m.bytes(false)),
            (0x36, m.bytes(false)),
            (0x2c, vec![0; 68]),
            (0x15, vec![0; 20]),
            (0x14, vec![0; 28]),
            (0x35, vec![0; 4]),
        ],
        false,
    );
    let n = normalize(&doc);
    assert_eq!(
        n.report.coverage,
        WldCoverage {
            unknown: 1,
            unselected_modern_meshes: 1,
            old_meshes: 1,
            actors: 2,
            other: 1
        }
    );
    for slot in [0, 3, 4, 5, 6] {
        assert!(matches!(
            normalize_selected_wld_terrain(
                WldTerrainInput {
                    document: &doc,
                    source_context: "ctx",
                    selections: &[WldTerrainSelection {
                        fragment_slot: slot,
                        owner_id: "owner-a"
                    }]
                },
                &WldCollisionLimits::default()
            ),
            Err(WldCollisionError::UnsupportedRequiredSource { .. })
        ));
    }
    for selections in [
        vec![
            WldTerrainSelection {
                fragment_slot: 1,
                owner_id: "owner-a",
            },
            WldTerrainSelection {
                fragment_slot: 1,
                owner_id: "owner-b",
            },
        ],
        vec![
            WldTerrainSelection {
                fragment_slot: 1,
                owner_id: "owner-a",
            },
            WldTerrainSelection {
                fragment_slot: 2,
                owner_id: "owner-a",
            },
        ],
        vec![WldTerrainSelection {
            fragment_slot: 77,
            owner_id: "owner-a",
        }],
        vec![WldTerrainSelection {
            fragment_slot: 1,
            owner_id: "UPPER",
        }],
    ] {
        assert!(matches!(
            normalize_selected_wld_terrain(
                WldTerrainInput {
                    document: &doc,
                    source_context: "ctx",
                    selections: &selections
                },
                &WldCollisionLimits::default()
            ),
            Err(WldCollisionError::InvalidSelection(_))
        ));
    }
    assert!(matches!(
        normalize_selected_wld_terrain(
            WldTerrainInput {
                document: &doc,
                source_context: "not/a/token",
                selections: &[WldTerrainSelection {
                    fragment_slot: 1,
                    owner_id: "owner-a"
                }]
            },
            &WldCollisionLimits::default()
        ),
        Err(WldCollisionError::InvalidSelection(_))
    ));
}
fn exact_limits(u: WldCollisionUsage) -> WldCollisionLimits {
    WldCollisionLimits {
        document_visits: u.document_visits,
        selected_owners: u.selected_owners,
        source_vertices: u.source_vertices,
        source_faces: u.source_faces,
        retained_indices: u.retained_indices,
        source_geometry_bytes: u.source_geometry_bytes,
        owned_payload_bytes: u.owned_payload_bytes,
        resource_entries: u.resource_entries,
        face_visits: u.face_visits,
        decoded_vertices: u.decoded_vertices,
    }
}
fn lower_cases(l: WldCollisionLimits) -> Vec<(WldCollisionBudget, WldCollisionLimits)> {
    let mut cases = vec![];
    macro_rules! one {
        ($field:ident,$kind:ident) => {
            if l.$field > 0 {
                let mut lowered = l;
                lowered.$field -= 1;
                cases.push((WldCollisionBudget::$kind, lowered));
            }
        };
    }
    one!(document_visits, DocumentVisits);
    one!(selected_owners, SelectedOwners);
    one!(source_vertices, SourceVertices);
    one!(source_faces, SourceFaces);
    one!(retained_indices, RetainedIndices);
    one!(source_geometry_bytes, SourceGeometryBytes);
    one!(owned_payload_bytes, OwnedPayloadBytes);
    one!(resource_entries, ResourceEntries);
    one!(face_visits, FaceVisits);
    one!(decoded_vertices, DecodedVertices);
    cases
}
#[test]
fn exact_usage_and_each_one_below_limit_refuse_even_empty_owners() {
    for excluded in [false, true] {
        let m = MeshFixture {
            faces: vec![(if excluded { 0x10 } else { 0 }, [0, 1, 2])],
            ..MeshFixture::default()
        };
        let doc = document(&[(0x9999, vec![]), (0x36, m.bytes(false))], false);
        let n = normalize(&doc);
        assert_eq!(
            n.usage,
            WldCollisionUsage {
                document_visits: 3,
                selected_owners: 1,
                source_vertices: 3,
                source_faces: 1,
                retained_indices: if excluded { 0 } else { 3 },
                source_geometry_bytes: 26,
                owned_payload_bytes: if excluded { 8857 } else { 9080 },
                resource_entries: if excluded { 11 } else { 14 },
                face_visits: if excluded { 2 } else { 3 },
                decoded_vertices: 3
            }
        );
        let exact = exact_limits(n.usage);
        let select = [WldTerrainSelection {
            fragment_slot: 1,
            owner_id: "owner-a",
        }];
        let call = |limits: &WldCollisionLimits| {
            normalize_selected_wld_terrain(
                WldTerrainInput {
                    document: &doc,
                    source_context: "authored-context",
                    selections: &select,
                },
                limits,
            )
        };
        assert_eq!(call(&exact).unwrap().usage, n.usage);
        for (kind, limits) in lower_cases(exact) {
            assert!(
                matches!(call(&limits),Err(WldCollisionError::BudgetExceeded {budget,..}) if budget==kind),
                "{excluded}/{kind:?}"
            );
        }
    }
}
#[test]
fn every_raised_hard_limit_is_invalid_and_zero_budget_does_not_become_empty() {
    let hard = WldCollisionLimits::HARD;
    for (kind, lowered) in lower_cases(hard) {
        let mut raised = hard;
        macro_rules! raise {
            ($field:ident) => {
                if lowered.$field != hard.$field {
                    raised.$field = usize::MAX;
                }
            };
        }
        raise!(document_visits);
        raise!(selected_owners);
        raise!(source_vertices);
        raise!(source_faces);
        raise!(retained_indices);
        raise!(source_geometry_bytes);
        raise!(owned_payload_bytes);
        raise!(resource_entries);
        raise!(face_visits);
        raise!(decoded_vertices);
        let doc = document(&[], false);
        assert_eq!(
            normalize_selected_wld_terrain(
                WldTerrainInput {
                    document: &doc,
                    source_context: "ctx",
                    selections: &[]
                },
                &raised
            )
            .unwrap_err(),
            WldCollisionError::InvalidLimits(kind)
        );
    }
    let m = MeshFixture {
        faces: vec![],
        ..MeshFixture::default()
    };
    let doc = document(&[(0x36, m.bytes(false))], false);
    let limits = WldCollisionLimits {
        owned_payload_bytes: 0,
        ..hard
    };
    assert!(matches!(
        normalize_selected_wld_terrain(
            WldTerrainInput {
                document: &doc,
                source_context: "ctx",
                selections: &[WldTerrainSelection {
                    fragment_slot: 0,
                    owner_id: "owner-a"
                }]
            },
            &limits
        ),
        Err(WldCollisionError::BudgetExceeded {
            budget: WldCollisionBudget::OwnedPayloadBytes,
            ..
        })
    ));
}
#[test]
fn bounded_histogram_and_multiple_owners_conserve_counts_and_order() {
    let mut mixed = MeshFixture::default();
    mixed
        .positions
        .extend([mixed.positions[0], [100, -100, 50]]);
    mixed.faces = (0..64u16).map(|f| (f, [0, 1, 2])).collect();
    mixed
        .faces
        .extend([(0, [0, 3, 2]), (0, [1, 2, 0]), (0x10, [0, 0, 2])]);
    let empty = MeshFixture {
        faces: vec![(0, [0, 0, 2])],
        ..MeshFixture::default()
    };
    let doc = document(
        &[(0x36, mixed.bytes(false)), (0x36, empty.bytes(false))],
        false,
    );
    let select = [
        WldTerrainSelection {
            fragment_slot: 0,
            owner_id: "owner-a",
        },
        WldTerrainSelection {
            fragment_slot: 1,
            owner_id: "owner-b",
        },
    ];
    let call = |limits: &WldCollisionLimits| {
        normalize_selected_wld_terrain(
            WldTerrainInput {
                document: &doc,
                source_context: "ctx",
                selections: &select,
            },
            limits,
        )
    };
    let n = call(&WldCollisionLimits::default()).unwrap();
    assert_eq!(n.report.owners.len(), 2);
    assert_eq!(n.report.owners[0].raw_face_flags.len(), 64);
    assert_eq!(
        n.report.faces,
        WldFaceCounts {
            source_faces: 68,
            source_flag_excluded: 33,
            raw_candidates: 35,
            coincident_source_positions_excluded: 2,
            retained_faces: 33
        }
    );
    for r in &n.report.owners {
        assert_eq!(
            r.raw_face_flags.iter().map(|h| h.count).sum::<u64>(),
            r.faces.source_faces as u64
        );
        assert_eq!(
            r.faces.source_faces,
            r.faces.source_flag_excluded + r.faces.raw_candidates
        );
        assert_eq!(
            r.faces.raw_candidates,
            r.faces.coincident_source_positions_excluded + r.faces.retained_faces
        );
    }
    let StaticCollisionScene::Available { meshes, instances } = n.scene else {
        panic!("empty")
    };
    assert_eq!(meshes.len(), 1);
    assert_eq!(meshes[0].positions.len(), 5);
    assert_eq!(instances.len(), 1);
    assert_eq!(&meshes[0].primitives[0].indices[96..], [1, 2, 0]);
    let limits = WldCollisionLimits {
        resource_entries: n.usage.resource_entries - 1,
        ..WldCollisionLimits::default()
    };
    assert!(matches!(
        call(&limits),
        Err(WldCollisionError::BudgetExceeded {
            budget: WldCollisionBudget::ResourceEntries,
            ..
        })
    ));
}

#[test]
fn extreme_integer_cross_and_atomic_late_error() {
    let m = MeshFixture {
        center: [0.; 3],
        exponent: 0,
        positions: vec![
            [-32768, -32768, -32768],
            [32767, -32768, 32767],
            [-32768, 32767, 32767],
        ],
        ..MeshFixture::default()
    };
    assert_eq!(run(&m).unwrap().report.faces.retained_faces, 1);
    let good = MeshFixture::default();
    let bad = MeshFixture {
        faces: vec![(0x10, [0, 0, 999])],
        ..MeshFixture::default()
    };
    let doc = document(
        &[(0x36, good.bytes(false)), (0x36, bad.bytes(false))],
        false,
    );
    assert!(matches!(
        normalize_selected_wld_terrain(
            WldTerrainInput {
                document: &doc,
                source_context: "ctx",
                selections: &[
                    WldTerrainSelection {
                        fragment_slot: 0,
                        owner_id: "owner-a"
                    },
                    WldTerrainSelection {
                        fragment_slot: 1,
                        owner_id: "owner-b"
                    }
                ]
            },
            &WldCollisionLimits::default()
        ),
        Err(WldCollisionError::InvalidGeometry { slot: 1, .. })
    ));
}
#[test]
fn normalized_writer_output_and_failed_write_are_observable() {
    let mut mixed = MeshFixture::default();
    mixed.positions.push(mixed.positions[0]);
    mixed
        .faces
        .extend([(0, [0, 3, 2]), (0x10, [0, 1, 2]), (0, [1, 2, 0])]);
    let n = run(&mixed).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("collision.glb");
    let requirements = write_static_collision(
        &n.scene,
        &"a".repeat(64),
        InspectionQueryPolicy::FrontFaces,
        &path,
        &CollisionWriteLimits::default(),
    )
    .unwrap();
    assert_eq!(requirements.capabilities, ["static-collision-triangles-v1"]);
    let bytes = std::fs::read(&path).unwrap();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    assert_eq!(
        json["nodes"][0]["extras"]["eqoxideCollisionInstance"]["ownerId"],
        "owner-a"
    );
    assert_eq!(json["accessors"][0]["count"], 4);
    assert_eq!(json["accessors"][1]["count"], 6);
    let bin_start = 20 + json_len + 8;
    let p0: [f32; 3] = std::array::from_fn(|i| {
        f32::from_le_bytes(
            bytes[bin_start + 4 * i..bin_start + 4 * i + 4]
                .try_into()
                .unwrap(),
        )
    });
    assert_eq!(p0, [-18., 0., -9.]);
    let tight = CollisionWriteLimits {
        max_geometry_bytes: 0,
        ..CollisionWriteLimits::default()
    };
    assert!(write_static_collision(
        &n.scene,
        &"a".repeat(64),
        InspectionQueryPolicy::FrontFaces,
        &path,
        &tight
    )
    .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let empty = run(&MeshFixture {
        faces: vec![(0, [0, 0, 2])],
        ..MeshFixture::default()
    })
    .unwrap();
    write_static_collision(
        &empty.scene,
        &"a".repeat(64),
        InspectionQueryPolicy::FrontFaces,
        &path,
        &CollisionWriteLimits::default(),
    )
    .unwrap();
    let bytes = std::fs::read(path).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes[20..]).unwrap();
    assert_eq!(
        json["extras"]["eqoxideCollision"]["availability"],
        "available-empty"
    );
    assert!(json.get("meshes").is_none());
}
