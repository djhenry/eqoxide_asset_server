#[path = "common/wld_associated_terrain_fixture.rs"]
mod fixture;
use eqoxide_asset_server::{wld_associated_terrain::*, static_collision::StaticCollisionScene};
#[test]
fn binds_region_document_slots_without_selecting_unassociated_or_filtering_tree() {
    let document = fixture::primary();
    let bindings = [
        WldRegionOwnerBinding {
            region_slot: 2,
            owner_id: "caller-a",
        },
        WldRegionOwnerBinding {
            region_slot: 5,
            owner_id: "caller-c",
        },
    ];
    let result = normalize_associated_wld_terrain(
        WldAssociatedTerrainInput {
            document: &document,
            source_context: "binding-fixture",
            bindings: &bindings,
        },
        &WldAssociatedTerrainLimits::default(),
    )
    .expect("complete explicit associated selection");
    assert_eq!(
        result.binding_usage,
        WldBindingUsage {
            bindings: 2,
            work_units: 10,
            payload_bytes: 136,
            resource_entries: 4
        }
    );
    assert_eq!(result.normalized.report.faces.retained_faces, 2);
    assert_eq!(
        result.normalized.report.coverage.unselected_modern_meshes,
        1
    );
    assert_eq!(result.normalized.report.coverage.unknown, 1);
    assert_eq!(
        result
            .normalized
            .report
            .owners
            .iter()
            .map(|r| (r.fragment_slot, r.owner_id.as_str()))
            .collect::<Vec<_>>(),
        [(6, "caller-a"), (7, "caller-c")]
    );
    let StaticCollisionScene::Available { meshes, instances } = result.normalized.scene else {
        panic!("available")
    };
    assert_eq!(
        meshes[0].positions,
        [[-18., 9., 0.], [-18., 13., 0.], [-15., 9., 0.]]
    );
    assert_eq!(
        meshes[1].positions,
        [[202., 99., 297.], [202., 103., 297.], [205., 99., 297.]]
    );
    assert_eq!(
        meshes
            .iter()
            .map(|m| m.primitives[0].indices.as_slice())
            .collect::<Vec<_>>(),
        [&[0, 1, 2][..], &[0, 1, 2][..]]
    );
    assert_eq!(instances[1].owner_id, "caller-c");
    let u = result.association_report.usage;
    assert_eq!(
        (
            u.document_visits,
            u.reference_visits,
            u.payload_bytes,
            u.resource_entries,
            u.work_units
        ),
        (18, 6, 1756, 25, 51)
    );
    assert!(
        !result.normalized.report.ownership_verified
            && !result.normalized.report.world_complete
            && !result
                .normalized
                .report
                .full_native_query_preparation_applied
    );
}
use eqoxide_asset_server::{wld_association::*, wld_collision::*};
use libeq_wld::parser::WldDoc;
fn bindings() -> [WldRegionOwnerBinding<'static>; 2] {
    [
        WldRegionOwnerBinding {
            region_slot: 2,
            owner_id: "caller-a",
        },
        WldRegionOwnerBinding {
            region_slot: 5,
            owner_id: "caller-c",
        },
    ]
}
fn run(
    doc: &WldDoc,
    b: &[WldRegionOwnerBinding<'_>],
    l: &WldAssociatedTerrainLimits,
) -> Result<WldAssociatedTerrain, WldAssociatedTerrainError> {
    normalize_associated_wld_terrain(
        WldAssociatedTerrainInput {
            document: doc,
            source_context: "binding-fixture",
            bindings: b,
        },
        l,
    )
}
#[test]
fn differently_ordered_selection_retains_source_face_order_and_unused_positions() {
    let mesh = fixture::mesh(
        [0.; 3],
        0x18003,
        &[[-2, 4, -6], [6, 4, -6], [-2, 10, -6], [-2, 4, -6]],
        &[(0, [2, 0, 1]), (0x10, [0, 1, 2]), (0, [0, 3, 2])],
    );
    let d = fixture::document(
        &[
            (0x22, fixture::region(0x181, 3)),
            (0x9999, vec![]),
            (0x36, mesh),
        ],
        1,
    );
    let b = [WldRegionOwnerBinding {
        region_slot: 0,
        owner_id: "chosen",
    }];
    let r = run(&d, &b, &WldAssociatedTerrainLimits::default()).unwrap();
    assert_eq!(
        r.binding_usage,
        WldBindingUsage {
            bindings: 1,
            work_units: 5,
            payload_bytes: 112,
            resource_entries: 3
        }
    );
    let f = &r.normalized.report.faces;
    assert_eq!(
        (
            f.source_faces,
            f.source_flag_excluded,
            f.raw_candidates,
            f.coincident_source_positions_excluded,
            f.retained_faces
        ),
        (3, 1, 2, 1, 1)
    );
    let StaticCollisionScene::Available { meshes, .. } = r.normalized.scene else {
        panic!("available")
    };
    assert_eq!(
        meshes[0].positions,
        [
            [2., -1., -3.],
            [2., 3., -3.],
            [5., -1., -3.],
            [2., -1., -3.]
        ]
    );
    assert_eq!(meshes[0].primitives[0].indices, [2, 0, 1]);
}
#[test]
fn inactive_and_disagreeing_headers_refuse() {
    for count in [0, 2] {
        let d = fixture::document(
            &[
                (0x36, fixture::triangle([0.; 3])),
                (0x22, fixture::region(0x181, 1)),
            ],
            count,
        );
        assert!(matches!(
            run(
                &d,
                &[WldRegionOwnerBinding {
                    region_slot: 1,
                    owner_id: "a"
                }],
                &WldAssociatedTerrainLimits::default()
            ),
            Err(WldAssociatedTerrainError::HeaderProfileRefused { .. })
        ));
    }
}
#[test]
fn anomalous_region_alongside_valid_association_cannot_be_silently_omitted() {
    let mut inline = fixture::region(0x181, 1);
    inline[12..16].copy_from_slice(&1u32.to_le_bytes());
    inline.splice(44..44, [0u8; 12]);
    let cases = [
        (vec![], AssociationStatus::UnavailableBody),
        (
            fixture::region(0x40, 0),
            AssociationStatus::UnsupportedLayout,
        ),
        (inline, AssociationStatus::ResolvedModern { slot: 0 }),
        (
            fixture::region(0x181, 0),
            AssociationStatus::NullRequiredReference,
        ),
        (
            fixture::region(0x181, -1),
            AssociationStatus::NamedUnresolved,
        ),
        (
            fixture::region(0x181, i32::MIN),
            AssociationStatus::NamedUnresolved,
        ),
        (
            fixture::region(0x181, 999),
            AssociationStatus::MissingTarget { slot: 998 },
        ),
        (
            fixture::region(0x181, 3),
            AssociationStatus::WrongKindTarget {
                slot: 2,
                original_kind: 0x22,
                body: TargetBody::Typed(TypedVariant::Region),
            },
        ),
    ];
    for (body, status) in cases {
        let d = fixture::document(
            &[
                (0x36, fixture::triangle([0.; 3])),
                (0x22, body),
                (0x22, fixture::region(0x181, 1)),
            ],
            2,
        );
        let error = run(
            &d,
            &[WldRegionOwnerBinding {
                region_slot: 2,
                owner_id: "a",
            }],
            &WldAssociatedTerrainLimits::default(),
        )
        .unwrap_err();
        assert!(
            matches!(error,WldAssociatedTerrainError::RegionProfileRefused{slot:1,association,..}if association==status),
            "{error:?}"
        );
    }
}
#[test]
fn required_unknown_modern_and_older_geometry_refuse() {
    let mut older = vec![0u8; 68];
    older[..4].copy_from_slice(&(-1i32).to_le_bytes());
    for (kind, body, status) in [
        (
            0x36,
            vec![],
            AssociationStatus::UnknownModernTarget { slot: 1 },
        ),
        (
            0x2c,
            older,
            AssociationStatus::OlderGeometryTarget { slot: 1 },
        ),
    ] {
        let d = fixture::document(
            &[
                (0x36, fixture::triangle([0.; 3])),
                (kind, body),
                (0x22, fixture::region(0x181, 2)),
                (0x22, fixture::region(0x181, 1)),
            ],
            2,
        );
        let error = run(
            &d,
            &[WldRegionOwnerBinding {
                region_slot: 3,
                owner_id: "a",
            }],
            &WldAssociatedTerrainLimits::default(),
        )
        .unwrap_err();
        assert!(
            matches!(error,WldAssociatedTerrainError::RegionProfileRefused{slot:2,association,..}if association==status),
            "{error:?}"
        );
    }
}
#[test]
fn aliases_refuse_explicitly_before_selected_frontend_duplicate_slot() {
    let d = fixture::document(
        &[
            (0x36, fixture::triangle([0.; 3])),
            (0x22, fixture::region(0x181, 1)),
            (0x22, fixture::region(0x101, 1)),
        ],
        2,
    );
    let b = [
        WldRegionOwnerBinding {
            region_slot: 1,
            owner_id: "a",
        },
        WldRegionOwnerBinding {
            region_slot: 2,
            owner_id: "b",
        },
    ];
    assert_eq!(
        run(&d, &b, &WldAssociatedTerrainLimits::default()).unwrap_err(),
        WldAssociatedTerrainError::AliasedTargets {
            resolved_associations: 2,
            distinct_targets: 1
        }
    );
}
#[test]
fn missing_extra_and_wrong_region_slot_bindings_are_explicit() {
    let d = fixture::primary();
    let b = bindings();
    let l = WldAssociatedTerrainLimits::default();
    assert_eq!(
        run(&d, &b[..1], &l).unwrap_err(),
        WldAssociatedTerrainError::MissingBindings {
            expected: 2,
            actual: 1
        }
    );
    let extra = [b[0], b[1], b[0]];
    assert_eq!(
        run(&d, &extra, &l).unwrap_err(),
        WldAssociatedTerrainError::ExtraBindings {
            expected: 2,
            actual: 3
        }
    );
    for wrong in [1, 6, 5] {
        let bad = [
            WldRegionOwnerBinding {
                region_slot: wrong,
                ..b[0]
            },
            b[1],
        ];
        assert_eq!(
            run(&d, &bad, &l).unwrap_err(),
            WldAssociatedTerrainError::WrongRegionSlot {
                binding_index: 0,
                expected_slot: 2,
                actual_slot: wrong
            }
        );
    }
    let bad = [
        b[0],
        WldRegionOwnerBinding {
            region_slot: 3,
            ..b[1]
        },
    ];
    assert_eq!(
        run(&d, &bad, &l).unwrap_err(),
        WldAssociatedTerrainError::WrongRegionSlot {
            binding_index: 1,
            expected_slot: 5,
            actual_slot: 3
        }
    );
}
#[test]
fn caller_identity_and_required_geometry_validation_are_delegated() {
    let d = fixture::primary();
    for owners in [["same", "same"], ["UPPER", "fine"]] {
        let b = [
            WldRegionOwnerBinding {
                region_slot: 2,
                owner_id: owners[0],
            },
            WldRegionOwnerBinding {
                region_slot: 5,
                owner_id: owners[1],
            },
        ];
        assert!(matches!(
            run(&d, &b, &WldAssociatedTerrainLimits::default()),
            Err(WldAssociatedTerrainError::Normalization(
                WldCollisionError::InvalidSelection(_)
            ))
        ));
    }
    assert!(matches!(
        normalize_associated_wld_terrain(
            WldAssociatedTerrainInput {
                document: &d,
                source_context: "INVALID",
                bindings: &bindings()
            },
            &WldAssociatedTerrainLimits::default()
        ),
        Err(WldAssociatedTerrainError::Normalization(
            WldCollisionError::InvalidSelection(_)
        ))
    ));
    let d = fixture::document(
        &[
            (0x36, fixture::mesh([0.; 3], 0, &[], &[])),
            (0x22, fixture::region(0x181, 1)),
        ],
        1,
    );
    assert!(matches!(
        run(
            &d,
            &[WldRegionOwnerBinding {
                region_slot: 1,
                owner_id: "a"
            }],
            &WldAssociatedTerrainLimits::default()
        ),
        Err(WldAssociatedTerrainError::Normalization(
            WldCollisionError::UnsupportedRequiredSource { slot: 0, .. }
        ))
    ));
}
#[test]
fn no_associations_refuse_but_fully_excluded_positive_selection_is_available_empty() {
    let d = fixture::document(&[(0x22, fixture::region(0x81, 0))], 1);
    assert_eq!(
        run(&d, &[], &WldAssociatedTerrainLimits::default()).unwrap_err(),
        WldAssociatedTerrainError::EmptyAssociationSelection
    );
    let m = fixture::mesh(
        [0.; 3],
        0x18003,
        &[[0, 0, 0], [2, 0, 0], [0, 2, 0]],
        &[(0x10, [0, 1, 2]), (0, [0, 0, 1])],
    );
    let d = fixture::document(&[(0x22, fixture::region(0x181, 2)), (0x36, m)], 1);
    let r = run(
        &d,
        &[WldRegionOwnerBinding {
            region_slot: 0,
            owner_id: "empty",
        }],
        &WldAssociatedTerrainLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        r.normalized.scene,
        StaticCollisionScene::AvailableEmpty
    ));
    assert_eq!(r.normalized.report.owners.len(), 1);
    assert_eq!(r.normalized.report.faces.source_flag_excluded, 1);
    assert_eq!(
        r.normalized
            .report
            .faces
            .coincident_source_positions_excluded,
        1
    );
    assert_eq!(r.normalized.usage.decoded_vertices, 3);
}
#[test]
fn missing_or_cyclic_tree_does_not_filter_positive_associations() {
    for tree in [None, Some(fixture::tree(true))] {
        let mut fragments = vec![
            (0x22, fixture::region(0x181, 2)),
            (0x36, fixture::triangle([0.; 3])),
        ];
        if let Some(tree) = tree {
            fragments.push((0x21, tree));
        }
        let d = fixture::document(&fragments, 1);
        let r = run(
            &d,
            &[WldRegionOwnerBinding {
                region_slot: 0,
                owner_id: "a",
            }],
            &WldAssociatedTerrainLimits::default(),
        )
        .unwrap();
        assert_eq!(r.normalized.report.faces.retained_faces, 1);
        assert_eq!(
            r.association_report.graph_analysis,
            GraphAnalysis::NotPerformed
        );
    }
}
#[test]
fn exact_binding_caps_succeed_and_each_lowered_counter_refuses() {
    let d = fixture::primary();
    let b = bindings();
    let exact = WldBindingLimits {
        bindings: 2,
        work_units: 10,
        payload_bytes: 136,
        resource_entries: 4,
    };
    let l = WldAssociatedTerrainLimits {
        binding: exact,
        ..Default::default()
    };
    assert!(run(&d, &b, &l).is_ok());
    for (counter, cap) in [
        (WldBindingBudget::Bindings, 1),
        (WldBindingBudget::WorkUnits, 9),
        (WldBindingBudget::PayloadBytes, 135),
        (WldBindingBudget::ResourceEntries, 3),
    ] {
        let mut l = l;
        match counter {
            WldBindingBudget::Bindings => l.binding.bindings = cap,
            WldBindingBudget::WorkUnits => l.binding.work_units = cap,
            WldBindingBudget::PayloadBytes => l.binding.payload_bytes = cap,
            WldBindingBudget::ResourceEntries => l.binding.resource_entries = cap,
        };
        assert_eq!(
            run(&d, &b, &l).unwrap_err(),
            WldAssociatedTerrainError::BindingBudgetExceeded {
                counter,
                attempted: cap + 1,
                limit: cap
            }
        );
    }
}
#[test]
fn zero_and_above_hard_binding_limits_validate_before_inspection() {
    let d = fixture::primary();
    for counter in [
        WldBindingBudget::Bindings,
        WldBindingBudget::WorkUnits,
        WldBindingBudget::PayloadBytes,
        WldBindingBudget::ResourceEntries,
    ] {
        for above in [false, true] {
            let mut l = WldAssociatedTerrainLimits::default();
            let f = match counter {
                WldBindingBudget::Bindings => &mut l.binding.bindings,
                WldBindingBudget::WorkUnits => &mut l.binding.work_units,
                WldBindingBudget::PayloadBytes => &mut l.binding.payload_bytes,
                WldBindingBudget::ResourceEntries => &mut l.binding.resource_entries,
            };
            *f = if above { *f + 1 } else { 0 };
            if above
                || matches!(
                    counter,
                    WldBindingBudget::Bindings
                        | WldBindingBudget::PayloadBytes
                        | WldBindingBudget::ResourceEntries
                )
            {
                l.inspection.document_slots = 0;
            }
            let e = run(&d, &bindings(), &l).unwrap_err();
            assert!(
                if above {
                    matches!(e,WldAssociatedTerrainError::InvalidBindingLimits{counter:c}if c==counter)
                } else {
                    matches!(e,WldAssociatedTerrainError::BindingBudgetExceeded{counter:c,..}if c==counter)
                },
                "{e:?}"
            );
        }
    }
}
#[test]
fn nested_limits_are_validated_only_before_their_respective_phases() {
    let d = fixture::primary();
    let mut l = WldAssociatedTerrainLimits::default();
    l.inspection.document_slots = 0;
    assert!(matches!(
        run(&d, &bindings(), &l),
        Err(WldAssociatedTerrainError::Inspection(
            WldAssociationError::BudgetExceeded {
                counter: WldAssociationBudget::DocumentSlots,
                ..
            }
        ))
    ));
    l = WldAssociatedTerrainLimits::default();
    l.normalization.selected_owners = 0;
    assert!(matches!(
        run(&d, &bindings(), &l),
        Err(WldAssociatedTerrainError::Normalization(
            WldCollisionError::BudgetExceeded {
                budget: WldCollisionBudget::SelectedOwners,
                ..
            }
        ))
    ));
    l.normalization.selected_owners = 100001;
    let bad = fixture::document(&[(0x22, fixture::region(0x81, 0))], 0);
    assert!(matches!(
        run(&bad, &[], &l),
        Err(WldAssociatedTerrainError::HeaderProfileRefused { .. })
    ));
}
