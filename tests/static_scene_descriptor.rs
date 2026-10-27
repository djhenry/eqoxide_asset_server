use eqoxide_asset_server::{
    cas::Cas,
    compatibility::ReaderRequirements,
    manifest::{FileEntry, Manifest, ManifestStore},
    static_collision::InspectionQueryPolicy,
    static_scene_descriptor::*,
};
fn visual_req(colored: bool) -> ReaderRequirements {
    ReaderRequirements {
        reader_version: 2,
        capabilities: if colored {
            vec!["static-visual-v1".into(), "vertex-rgba-v1".into()]
        } else {
            vec!["static-visual-v1".into()]
        },
    }
}
fn collision_req() -> ReaderRequirements {
    ReaderRequirements {
        reader_version: 2,
        capabilities: vec!["static-collision-triangles-v1".into()],
    }
}
fn visual() -> VisualComponent {
    VisualComponent::Available {
        scope: "declared-static-visual-scene-v1".into(),
        file: SceneFile {
            path: "visual.glb".into(),
            blake3: Cas::hash(b"synthetic visual"),
            size: 16,
        },
        requirements: visual_req(true),
    }
}
fn collision(empty: bool) -> CollisionComponent {
    let file = SceneFile {
        path: "collision.glb".into(),
        blake3: Cas::hash(b"synthetic collision"),
        size: 19,
    };
    if empty {
        CollisionComponent::AvailableEmpty {
            scope: "declared-static-triangle-candidates-v1".into(),
            file,
            requirements: collision_req(),
            query_policy: InspectionQueryPolicy::BothFaces,
        }
    } else {
        CollisionComponent::Available {
            scope: "declared-static-triangle-candidates-v1".into(),
            file,
            requirements: collision_req(),
            query_policy: InspectionQueryPolicy::FrontFaces,
        }
    }
}
#[test]
fn legitimate_availability_and_policies_round_trip_with_exact_capability_union() {
    for v in [
        VisualComponent::NotExtracted {},
        VisualComponent::Unsupported {},
        visual(),
    ] {
        for c in [
            CollisionComponent::NotExtracted {},
            CollisionComponent::Unsupported {},
            collision(false),
            collision(true),
        ] {
            let descriptor =
                SceneDescriptor::new("demo", &"a".repeat(64), v.clone(), c.clone()).unwrap();
            let limits = DescriptorLimits::default();
            let bytes = encode_scene_descriptor(&descriptor, &limits).unwrap();
            assert_eq!(
                decode_scene_descriptor(&bytes, &limits).unwrap(),
                descriptor
            );
            assert_eq!(descriptor.set, "zone/demo");
            assert_eq!(descriptor.requirements.reader_version, 2);
            assert!(descriptor.required_operations.is_empty());
            let mut expected = vec!["static-scene-package-v1"];
            if matches!(v, VisualComponent::Available { .. }) {
                expected.extend(["static-visual-v1", "vertex-rgba-v1"]);
            }
            if matches!(
                c,
                CollisionComponent::Available { .. } | CollisionComponent::AvailableEmpty { .. }
            ) {
                expected.push("static-collision-triangles-v1");
            }
            expected.sort();
            assert_eq!(descriptor.requirements.capabilities, expected);
        }
    }
}
#[test]
fn factory_hashes_real_bytes_without_certifying_component_contents() {
    let limits = DescriptorLimits::default();
    let file = SceneFile::from_bytes("visual.glb", b"not a GLB", &limits).unwrap();
    assert_eq!(file.blake3, Cas::hash(b"not a GLB"));
    assert_eq!(file.size, 9);
}
fn fixture() -> SceneDescriptor {
    SceneDescriptor {
        schema_version: 1,
        set: "zone/demo".into(),
        zone: "demo".into(),
        scope: "declared-static-inspection-v1".into(),
        coordinate_profile: "eqoxide-static-y-up-v1".into(),
        unit_scale: 1,
        bake_revision: "a".repeat(64),
        requirements: ReaderRequirements {
            reader_version: 2,
            capabilities: vec![
                "static-collision-triangles-v1".into(),
                "static-scene-package-v1".into(),
                "static-visual-v1".into(),
                "vertex-rgba-v1".into(),
            ],
        },
        required_operations: vec![],
        components: SceneComponents {
            visual: visual(),
            collision: collision(false),
        },
    }
}
fn wire() -> Vec<u8> {
    serde_json::to_vec(&fixture()).unwrap()
}
fn edited(f: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut value = serde_json::to_value(fixture()).unwrap();
    f(&mut value);
    serde_json::to_vec(&value).unwrap()
}
fn refused(bytes: &[u8]) {
    assert!(decode_scene_descriptor(bytes, &DescriptorLimits::default()).is_err());
}
fn manifest(bytes: &[u8], d: &SceneDescriptor) -> Manifest {
    let mut files = vec![FileEntry {
        path: "scene.json".into(),
        size: bytes.len() as u64,
        blake3: Cas::hash(bytes),
        chunks: vec![Cas::hash(bytes)],
    }];
    if let VisualComponent::Available { file, .. } = &d.components.visual {
        files.push(FileEntry {
            path: file.path.clone(),
            size: file.size,
            blake3: file.blake3.clone(),
            chunks: vec![file.blake3.clone()],
        });
    }
    if let CollisionComponent::Available { file, .. }
    | CollisionComponent::AvailableEmpty { file, .. } = &d.components.collision
    {
        files.push(FileEntry {
            path: file.path.clone(),
            size: file.size,
            blake3: file.blake3.clone(),
            chunks: vec![file.blake3.clone()],
        });
    }
    let mut m = Manifest {
        schema_version: 1,
        revision: String::new(),
        requirements: d.requirements.clone(),
        set: d.set.clone(),
        digest: String::new(),
        files,
    };
    rehash(&mut m);
    m
}
fn rehash(m: &mut Manifest) {
    m.digest = ManifestStore::set_digest(&m.files);
    m.revision = m.canonical_revision().unwrap();
}
#[test]
fn strict_schema_identity_and_integer_tokens_refuse_lossy_or_unknown_inputs() {
    for (key, value) in [
        ("schemaVersion", serde_json::json!(2)),
        ("schemaVersion", serde_json::json!(1.0)),
        ("unitScale", serde_json::json!(1.0)),
        ("unitScale", serde_json::json!(-1)),
        ("set", serde_json::json!("zone/other")),
        ("zone", serde_json::json!("UPPER")),
        ("scope", serde_json::json!("gameplay")),
        ("coordinateProfile", serde_json::json!("unknown")),
        ("bakeRevision", serde_json::json!("A".repeat(64))),
        (
            "requiredOperations",
            serde_json::json!(["static-segment-query-v1"]),
        ),
        ("unexpected", serde_json::json!(true)),
    ] {
        refused(&edited(|j| j[key] = value));
    }
    for key in [
        "schemaVersion",
        "requiredOperations",
        "components",
        "requirements",
    ] {
        refused(&edited(|j| {
            j.as_object_mut().unwrap().remove(key);
        }));
    }
    for role in ["visual", "collision"] {
        refused(&edited(|j| {
            j["components"].as_object_mut().unwrap().remove(role);
        }));
    }
    refused(&edited(|j| {
        j["components"]["visual"]["availability"] = serde_json::json!("available-empty")
    }));
    refused(&edited(|j| {
        j["components"]["collision"]["availability"] = serde_json::json!("missing")
    }));
    refused(&edited(|j| {
        j["components"]["collision"]["queryPolicy"] = serde_json::json!("native-movement")
    }));
    refused(&edited(|j| {
        j["components"]["visual"]["file"]["size"] = serde_json::json!(16.0)
    }));
    refused(&edited(|j| {
        j["components"]["visual"]["file"]["size"] = serde_json::json!(u64::MAX)
    }));
}
#[test]
fn unavailable_records_cannot_smuggle_any_payload_or_unknown_field() {
    for role in ["visual", "collision"] {
        for state in ["not-extracted", "unsupported"] {
            for field in ["file", "requirements", "scope", "queryPolicy", "hidden"] {
                let visual = if role == "visual" {
                    if state == "not-extracted" {
                        VisualComponent::NotExtracted {}
                    } else {
                        VisualComponent::Unsupported {}
                    }
                } else {
                    visual()
                };
                let collision = if role == "collision" {
                    if state == "not-extracted" {
                        CollisionComponent::NotExtracted {}
                    } else {
                        CollisionComponent::Unsupported {}
                    }
                } else {
                    collision(false)
                };
                let control =
                    SceneDescriptor::new("demo", &"a".repeat(64), visual, collision).unwrap();
                let control_bytes = serde_json::to_vec(&control).unwrap();
                assert!(
                    decode_scene_descriptor(&control_bytes, &DescriptorLimits::default()).is_ok()
                );
                let mut j = serde_json::to_value(&control).unwrap();
                j["components"][role]
                    .as_object_mut()
                    .unwrap()
                    .insert(field.into(), serde_json::Value::Null);
                refused(&serde_json::to_vec(&j).unwrap());
            }
        }
    }
    for role in ["visual", "collision"] {
        for field in ["scope", "file", "requirements"] {
            refused(&edited(|j| {
                j["components"][role].as_object_mut().unwrap().remove(field);
            }));
        }
    }
    refused(&edited(|j| {
        j["components"]["collision"]
            .as_object_mut()
            .unwrap()
            .remove("queryPolicy");
    }));
}
#[test]
fn duplicate_decoded_keys_are_refused_in_tagged_and_nested_records() {
    let original = String::from_utf8(wire()).unwrap();
    for (old, new) in [
        (
            "\"schemaVersion\":1",
            "\"schemaVersion\":2,\"schemaVersion\":1",
        ),
        (
            "\"schemaVersion\":1",
            "\"schemaVersion\":1,\"schema\\u0056ersion\":1",
        ),
        (
            "\"availability\":\"available\"",
            "\"availability\":\"unsupported\",\"availability\":\"available\"",
        ),
        (
            "\"path\":\"visual.glb\"",
            "\"path\":\"other\",\"path\":\"visual.glb\"",
        ),
        (
            "\"reader_version\":2",
            "\"reader_version\":3,\"reader_version\":2",
        ),
        (
            "\"scope\":\"declared-static-visual-scene-v1\"",
            "\"scope\":\"unknown\",\"scope\":\"declared-static-visual-scene-v1\"",
        ),
    ] {
        assert!(original.contains(old));
        refused(original.replacen(old, new, 1).as_bytes());
    }
    for role in ["visual", "collision"] {
        for state in ["not-extracted", "unsupported"] {
            let bytes =
                edited(|j| j["components"][role] = serde_json::json!({"availability":state}));
            let raw = String::from_utf8(bytes).unwrap();
            let old = format!("\"availability\":\"{state}\"");
            refused(raw.replacen(&old, &format!("{old},{old}"), 1).as_bytes());
            refused(
                raw.replacen(
                    &old,
                    &format!("{old},\"availabilit\\u0079\":\"{state}\""),
                    1,
                )
                .as_bytes(),
            );
        }
    }
}
#[test]
fn references_and_declared_requirements_are_strictly_bounded() {
    for path in ["", "../bad", "/absolute", "a//b", "a:b", "scene.json"] {
        assert!(SceneFile::from_bytes(path, b"x", &DescriptorLimits::default()).is_err());
    }
    assert!(SceneFile::from_bytes("x", b"", &DescriptorLimits::default()).is_err());
    let mut limit = DescriptorLimits::default();
    limit.max_component_bytes = 8;
    assert!(SceneFile::from_bytes("x", b"ninebytes", &limit).is_err());
    for (field, value) in [
        ("path", serde_json::json!("scene.json")),
        ("size", serde_json::json!(0)),
        ("blake3", serde_json::json!("g".repeat(64))),
    ] {
        refused(&edited(|j| {
            j["components"]["visual"]["file"][field] = value
        }));
    }
    refused(&edited(|j| {
        j["components"]["collision"]["file"]["path"] = serde_json::json!("visual.glb")
    }));
    for role in ["visual", "collision"] {
        refused(&edited(|j| {
            j["components"][role]["requirements"]["reader_version"] = serde_json::json!(3)
        }));
        refused(&edited(|j| {
            j["components"][role]["requirements"]["capabilities"] =
                serde_json::json!(["static-segment-query-v1"])
        }));
        refused(&edited(|j| {
            let caps = j["components"][role]["requirements"]["capabilities"]
                .as_array_mut()
                .unwrap();
            caps.push(caps[0].clone());
        }));
    }
    refused(&edited(|j| {
        j["requirements"]["capabilities"] = serde_json::json!(["static-scene-package-v1"])
    }));
    refused(&edited(|j| {
        let caps = j["requirements"]["capabilities"].as_array_mut().unwrap();
        caps.push(caps[0].clone());
    }));
    refused(&edited(|j| {
        j["requirements"]["reader_version"] = serde_json::json!(1)
    }));
}
#[test]
fn descriptor_json_budget_is_exact_and_deep_input_is_refused() {
    let d = fixture();
    let bytes = encode_scene_descriptor(&d, &DescriptorLimits::default()).unwrap();
    let mut limits = DescriptorLimits::default();
    limits.max_json_bytes = bytes.len();
    assert_eq!(encode_scene_descriptor(&d, &limits).unwrap(), bytes);
    assert!(decode_scene_descriptor(&bytes, &limits).is_ok());
    limits.max_json_bytes -= 1;
    assert!(encode_scene_descriptor(&d, &limits).is_err());
    assert!(decode_scene_descriptor(&bytes, &limits).is_err());
    let nested = format!("\"scope\":{}0{}", "[".repeat(256), "]".repeat(256));
    let raw = String::from_utf8(wire()).unwrap();
    refused(
        raw.replacen("\"scope\":\"declared-static-visual-scene-v1\"", &nested, 1)
            .as_bytes(),
    );
}
#[test]
fn dto_mutations_are_revalidated_at_encoding_and_binding() {
    let mut d = fixture();
    d.unit_scale = 2;
    assert!(encode_scene_descriptor(&d, &DescriptorLimits::default()).is_err());
    let bytes = serde_json::to_vec(&d).unwrap();
    let m = manifest(&bytes, &d);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
            .is_err()
    );
}
#[test]
fn binding_matches_real_descriptor_bytes_and_declared_component_entries_only() {
    let d = fixture();
    let bytes = encode_scene_descriptor(&d, &DescriptorLimits::default()).unwrap();
    let m = manifest(&bytes, &d);
    let proof = bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
        .unwrap();
    assert_eq!(proof.descriptor(), &d);
    assert_eq!(proof.manifest_revision(), m.revision);
    assert_eq!(proof.descriptor_blake3(), Cas::hash(&bytes));
    // No real CAS data was stored and these component bytes are not GLB. Binding certifies neither.
    let d = SceneDescriptor::new(
        "demo",
        &"a".repeat(64),
        VisualComponent::Available {
            scope: "declared-static-visual-scene-v1".into(),
            file: SceneFile::from_bytes("visual.glb", b"not a GLB", &DescriptorLimits::default())
                .unwrap(),
            requirements: visual_req(false),
        },
        CollisionComponent::Unsupported {},
    )
    .unwrap();
    let bytes = encode_scene_descriptor(&d, &DescriptorLimits::default()).unwrap();
    let m = manifest(&bytes, &d);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default()).is_ok()
    );
}
#[test]
fn both_unavailable_is_explicit_metadata_with_descriptor_only_membership() {
    let d = SceneDescriptor::new(
        "demo",
        &"a".repeat(64),
        VisualComponent::NotExtracted {},
        CollisionComponent::Unsupported {},
    )
    .unwrap();
    assert_eq!(d.requirements.capabilities, ["static-scene-package-v1"]);
    let bytes = encode_scene_descriptor(&d, &DescriptorLimits::default()).unwrap();
    let mut m = manifest(&bytes, &d);
    assert_eq!(m.files.len(), 1);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default()).is_ok()
    );
    m.files.push(FileEntry {
        path: "stray.glb".into(),
        size: 1,
        blake3: "b".repeat(64),
        chunks: vec![],
    });
    rehash(&mut m);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
            .is_err()
    );
}
#[test]
fn rehashed_manifest_corruptions_cannot_hide_behind_stale_envelope_errors() {
    let d = fixture();
    let bytes = wire();
    let base = manifest(&bytes, &d);
    for which in 0..8 {
        let mut m = base.clone();
        match which {
            0 => m.files[0].size += 1,
            1 => m.files[0].blake3 = "b".repeat(64),
            2 => m.files[1].size += 1,
            3 => m.files[1].blake3 = "b".repeat(64),
            4 => m.files[1].path = "wrong.glb".into(),
            5 => {
                m.files.remove(2);
            }
            6 => m.files.push(FileEntry {
                path: "extra".into(),
                size: 1,
                blake3: "c".repeat(64),
                chunks: vec![],
            }),
            _ => m.requirements.capabilities.push("legacy-assets-v1".into()),
        };
        rehash(&mut m);
        m.validate("zone/demo").unwrap();
        assert!(
            bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
                .is_err(),
            "corruption {which}"
        );
    }
    let mut m = base.clone();
    m.set = "zone/other".into();
    rehash(&mut m);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "other", &DescriptorLimits::default())
            .is_err()
    );
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
            .is_err()
    );
    assert!(bind_scene_descriptor_to_manifest(
        &bytes,
        &base,
        "other",
        &DescriptorLimits::default()
    )
    .is_err());
    let mut m = base.clone();
    m.requirements.reader_version = 3;
    rehash(&mut m);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
            .is_err()
    );
    let mut m = base.clone();
    m.files.push(m.files[1].clone());
    rehash(&mut m);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
            .is_err()
    );
    let mut m = base.clone();
    m.digest = "a".repeat(64);
    m.revision = m.canonical_revision().unwrap();
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
            .is_err()
    );
    let mut m = base.clone();
    m.files.remove(0);
    rehash(&mut m);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
            .is_err()
    );
    let spaced = [bytes.as_slice(), b" "].concat();
    assert!(bind_scene_descriptor_to_manifest(
        &spaced,
        &base,
        "demo",
        &DescriptorLimits::default()
    )
    .is_err());
    let mut m = base.clone();
    m.revision = "a".repeat(64);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
            .is_err()
    );
}
#[test]
fn normalized_manifest_capabilities_and_chunk_layout_keep_exact_revision_identity() {
    let d = fixture();
    let bytes = wire();
    let mut m = manifest(&bytes, &d);
    let original = m.revision.clone();
    m.requirements.capabilities.reverse();
    m.requirements
        .capabilities
        .push("static-scene-package-v1".into());
    rehash(&mut m);
    assert_eq!(m.revision, original);
    assert!(
        bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default()).is_ok()
    );
    let additional_chunk = m.files[1].blake3.clone();
    m.files[1].chunks.push(additional_chunk);
    rehash(&mut m);
    assert_ne!(m.revision, original);
    let proof = bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &DescriptorLimits::default())
        .unwrap();
    assert_eq!(proof.manifest_revision(), m.revision);
}
#[test]
fn manifest_work_preflight_runs_before_existing_unbounded_canonical_serialization() {
    let d = fixture();
    let bytes = wire();
    let mut m = manifest(&bytes, &d);
    m.revision = "invalid".into();
    let mut l = DescriptorLimits::default();
    l.max_manifest_files = 2;
    let error = bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &l)
        .unwrap_err()
        .to_string();
    assert!(error.contains("manifest file count"), "{error}");
    let mut l = DescriptorLimits::default();
    l.max_chunk_references = 2;
    let error = bind_scene_descriptor_to_manifest(&bytes, &m, "demo", &l)
        .unwrap_err()
        .to_string();
    assert!(error.contains("manifest chunk references"), "{error}");
}
#[test]
fn atomic_descriptor_write_preserves_prior_file_on_validation_or_budget_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scene.json");
    std::fs::write(&path, b"sentinel").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut d = fixture();
    d.unit_scale = 2;
    assert!(write_scene_descriptor(&d, &path, &DescriptorLimits::default()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"sentinel");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let mut l = DescriptorLimits::default();
    l.max_json_bytes = 1;
    assert!(write_scene_descriptor(&fixture(), &path, &l).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"sentinel");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let requirements =
        write_scene_descriptor(&fixture(), &path, &DescriptorLimits::default()).unwrap();
    assert_eq!(requirements, fixture().requirements);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        decode_scene_descriptor(&bytes, &DescriptorLimits::default()).unwrap(),
        fixture()
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
}

