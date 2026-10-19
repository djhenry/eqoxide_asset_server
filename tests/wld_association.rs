#[path = "common/wld_association_fixture.rs"]
mod fixture;
use eqoxide_asset_server::wld_association::*;
use libeq_wld::parser::{FragmentType, WldDoc};

fn inspect(document: &WldDoc) -> WldAssociationReport {
    inspect_wld_terrain_associations(document, WldAssociationLimits::default(), None).unwrap()
}
fn oracle_limits() -> WldAssociationLimits {
    // Hand-derived contract model: D8 R4 M3 N3 Q4 P2 L2 C3; three known flags.
    WldAssociationLimits {
        document_slots: 8,
        region_records: 4,
        modern_records: 3,
        tree_nodes: 3,
        flag_values: 3,
        lookup_attempts: 2,
        resolved_associations: 2,
        document_visits: 18,
        reference_visits: 13,
        histogram_comparisons: 3,
        work_units: 61,
        payload_bytes: 2220,
        resource_entries: 29,
    }
}
fn oracle_usage() -> WldAssociationUsage {
    WldAssociationUsage {
        document_slots: 8,
        region_records: 4,
        modern_records: 3,
        tree_nodes: 3,
        flag_values: 3,
        lookup_attempts: 2,
        resolved_associations: 2,
        document_visits: 18,
        reference_visits: 13,
        histogram_comparisons: 3,
        work_units: 61,
        payload_bytes: 2220,
        resource_entries: 29,
    }
}
#[test]
fn asymmetric_associations_preserve_aliases_and_disconnected_regions() {
    let document = fixture::asymmetric();
    assert!(matches!(
        document.at(0),
        Some(FragmentType::DmSpriteDef2(_))
    ));
    assert!(matches!(document.at(2), Some(FragmentType::Unknown(_))));
    let Some(FragmentType::Region(word_region)) = document.at(5) else {
        panic!("word Region");
    };
    assert_eq!(word_region.visible_lists.len(), 1);
    let encoded = libeq_wld::parser::Fragment::to_bytes(word_region);
    assert_eq!(&encoded[44..48], &[1, 0, 1, 2]); // One word value 513, not one byte.
    let context = Some([0xa5; 32]);
    let report = inspect_wld_terrain_associations(&document, oracle_limits(), context).unwrap();
    assert_eq!(report.schema, "wld-terrain-association-observations-v1");
    assert_eq!(report.context, context);
    assert_eq!(report.usage, oracle_usage());
    assert_eq!(
        report.header.stored_words,
        [0x54503d02, 0x15500, 8, 4, 0, 0, 0]
    );
    assert_eq!(
        (
            report.header.actual_document_count,
            report.header.actual_region_count
        ),
        (8, 4)
    );
    assert_eq!(
        report.header.stored_region_count_gate,
        StoredRegionCountGate::Nonzero
    );
    assert_eq!(report.header.region_count_agreement, CountAgreement::Equal);
    assert_eq!(
        report
            .regions
            .iter()
            .map(|r| (r.slot, r.ordinal))
            .collect::<Vec<_>>(),
        [(1, 1), (2, 2), (5, 3), (7, 4)]
    );
    assert_eq!(
        report
            .regions
            .iter()
            .map(|r| r.association)
            .collect::<Vec<_>>(),
        [
            AssociationStatus::ResolvedModern { slot: 0 },
            AssociationStatus::UnavailableBody,
            AssociationStatus::ResolvedModern { slot: 0 },
            AssociationStatus::NoModernField,
        ]
    );
    assert_eq!(report.regions[1].flags, None);
    assert_eq!(report.regions[1].pools, None);
    assert_eq!(report.regions[1].reference, None);
    assert_eq!(
        report.regions[2].layout,
        LayoutStatus::AcceptedWordVisibility
    );
    assert_eq!(
        report.regions[2].pools.unwrap(),
        RegionPools {
            declared: [0, 0, 0, 0, 0, 0, 0, 1],
            lengths: [0, 0, 0, 0, 0, 0, 1]
        }
    );
    assert_eq!(
        report.flag_histogram,
        [
            FlagCount {
                flags: 0x181,
                count: 1
            },
            FlagCount {
                flags: 0x101,
                count: 1
            },
            FlagCount {
                flags: 0x81,
                count: 1
            }
        ]
    );
    assert_eq!(report.unavailable_flags_count, 1);
    assert_eq!(report.histogram_capacity, 4);
    assert_eq!(
        report.modern_records,
        [
            ModernObservation {
                slot: 0,
                body: BodyStatus::Typed,
                resolved_association_count: 2
            },
            ModernObservation {
                slot: 3,
                body: BodyStatus::Typed,
                resolved_association_count: 0
            },
            ModernObservation {
                slot: 6,
                body: BodyStatus::Typed,
                resolved_association_count: 0
            },
        ]
    );
    assert_eq!(report.resolved_association_count, 2);
    assert_eq!(report.distinct_resolved_modern_target_count, 1);
    assert_eq!(report.typed_modern_zero_observed_association_count, 2);
    assert_eq!(
        report.scope,
        AssociationScope {
            accepted_layout_count: 3,
            unavailable_layout_count: 1,
            no_modern_field_count: 1,
            unresolved_region_association_count: 1,
            unavailable_body_association_count: 1,
            no_legacy_or_inline_observed_count: 3,
            unavailable_legacy_count: 1,
            ..AssociationScope::default()
        }
    );
    assert_eq!(
        report.tree,
        TreeObservation {
            original_tree_count: 1,
            selected_slot: Some(4),
            status: TreeStatus::Typed,
            declared_node_count: Some(3),
            actual_node_count: Some(3),
            node_count_agreement: Some(CountAgreement::Equal)
        }
    );
    assert_eq!(report.nodes.len(), 3);
    assert_eq!(
        report.nodes[1].region.status,
        OrdinalStatus::InRangeRegion {
            slot: 2,
            body: BodyStatus::UnknownBody
        }
    );
    assert_eq!(
        report.nodes[2].front,
        OrdinalObservation {
            reference: RawReference::Index(3),
            status: OrdinalStatus::InRangeNode
        }
    );
    assert_eq!(
        report.nodes[2].region.status,
        OrdinalStatus::InRangeRegion {
            slot: 5,
            body: BodyStatus::Typed
        }
    );
    assert_eq!(report.graph_analysis, GraphAnalysis::NotPerformed);
    for node in &report.nodes {
        assert_eq!(node.back.status, OrdinalStatus::Absent);
    }
    conservation(&report);
}
fn conservation(report: &WldAssociationReport) {
    assert_eq!(
        report.regions.len() as u64,
        report.resolved_association_count
            + report.scope.no_modern_field_count
            + report.scope.unresolved_region_association_count
    );
    assert_eq!(
        report.regions.len() as u64,
        report.flag_histogram.iter().map(|h| h.count).sum::<u64>() + report.unavailable_flags_count
    );
    assert_eq!(
        report
            .modern_records
            .iter()
            .map(|m| m.resolved_association_count)
            .sum::<u64>(),
        report.resolved_association_count
    );
}
#[test]
fn every_exact_cap_fits_and_each_one_below_refuses_atomically() {
    let document = fixture::asymmetric();
    assert_eq!(
        inspect_wld_terrain_associations(&document, oracle_limits(), None)
            .unwrap()
            .usage,
        oracle_usage()
    );
    macro_rules! boundary {
        ($($field:ident => $counter:ident),* $(,)?) => {$(
            let mut limits = oracle_limits(); limits.$field -= 1;
            assert!(matches!(inspect_wld_terrain_associations(&document, limits, None), Err(WldAssociationError::BudgetExceeded { counter: WldAssociationBudget::$counter, .. })), "{}", stringify!($field));
            let mut limits = WldAssociationLimits::default(); limits.$field = 0;
            assert!(matches!(inspect_wld_terrain_associations(&document, limits, None), Err(WldAssociationError::BudgetExceeded { counter: WldAssociationBudget::$counter, .. })), "zero {}", stringify!($field));
            let mut limits = WldAssociationLimits::default(); limits.$field += 1;
            assert_eq!(inspect_wld_terrain_associations(&document, limits, None), Err(WldAssociationError::InvalidLimits { counter: WldAssociationBudget::$counter }));
        )*}
    }
    boundary! {
        document_slots => DocumentSlots, region_records => RegionRecords, modern_records => ModernRecords,
        tree_nodes => TreeNodes, flag_values => FlagValues, lookup_attempts => LookupAttempts,
        resolved_associations => ResolvedAssociations, document_visits => DocumentVisits,
        reference_visits => ReferenceVisits, histogram_comparisons => HistogramComparisons,
        work_units => WorkUnits, payload_bytes => PayloadBytes, resource_entries => ResourceEntries,
    }
}
#[test]
fn empty_document_still_charges_fixed_capacities_but_zero_domains_fit() {
    let document = fixture::document(&[], 0);
    let limits = WldAssociationLimits {
        document_slots: 0,
        region_records: 0,
        modern_records: 0,
        tree_nodes: 0,
        flag_values: 0,
        lookup_attempts: 0,
        resolved_associations: 0,
        document_visits: 0,
        reference_visits: 0,
        histogram_comparisons: 0,
        work_units: 7,
        payload_bytes: 684,
        resource_entries: 7,
    };
    let report = inspect_wld_terrain_associations(&document, limits, None).unwrap();
    assert_eq!(
        report.usage,
        WldAssociationUsage {
            work_units: 7,
            payload_bytes: 684,
            resource_entries: 7,
            ..WldAssociationUsage::default()
        }
    );
    assert_eq!(
        report.header.stored_region_count_gate,
        StoredRegionCountGate::Zero
    );
    assert_eq!(report.header.region_count_agreement, CountAgreement::Equal);
    assert_eq!(report.tree.status, TreeStatus::Missing);
    for limits in [
        WldAssociationLimits {
            work_units: 6,
            ..limits
        },
        WldAssociationLimits {
            payload_bytes: 683,
            ..limits
        },
        WldAssociationLimits {
            resource_entries: 6,
            ..limits
        },
    ] {
        assert!(matches!(
            inspect_wld_terrain_associations(&document, limits, None),
            Err(WldAssociationError::BudgetExceeded { .. })
        ));
    }
}
#[test]
fn differently_ordered_model_counts_every_comparison_and_alias() {
    // D10 R5 M3 N2 Q5 P3 L3; flags scan lengths 0+1+1+2+2 => C6.
    let document = fixture::document(
        &[
            (0x22, fixture::region(0x101, 7, true)),
            (0x21, fixture::tree(&[[0, 0, 5], [2, 0, 1]])),
            (0x36, fixture::modern()),
            (0x22, fixture::region(0x81, 0, false)),
            (0x22, fixture::region(0x101, 7, false)),
            (0x36, fixture::modern()),
            (0x36, fixture::modern()),
            (0x22, fixture::region(0x181, 3, false)),
            (0x22, fixture::region(0x81, 0, false)),
            (0x99, vec![]),
        ],
        99,
    );
    let report = inspect(&document);
    assert_eq!(
        report.usage,
        WldAssociationUsage {
            document_slots: 10,
            region_records: 5,
            modern_records: 3,
            tree_nodes: 2,
            flag_values: 3,
            lookup_attempts: 3,
            resolved_associations: 3,
            document_visits: 23,
            reference_visits: 11,
            histogram_comparisons: 6,
            work_units: 70,
            payload_bytes: 2412,
            resource_entries: 32
        }
    );
    assert_eq!(
        report.flag_histogram,
        [
            FlagCount {
                flags: 0x101,
                count: 2
            },
            FlagCount {
                flags: 0x81,
                count: 2
            },
            FlagCount {
                flags: 0x181,
                count: 1
            }
        ]
    );
    assert_eq!(
        report.header.region_count_agreement,
        CountAgreement::Different
    );
    assert_eq!(
        report
            .modern_records
            .iter()
            .map(|r| r.resolved_association_count)
            .collect::<Vec<_>>(),
        [1, 0, 2]
    );
    assert_eq!(
        report.nodes[0].region.status,
        OrdinalStatus::InRangeRegion {
            slot: 8,
            body: BodyStatus::Typed
        }
    );
    assert_eq!(
        report.nodes[1].region.status,
        OrdinalStatus::InRangeRegion {
            slot: 0,
            body: BodyStatus::Typed
        }
    );
    conservation(&report);
}
#[test]
fn document_target_classes_use_exact_typed_variants_and_charge_attempts() {
    let cases = [
        (1, AssociationStatus::ResolvedModern { slot: 0 }),
        (2, AssociationStatus::UnknownModernTarget { slot: 1 }),
        (3, AssociationStatus::OlderGeometryTarget { slot: 2 }),
        (
            4,
            AssociationStatus::WrongKindTarget {
                slot: 3,
                original_kind: 0x2c,
                body: TargetBody::Typed(TypedVariant::BmInfoRtk),
            },
        ),
        (
            5,
            AssociationStatus::WrongKindTarget {
                slot: 4,
                original_kind: 0x2c,
                body: TargetBody::UnknownBody,
            },
        ),
        (
            6,
            AssociationStatus::WrongKindTarget {
                slot: 5,
                original_kind: 0x21,
                body: TargetBody::Typed(TypedVariant::WorldTree),
            },
        ),
        (
            7,
            AssociationStatus::WrongKindTarget {
                slot: 6,
                original_kind: 0x99,
                body: TargetBody::UnknownBody,
            },
        ),
        (999, AssociationStatus::MissingTarget { slot: 998 }),
        (
            i32::MAX,
            AssociationStatus::MissingTarget { slot: 2147483646 },
        ),
        (0, AssociationStatus::NullRequiredReference),
        (-1, AssociationStatus::NamedUnresolved),
        (i32::MIN, AssociationStatus::NamedUnresolved),
    ];
    for (reference, expected) in cases {
        let document = fixture::document(
            &[
                (0x36, fixture::modern()),
                (0x36, vec![]),
                (0x2c, fixture::older_geometry()),
                (0x2c, fixture::alternate_2c()),
                (0x2c, vec![]),
                (0x21, fixture::tree(&[])),
                (0x99, vec![]),
                (0x22, fixture::region(0x181, reference, false)),
            ],
            0,
        );
        assert!(matches!(document.at(2), Some(FragmentType::DmSpriteDef(_))));
        assert!(matches!(document.at(3), Some(FragmentType::BmInfoRtk(_))));
        let report = inspect(&document);
        assert_eq!(report.regions[0].association, expected);
        assert_eq!(
            report.regions[0].reference,
            Some(if reference > 0 {
                RawReference::Index(reference as u32)
            } else {
                RawReference::Name(reference)
            })
        );
        let observed_class_count = match expected {
            AssociationStatus::ResolvedModern { .. } => report.resolved_association_count,
            AssociationStatus::UnknownModernTarget { .. } => {
                report.scope.unknown_modern_target_count
            }
            AssociationStatus::OlderGeometryTarget { .. } => {
                report.scope.older_geometry_target_count
            }
            AssociationStatus::WrongKindTarget { .. } => report.scope.wrong_kind_target_count,
            AssociationStatus::MissingTarget { .. } => report.scope.missing_target_count,
            AssociationStatus::NullRequiredReference => report.scope.null_required_reference_count,
            AssociationStatus::NamedUnresolved => report.scope.named_unresolved_count,
            _ => panic!("unexpected case"),
        };
        assert_eq!(observed_class_count, 1);
        let p = u64::from(reference > 0);
        let l = u64::from(reference == 1);
        assert_eq!(
            report.usage,
            WldAssociationUsage {
                document_slots: 8,
                region_records: 1,
                modern_records: 2,
                flag_values: 1,
                lookup_attempts: p,
                resolved_associations: l,
                document_visits: 16 + p,
                reference_visits: 1,
                work_units: 35 + p + l,
                payload_bytes: 1084,
                resource_entries: 19,
                ..WldAssociationUsage::default()
            }
        );
        assert_eq!(report.modern_records[1].body, BodyStatus::UnknownBody);
        assert_eq!(report.modern_records[1].resolved_association_count, 0);
        assert_eq!(
            report.header.stored_region_count_gate,
            StoredRegionCountGate::Zero
        );
        assert_eq!(
            report.header.region_count_agreement,
            CountAgreement::Different
        );
        conservation(&report);
    }
}
#[test]
fn unsupported_layout_does_not_trust_shifted_modern_reference() {
    // The parser consumes the first word as a reference; the unsupported preceding
    // field and the following genuine reference must never authorize a lookup.
    for flags in [0x1c1, 0x141, 0x1e1, 0x180, 0x100, 0] {
        let mut region = fixture::region(flags, 1, false);
        region.extend(777i32.to_le_bytes());
        let document = fixture::document(&[(0x36, fixture::modern()), (0x22, region)], 1);
        let report = inspect(&document);
        assert_eq!(report.regions[0].flags, Some(flags));
        assert_eq!(
            report.regions[0].association,
            AssociationStatus::UnsupportedLayout
        );
        assert_eq!(report.regions[0].reference, None);
        assert_eq!(report.usage.lookup_attempts, 0);
        assert_eq!(report.scope.unsupported_layout_count, 1);
        assert_eq!(report.typed_modern_zero_observed_association_count, 1);
        conservation(&report);
    }
}
#[test]
fn unknown_last_tree_blocks_fallback_and_missing_tree_keeps_links() {
    for suffix in [
        vec![],
        vec![(0x21, fixture::tree(&[[0, 0, 1]])), (0x21, vec![])],
    ] {
        let mut fragments = vec![
            (0x36, fixture::modern()),
            (0x22, fixture::region(0x181, 1, false)),
        ];
        fragments.extend(suffix);
        let report = inspect(&fixture::document(&fragments, 1));
        assert_eq!(report.resolved_association_count, 1);
        assert!(report.nodes.is_empty());
        assert_eq!(report.usage.tree_nodes, 0);
        if fragments.len() == 2 {
            assert_eq!(report.tree.status, TreeStatus::Missing);
        } else {
            assert_eq!(
                report.tree,
                TreeObservation {
                    original_tree_count: 2,
                    selected_slot: Some(3),
                    status: TreeStatus::UnknownBody,
                    declared_node_count: None,
                    actual_node_count: None,
                    node_count_agreement: None
                }
            );
        }
    }
}
#[test]
fn later_typed_tree_uses_actual_nodes_and_region_ordinal_bounds() {
    let document = fixture::document(
        &[
            (0x22, vec![]),
            (0x22, fixture::region(0x81, 0, false)),
            (0x21, fixture::tree(&[[99, 99, 99]; 8])),
            (0x21, fixture::tree(&[[3, -2, 3], [2, 0, 1]])),
        ],
        200,
    );
    let mut limits = WldAssociationLimits::default();
    limits.tree_nodes = 2;
    let report = inspect_wld_terrain_associations(&document, limits, None).unwrap();
    assert_eq!(report.tree.original_tree_count, 2);
    assert_eq!(report.tree.selected_slot, Some(3));
    assert_eq!(report.nodes[0].front.status, OrdinalStatus::OutOfRange);
    assert_eq!(
        report.nodes[0].back,
        OrdinalObservation {
            reference: RawReference::Name(-2),
            status: OrdinalStatus::InvalidOrdinalDomain
        }
    );
    assert_eq!(report.nodes[0].region.status, OrdinalStatus::OutOfRange);
    assert_eq!(
        report.nodes[1].region.status,
        OrdinalStatus::InRangeRegion {
            slot: 0,
            body: BodyStatus::UnknownBody
        }
    );
    assert_eq!(report.nodes[1].front.status, OrdinalStatus::InRangeNode);
    assert_eq!(report.graph_analysis, GraphAnalysis::NotPerformed);
}
#[test]
fn inline_geometry_coverage_does_not_suppress_modern_links() {
    for flags in [0x81, 0x181] {
        let mut region = fixture::region(flags, 1, false);
        region[12..16].copy_from_slice(&1u32.to_le_bytes());
        region.splice(44..44, [0u8; 12]);
        let document = fixture::document(&[(0x36, fixture::modern()), (0x22, region)], 1);
        let report = inspect(&document);
        assert_eq!(
            report.regions[0].legacy_coverage,
            LegacyCoverage::LegacyOrInlinePresent
        );
        assert_eq!(report.scope.legacy_or_inline_count, 1);
        assert_eq!(report.regions[0].pools.unwrap().lengths[0], 1);
        assert_eq!(report.resolved_association_count, u64::from(flags == 0x181));
    }
}
#[test]
fn inspection_preserves_source_and_is_deterministic() {
    let document = fixture::asymmetric();
    let before = document.to_bytes();
    let digest = blake3::hash(&before);
    let first = inspect(&document);
    let second = inspect(&document);
    assert_eq!(first, second);
    assert_eq!(blake3::hash(&document.to_bytes()), digest);
    assert_eq!(document.to_bytes(), before);
}

