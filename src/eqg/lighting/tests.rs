use super::*;
use crate::eqg::{ZoneMesh, ZonePlacement};
use libeq_eqg::mesh::{MeshKind, Vertex};

fn mesh(name: &str, kind: MeshKind) -> ZoneMesh {
    ZoneMesh {
        source_name: name.into(),
        kind,
        version: 1,
        string_table: vec![],
        materials: vec![],
        vertices: (0..3)
            .map(|_| Vertex {
                position: [0.; 3],
                normal: [0.; 3],
                color: None,
                uv0: [0.; 2],
                uv1: None,
            })
            .collect(),
        triangles: vec![],
        bone_count: None,
        uv_marker: None,
        trailing_data: vec![],
    }
}
fn scene(version: u32) -> BinaryZoneScene {
    BinaryZoneScene {
        archive_path: "different.eqg".into(),
        descriptor_name: "different.zon".into(),
        descriptor_location: "synthetic",
        version,
        string_table: b"object.mod\0Actual.TER\0instance\0other\0".to_vec(),
        model_name_offsets: vec![Some(0), Some(11), Some(0), None],
        model_slots: vec![Some(0), Some(1), Some(0), None],
        meshes: vec![
            mesh("object.mod", MeshKind::Model),
            mesh("Actual.TER", MeshKind::Terrain),
        ],
        placements: vec![placement(Some(2), 22), placement(Some(0), 31)],
        regions: vec![],
        lights: vec![],
        trailing_data: vec![],
    }
}
fn placement(model_index: Option<u32>, name_offset: u32) -> ZonePlacement {
    ZonePlacement {
        model_index,
        name_offset,
        position: [0.; 3],
        rotation: [0.; 3],
        scale: 1.,
        extension_data: vec![],
    }
}
fn classic(words: &[u32], suffix: &[u8]) -> Vec<u8> {
    let mut b = b"EQGP".to_vec();
    b.extend_from_slice(&(words.len() as u32).to_le_bytes());
    for word in words {
        b.extend_from_slice(&word.to_le_bytes());
    }
    b.extend_from_slice(suffix);
    b
}
fn resource<'a>(name: &'a str, bytes: &'a [u8]) -> ExternalResource<'a> {
    ExternalResource {
        name,
        provenance: "explicit",
        bytes,
    }
}
fn run<'a>(s: &'a BinaryZoneScene, inputs: &[ExternalResource<'a>]) -> LightingDiagnostics<'a> {
    diagnose(s, inputs, &DiagnosticLimits::default()).unwrap()
}
fn words(target: &TargetDiagnostic<'_>, count: usize, cmp: CountComparison) {
    assert!(
        matches!(target.outcome, TargetOutcome::Words {count:n,comparison:c} if n==count && c==cmp),
        "{:?}",
        target.outcome
    );
}
#[test]
fn terrain_member_and_raw_placement_names_preserve_deduplicated_identity() {
    let s = scene(1);
    let b = classic(&[0xffffffff, 0x01234567, 0x80000000], b"suffix");
    let r = run(
        &s,
        &[resource("actual.lit", &b), resource("instance.LIT", &b)],
    );
    assert_eq!(r.targets.len(), 3);
    assert_eq!(r.targets[0].source_mesh_name, Some("Actual.TER"));
    words(&r.targets[0], 3, CountComparison::MatchesCurrentPoolOnly);
    assert_eq!(r.targets[1].raw_placement_name, Some(&b"instance"[..]));
    assert_eq!(r.targets[1].original_model_slot, Some(2));
    assert_eq!(r.targets[1].placement_mesh_index, Some(0));
    assert_eq!(r.targets[2].original_model_slot, Some(0));
    assert_eq!(r.targets[2].placement_mesh_index, Some(0));
    assert!(
        matches!(&r.targets[2].candidate,Candidate::MissingInExplicitInputs {requested_name} if requested_name=="other.LIT")
    );
    let parsed = r.resources[0].parsed.as_ref().unwrap();
    assert_eq!(parsed.get(0), Some(0xffffffff));
    assert_eq!(
        parsed.packed_words().collect::<Vec<_>>(),
        vec![0xffffffff, 0x01234567, 0x80000000]
    );
    assert_eq!(parsed.trailing_data(), b"suffix");
    assert!(std::ptr::eq(
        parsed.packed_bytes().as_ptr(),
        b[8..].as_ptr()
    ));
}
#[test]
fn version_two_record_zero_suppresses_external_for_empty_and_mismatched_words() {
    let b = classic(&[1, 2, 3], b"");
    let mut s = scene(2);
    s.placements[1].extension_data = 0xFEDCBA98u32.to_le_bytes().to_vec();
    let r = run(&s, &[resource("actual.lit", &b), resource("other.lit", &b)]);
    assert_eq!(r.targets.len(), 2);
    words(&r.targets[0], 0, CountComparison::Empty);
    assert_eq!(r.targets[0].placement_ordinal, Some(0));
    assert_eq!(r.targets[0].placement_mesh_index, Some(0));
    assert_eq!(r.targets[0].target_mesh_index, Some(1));
    assert!(matches!(
        r.targets[0].candidate,
        Candidate::Inline {
            record_ordinal: 0,
            ..
        }
    ));
    words(&r.targets[1], 1, CountComparison::DiffersFromCurrentPool);
    match r.targets[1].candidate {
        Candidate::Inline { packed_bytes, .. } => {
            assert_eq!(packed_bytes, &0xFEDCBA98u32.to_le_bytes())
        }
        _ => panic!(),
    };
    s.placements[0].extension_data = vec![0; 4];
    let r = run(&s, &[resource("actual.lit", &b)]);
    words(&r.targets[0], 1, CountComparison::DiffersFromCurrentPool);
}
#[test]
fn explicit_inputs_reject_duplicate_case_keys_even_unused() {
    let s = scene(1);
    let b = classic(&[], b"");
    let e = diagnose(
        &s,
        &[resource("unused.LIT", &b), resource("UNUSED.lit", &b)],
        &DiagnosticLimits::default(),
    )
    .unwrap_err();
    assert!(e.to_string().contains("ambiguous"));
}
#[test]
fn classic_invalid_headerless_empty_mismatch_and_suffix_are_distinct() {
    let s = scene(1);
    let empty = classic(&[], b"opaque");
    let short = classic(&[7], b"");
    let r = run(
        &s,
        &[
            resource("Actual.LIT", &empty),
            resource("instance.LIT", &[1, 2, 3, 4]),
            resource("other.LIT", &short),
        ],
    );
    words(&r.targets[0], 0, CountComparison::Empty);
    assert!(matches!(
        r.targets[1].outcome,
        TargetOutcome::InvalidOrUnsupportedClassicResource
    ));
    assert!(r.resources[1].parsed.is_err());
    words(&r.targets[2], 1, CountComparison::DiffersFromCurrentPool);
    assert_eq!(
        r.resources[0].parsed.as_ref().unwrap().trailing_data(),
        b"opaque"
    );
}
#[test]
fn raw_non_utf8_and_path_names_are_preserved_but_refused() {
    let mut s = scene(1);
    s.string_table[22] = 0xff;
    let r = run(&s, &[]);
    assert_eq!(r.targets[1].raw_placement_name.unwrap()[0], 0xff);
    assert!(matches!(
        r.targets[1].candidate,
        Candidate::RefusedName(NameRefusal::NonUtf8)
    ));
    s.string_table[22] = b'/';
    let r = run(&s, &[]);
    assert!(matches!(
        r.targets[1].candidate,
        Candidate::RefusedName(NameRefusal::UnsafePathOrControl)
    ));
    s.meshes[1].source_name = "Actual.mod".into();
    let r = run(&s, &[]);
    assert!(matches!(
        r.targets[0].candidate,
        Candidate::RefusedName(NameRefusal::InvalidTerrainExtension)
    ));
}
#[test]
fn nullable_targets_missing_record_zero_and_repeated_terrain_are_explicit() {
    let mut s = scene(1);
    s.placements[0].model_index = None;
    s.placements[1].model_index = Some(3);
    let r = run(&s, &[]);
    assert!(matches!(
        r.targets[1].outcome,
        TargetOutcome::Unassigned(UnassignedReason::NullModelSlot)
    ));
    assert!(matches!(
        r.targets[2].outcome,
        TargetOutcome::Unassigned(UnassignedReason::NullResolvedMesh)
    ));
    s.placements[1].model_index = Some(1);
    let r = run(&s, &[]);
    assert!(matches!(
        r.targets[2].outcome,
        TargetOutcome::Unassigned(UnassignedReason::RepeatedTerrainPlacement)
    ));
    s.version = 2;
    s.placements.clear();
    let r = run(&s, &[]);
    assert!(matches!(
        r.targets[0].candidate,
        Candidate::UnsupportedMissingVersionTwoRecordZero
    ));
}
#[test]
fn manually_constructed_invalid_source_is_rejected_without_indexing_panics() {
    for mutation in 0..10 {
        let mut s = scene(2);
        match mutation {
            0 => s.version = 3,
            1 => s.meshes[0].version = 4,
            2 => s.model_slots.pop().map(|_| ()).unwrap(),
            3 => s.model_slots[0] = Some(9),
            4 => s.model_name_offsets[0] = Some(900),
            5 => s.placements[0].model_index = Some(9),
            6 => s.placements[0].name_offset = 999,
            7 => s.string_table.pop().map(|_| ()).unwrap(),
            8 => s.placements[0].extension_data = vec![1],
            9 => s.meshes[1].kind = MeshKind::Model,
            _ => unreachable!(),
        }
        assert!(
            diagnose(&s, &[], &DiagnosticLimits::default()).is_err(),
            "mutation {mutation}"
        );
    }
    let mut s = scene(1);
    s.placements[0].extension_data = vec![0; 4];
    assert!(diagnose(&s, &[], &DiagnosticLimits::default()).is_err());
}
#[test]
fn budgets_count_unused_resources_all_extensions_records_and_names() {
    let s = scene(2);
    let b = classic(&[1, 2, 3], b"opaque");
    for field in 0..6 {
        let mut limits = DiagnosticLimits::default();
        match field {
            0 => limits.max_targets = 1,
            1 => limits.max_resources = 0,
            2 => limits.max_logical_records = 1,
            3 => limits.max_name_bytes = 1,
            4 => limits.max_input_bytes = 1,
            5 => limits.max_words = 2,
            _ => unreachable!(),
        }
        let e = diagnose(&s, &[resource("unused.lit", &b)], &limits).unwrap_err();
        assert!(e.to_string().contains("limit"), "{e}");
    }
    let mut s = scene(2);
    s.placements[0].extension_data = vec![0; 16];
    let limits = DiagnosticLimits {
        max_words: 3,
        ..DiagnosticLimits::default()
    };
    assert!(
        diagnose(&s, &[], &limits)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
    let mut s = scene(1);
    s.trailing_data = vec![0; 64];
    let limits = DiagnosticLimits {
        max_input_bytes: 63,
        ..DiagnosticLimits::default()
    };
    assert!(
        diagnose(&s, &[], &limits)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
}
#[test]
fn repeated_resource_names_parse_once_without_consuming_word_budget_per_target() {
    let mut s = scene(1);
    s.placements[1].name_offset = 22;
    let b = classic(&[1, 2, 3], b"");
    let r = diagnose(
        &s,
        &[resource("instance.lit", &b)],
        &DiagnosticLimits {
            max_words: 3,
            ..DiagnosticLimits::default()
        },
    )
    .unwrap();
    assert_eq!(r.resources.len(), 1);
    words(&r.targets[1], 3, CountComparison::MatchesCurrentPoolOnly);
    words(&r.targets[2], 3, CountComparison::MatchesCurrentPoolOnly);
    assert!(matches!(
        r.targets[1].candidate,
        Candidate::External {
            resource_index: 0,
            ..
        }
    ));
    assert!(matches!(
        r.targets[2].candidate,
        Candidate::External {
            resource_index: 0,
            ..
        }
    ));
}
#[test]
fn bounded_string_search_and_repeated_offsets_charge_actual_name_work() {
    let mut charged = 0;
    let table = vec![b'x'; 100_000];
    assert!(
        bounded_string(&table, 0, &mut charged, 8)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
    let table = b"repeated\0";
    let mut charged = 0;
    assert_eq!(
        bounded_string(table, 0, &mut charged, 55).unwrap(),
        b"repeated"
    );
    assert_eq!(charged, 54);
    assert!(
        bounded_string(table, 0, &mut charged, 55)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
}
#[test]
fn version_two_distinct_arrays_remain_per_placement_on_shared_source_pool() {
    let mut s = scene(2);
    s.placements.push(placement(Some(2), 22));
    s.placements[1].extension_data = [1u32, 2, 3]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    s.placements[2].extension_data = [0xffffffffu32, 0x80000000, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let r = run(&s, &[]);
    assert_eq!(
        r.targets[1].target_mesh_index,
        r.targets[2].target_mesh_index
    );
    assert_eq!(r.targets[1].original_model_slot, Some(0));
    assert_eq!(r.targets[2].original_model_slot, Some(2));
    for t in &r.targets[1..] {
        words(t, 3, CountComparison::MatchesCurrentPoolOnly);
    }
    match (&r.targets[1].candidate, &r.targets[2].candidate) {
        (
            Candidate::Inline {
                record_ordinal: a,
                packed_bytes: pa,
            },
            Candidate::Inline {
                record_ordinal: b,
                packed_bytes: pb,
            },
        ) => {
            assert_eq!((*a, *b), (1, 2));
            assert_ne!(pa, pb);
            assert!(std::ptr::eq(
                pa.as_ptr(),
                s.placements[1].extension_data.as_ptr()
            ));
            assert!(std::ptr::eq(
                pb.as_ptr(),
                s.placements[2].extension_data.as_ptr()
            ));
        }
        _ => panic!(),
    }
}
#[test]
fn truncated_classic_and_unselected_source_suffix_still_remain_visible_or_bounded() {
    let s = scene(1);
    let b = b"EQGP\x03\0\0\0\x01\0\0\0";
    let r = run(&s, &[resource("Actual.LIT", b)]);
    assert!(matches!(
        r.resources[0].parsed,
        Err(raw::ParseError::Truncated { .. })
    ));
    assert!(matches!(
        r.targets[0].outcome,
        TargetOutcome::InvalidOrUnsupportedClassicResource
    ));
    assert_eq!(r.resources[0].input.bytes, b);
    let mut s = scene(1);
    s.meshes[0].trailing_data = vec![0; 128];
    assert!(
        diagnose(
            &s,
            &[],
            &DiagnosticLimits {
                max_input_bytes: 127,
                ..DiagnosticLimits::default()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("limit")
    );
    s.meshes[0].trailing_data.clear();
    s.meshes[1].vertices.clear();
    assert!(matches!(
        run(&s, &[resource("Actual.LIT", &classic(&[], b""))]).targets[0].outcome,
        TargetOutcome::Words {
            count: 0,
            comparison: CountComparison::Empty
        }
    ));
}
#[test]
fn equal_counts_never_certify_lod_or_hierarchical_consumers() {
    let mut s = scene(1);
    let b = classic(&[1, 2, 3], b"");
    let r = run(&s, &[resource("instance.LIT", &b)]);
    assert_eq!(
        r.targets[1].consumer_scope,
        ConsumerScope::CurrentPoolOnlyNativeContextUnresolved
    );
    s.meshes[0].bone_count = Some(2);
    let r = run(&s, &[resource("instance.LIT", &b)]);
    words(&r.targets[1], 3, CountComparison::MatchesCurrentPoolOnly);
    assert_eq!(
        r.targets[1].consumer_scope,
        ConsumerScope::HierarchicalConsumerUnsupported
    );
}