#[test]
fn failed_descriptor_replacement_preserves_destination_and_cleans_staging() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("scene.json");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("sentinel"), b"unchanged").unwrap();
    assert!(
        write_scene_descriptor(&fixture(), &destination, &DescriptorLimits::default()).is_err()
    );
    assert_eq!(
        std::fs::read(destination.join("sentinel")).unwrap(),
        b"unchanged"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn preserved_source_query_frames_descriptor_union_and_refusals() {
    let cap = "eqoxide-source-query-frames-preserved-v1";
    for (empty, reverse) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut c = collision(empty);
        let req = match &mut c {
            CollisionComponent::Available { requirements, .. }
            | CollisionComponent::AvailableEmpty { requirements, .. } => requirements,
            _ => unreachable!(),
        };
        // Component order is set-like; descriptor root remains sorted.
        req.capabilities.push(cap.into());
        if reverse {
            req.capabilities.reverse();
        }
        let d = SceneDescriptor::new("demo", &"a".repeat(64), visual(), c).unwrap();
        assert_eq!(
            d.requirements.capabilities,
            [
                cap,
                "static-collision-triangles-v1",
                "static-scene-package-v1",
                "static-visual-v1",
                "vertex-rgba-v1"
            ]
        );
        let bytes = encode_scene_descriptor(&d, &DescriptorLimits::default()).unwrap();
        assert_eq!(
            decode_scene_descriptor(&bytes, &DescriptorLimits::default()).unwrap(),
            d
        );
        for invalid in [
            vec![cap, cap, "static-collision-triangles-v1"],
            vec![cap],
            vec![cap, "static-collision-triangles-v1", "unknown-v1"],
        ] {
            let mut bad = serde_json::to_value(&d).unwrap();
            bad["components"]["collision"]["requirements"]["capabilities"] =
                serde_json::json!(invalid);
            refused(&serde_json::to_vec(&bad).unwrap());
        }
    }
    let d = SceneDescriptor::new(
        "demo",
        &"a".repeat(64),
        visual(),
        CollisionComponent::NotExtracted {},
    )
    .unwrap();
    let mut stray = serde_json::to_value(&d).unwrap();
    stray["requirements"]["capabilities"]
        .as_array_mut()
        .unwrap()
        .insert(0, cap.into());
    refused(&serde_json::to_vec(&stray).unwrap());
    let mut v = visual();
    if let VisualComponent::Available { requirements, .. } = &mut v {
        requirements.capabilities.push(cap.into());
    }
    assert!(SceneDescriptor::new(
        "demo",
        &"a".repeat(64),
        v,
        CollisionComponent::NotExtracted {},
    )
    .is_err());
}