#[test]
fn histogram_capacity_saturates_at_256_and_counts_unused_reservations() {
    let mut fragments: Vec<_> = (0..256)
        .map(|index| (0x22, fixture::region(0x81 | (index << 9), 0, false)))
        .collect();
    fragments.push((0x22, fixture::region(0x81, 0, false)));
    let report = inspect(&fixture::document(&fragments, 257));
    assert_eq!(report.histogram_capacity, 256);
    assert_eq!(report.flag_histogram.len(), 256);
    assert_eq!(
        report.flag_histogram[0],
        FlagCount {
            flags: 0x81,
            count: 2
        }
    );
    assert_eq!(
        report.flag_histogram[255],
        FlagCount {
            flags: 0x1fe81,
            count: 1
        }
    );
    // 0+1+...+255 comparisons plus one equality on the final repeated value.
    assert_eq!(
        report.usage,
        WldAssociationUsage {
            document_slots: 257,
            region_records: 257,
            flag_values: 256,
            document_visits: 514,
            reference_visits: 257,
            histogram_comparisons: 32641,
            work_units: 33933,
            payload_bytes: 72628,
            resource_entries: 777,
            ..WldAssociationUsage::default()
        }
    );
    fragments[256] = (0x22, fixture::region(0x20081, 0, false));
    assert!(matches!(
        inspect_wld_terrain_associations(
            &fixture::document(&fragments, 257),
            WldAssociationLimits::default(),
            None
        ),
        Err(WldAssociationError::BudgetExceeded {
            counter: WldAssociationBudget::FlagValues,
            attempted: 257,
            limit: 256,
            ..
        })
    ));
    let unknowns = vec![(0x22, vec![]); 257];
    let report = inspect(&fixture::document(&unknowns, 257));
    assert_eq!(report.histogram_capacity, 256);
    assert_eq!(report.usage.payload_bytes, 72628);
    assert_eq!(report.usage.resource_entries, 777);
    assert_eq!(report.usage.flag_values, 0);
    assert_eq!(report.unavailable_flags_count, 257);
    conservation(&report);
}

#[test]
fn header_words_are_evidence_without_magic_or_version_admission() {
    let mut bytes = fixture::bytes(&[], 99);
    let words = [0x12345678u32, 0x55667788, 0, 99, 42, 0, 17];
    for (chunk, word) in bytes.chunks_exact_mut(4).zip(words) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    let document = WldDoc::parse(&bytes).unwrap();
    let report = inspect(&document);
    assert_eq!(report.header.stored_words, words);
    assert_eq!(
        report.header.stored_region_count_gate,
        StoredRegionCountGate::Nonzero
    );
    assert_eq!(
        report.header.region_count_agreement,
        CountAgreement::Different
    );
    assert_eq!(report.graph_analysis, GraphAnalysis::NotPerformed);
}

#[test]
fn alternate_world_vertices_2c_is_not_older_geometry() {
    // Positive first word and at least fifty bytes select the alternate parser.
    let mut body = vec![0u8; 52];
    body[0..4].copy_from_slice(&1u32.to_le_bytes());
    let document = fixture::document(&[(0x2c, body), (0x22, fixture::region(0x181, 1, false))], 1);
    assert!(matches!(
        document.at(0),
        Some(FragmentType::WorldVertices(_))
    ));
    let report = inspect(&document);
    assert_eq!(
        report.regions[0].association,
        AssociationStatus::WrongKindTarget {
            slot: 0,
            original_kind: 0x2c,
            body: TargetBody::Typed(TypedVariant::WorldVertices),
        }
    );
    assert_eq!(report.scope.wrong_kind_target_count, 1);
    assert_eq!(
        report.usage,
        WldAssociationUsage {
            document_slots: 2,
            region_records: 1,
            flag_values: 1,
            lookup_attempts: 1,
            document_visits: 5,
            reference_visits: 1,
            work_units: 16,
            payload_bytes: 972,
            resource_entries: 11,
            ..WldAssociationUsage::default()
        }
    );
}