#[test]
fn retired_explicit_query_frame_capability_is_refused_in_every_role() {
    let old = "static-collision-query-unit-frames-v1";
    for empty in [false, true] {
        let mut c = collision(empty);
        match &mut c {
            CollisionComponent::Available { requirements, .. }
            | CollisionComponent::AvailableEmpty { requirements, .. } => {
                requirements.capabilities.push(old.into())
            }
            _ => unreachable!(),
        }
        assert!(SceneDescriptor::new("demo", &"a".repeat(64), visual(), c).is_err());
        let d = SceneDescriptor::new("demo", &"a".repeat(64), visual(), collision(empty)).unwrap();
        let mut bad = serde_json::to_value(&d).unwrap();
        // A matching root union cannot legitimize the retired component declaration.
        bad["requirements"]["capabilities"]
            .as_array_mut()
            .unwrap()
            .insert(0, old.into());
        bad["components"]["collision"]["requirements"]["capabilities"]
            .as_array_mut()
            .unwrap()
            .push(old.into());
        refused(&serde_json::to_vec(&bad).unwrap());
    }
    for cap in [old, "eqoxide-source-query-frames-preserved-v1"] {
        for collision_role in [
            CollisionComponent::NotExtracted {},
            CollisionComponent::Unsupported {},
        ] {
            let d =
                SceneDescriptor::new("demo", &"a".repeat(64), visual(), collision_role).unwrap();
            let mut root_only = serde_json::to_value(&d).unwrap();
            root_only["requirements"]["capabilities"]
                .as_array_mut()
                .unwrap()
                .insert(0, cap.into());
            refused(&serde_json::to_vec(&root_only).unwrap());
            let mut smuggled = serde_json::to_value(&d).unwrap();
            smuggled["components"]["collision"]["requirements"] = serde_json::json!({"readerVersion":2,"capabilities":[cap,"static-collision-triangles-v1"]});
            refused(&serde_json::to_vec(&smuggled).unwrap());
        }
        let mut v = visual();
        if let VisualComponent::Available { requirements, .. } = &mut v {
            requirements.capabilities.push(cap.into());
        }
        assert!(SceneDescriptor::new(
            "demo",
            &"a".repeat(64),
            v,
            CollisionComponent::NotExtracted {}
        )
        .is_err());
    }
}
