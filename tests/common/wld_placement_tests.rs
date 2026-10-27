use super::*;
use std::io::Cursor;

#[allow(dead_code)]
#[path = "wld_fixture.rs"]
mod mesh_fixture;

#[derive(Clone, Debug)]
struct ActorSpec {
    position: [f32; 3],
    // On-disk field order: rotate_z, rotate_y, rotate_x, in 512ths of a turn.
    rotation: [f32; 3],
    location: bool,
    scale: Option<f32>,
    radius: Option<f32>,
}
impl Default for ActorSpec {
    fn default() -> Self {
        Self {
            position: [17., -11., 23.],
            rotation: [0.; 3],
            location: true,
            scale: Some(1.5),
            radius: Some(9.),
        }
    }
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn document(fragments: &[(u32, Vec<u8>)], strings: &str) -> Vec<u8> {
    let key = [0x95, 0x3a, 0xc5, 0x2a, 0x95, 0x7a, 0x95, 0x6a];
    let encoded: Vec<u8> = strings
        .bytes()
        .zip(key.into_iter().cycle())
        .map(|(a, b)| a ^ b)
        .collect();
    let mut out = words(&[
        0x54503d02,
        0x15500,
        fragments.len() as u32,
        0,
        0,
        encoded.len() as u32,
        0,
    ]);
    out.extend(encoded);
    for (kind, body) in fragments {
        out.extend(words(&[body.len() as u32, *kind]));
        out.extend(body);
    }
    out
}
fn actor(spec: &ActorSpec) -> Vec<u8> {
    let flags = (u32::from(spec.location) * 2)
        | (u32::from(spec.radius.is_some()) * 4)
        | (u32::from(spec.scale.is_some()) * 8);
    let mut out = words(&[0, (-1_i32) as u32, flags, 0]);
    if spec.location {
        for value in spec.position.into_iter().chain(spec.rotation) {
            out.extend(value.to_le_bytes());
        }
        out.extend(0_u32.to_le_bytes());
    }
    for value in [spec.radius, spec.scale].into_iter().flatten() {
        out.extend(value.to_le_bytes());
    }
    out.extend(0_u32.to_le_bytes()); // Empty user data.
    out
}
fn archive(path: &Path, name: &str, bytes: &[u8]) {
    let mut writer = libeq_pfs::PfsWriter::create(std::fs::File::create(path).unwrap()).unwrap();
    writer.insert(name, Cursor::new(bytes)).unwrap();
    writer.finish().unwrap();
}
fn main_archive(path: &Path, spec: &ActorSpec) {
    // An unrelated fragment proves error context uses the original document slot.
    let bytes = document(
        &[(0x9999, vec![]), (0x15, actor(spec))],
        "\0PROBE_ACTORDEF\0",
    );
    archive(path, "objects.wld", &bytes);
}
fn model_archive(path: &Path) {
    let mut mesh = mesh_fixture::MeshFixture::default().bytes(false);
    mesh[0..4].copy_from_slice(&(-1_i32).to_le_bytes());
    mesh[8..12].copy_from_slice(&2_u32.to_le_bytes()); // Palette fragment, one-based.
    mesh[88..90].copy_from_slice(&1_u16.to_le_bytes()); // One face/material group.
    mesh.extend(1_u16.to_le_bytes());
    mesh.extend(0_u16.to_le_bytes());
    let palette = words(&[0, 0, 1, 3]);
    let material = words(&[0, 0, 3, 0, 1_f32.to_bits(), 1_f32.to_bits(), 0]);
    let bytes = document(
        &[(0x36, mesh), (0x31, palette), (0x30, material)],
        "\0PROBE_DMSPRITEDEF\0",
    );
    archive(path, "models.wld", &bytes);
}

// Independent scalar source-space oracle: Rx(c), then Ry(-b), then Rz(a).
// Mesh center is included BEFORE scale and rotations; swap Y/Z only at the end.
fn expected(spec: &ActorSpec) -> Vec<[f32; 3]> {
    [[-1., 2., -3.], [3., 2., -3.], [-1., 5., -3.]]
        .into_iter()
        .map(|v| {
            let s = f64::from(spec.scale.unwrap());
            let [mut x, mut y, mut z] = [(v[0] + 10.) * s, (v[1] - 20.) * s, (v[2] + 3.) * s];
            let [a, b, c] = spec
                .rotation
                .map(|v| f64::from(v) * std::f64::consts::TAU / 512.);
            (y, z) = (y * c.cos() - z * c.sin(), y * c.sin() + z * c.cos());
            (x, z) = (x * b.cos() - z * b.sin(), x * b.sin() + z * b.cos());
            (x, y) = (x * a.cos() - y * a.sin(), x * a.sin() + y * a.cos());
            [
                (x + f64::from(spec.position[0])) as f32,
                (z + f64::from(spec.position[2])) as f32,
                (y + f64::from(spec.position[1])) as f32,
            ]
        })
        .collect()
}
fn check_case(name: &str, spec: ActorSpec) {
    let temporary = tempfile::tempdir().unwrap();
    let directory = std::env::var_os("WLD_PLACEMENT_TEST_OUTPUT")
        .map(|root| std::path::PathBuf::from(root).join(name))
        .unwrap_or_else(|| temporary.path().to_path_buf());
    std::fs::create_dir_all(&directory).unwrap();
    let main = directory.join("placement.s3d");
    let model = directory.join("model.s3d");
    let glb = directory.join("visual.glb");
    main_archive(&main, &spec);
    model_archive(&model);
    let placements = read_placements(&main).unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].0, "PROBE");
    let models = load_object_models(&model, Some(&main)).unwrap();
    // Actual model loader includes the nonzero raw mesh center, exactly once.
    assert_eq!(
        models["PROBE"][0].positions,
        [[9., 0., -18.], [13., 0., -18.], [9., 0., -15.]]
    );
    bake_zone(&main, Some(&model), &glb).unwrap();
    let expected = expected(&spec);
    std::fs::write(
        directory.join("expected.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"positions":expected})).unwrap(),
    )
    .unwrap();
    let (doc, buffers, _) = gltf::import(&glb).unwrap();
    let nodes: Vec<_> = doc.nodes().collect();
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];
    let matrix = node.transform().matrix();
    assert_eq!(matrix, placements[0].1);
    let primitive = node.mesh().unwrap().primitives().next().unwrap();
    let reader = primitive.reader(|b| Some(&buffers[b.index()]));
    assert_eq!(
        reader
            .read_indices()
            .unwrap()
            .into_u32()
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    for (vertex, want) in reader.read_positions().unwrap().zip(expected) {
        for axis in 0..3 {
            let actual = matrix[0][axis] * vertex[0]
                + matrix[1][axis] * vertex[1]
                + matrix[2][axis] * vertex[2]
                + matrix[3][axis];
            assert!(
                (actual - want[axis]).abs() < 0.00005,
                "{name} axis {axis}: {actual} != {}",
                want[axis]
            );
        }
    }
}
#[test]
fn wld_placement_raw_x_axis() {
    check_case(
        "raw-x",
        ActorSpec {
            rotation: [0., 0., 64.],
            ..Default::default()
        },
    );
}
#[test]
fn wld_placement_raw_y_axis() {
    check_case(
        "raw-y",
        ActorSpec {
            rotation: [0., 128., 0.],
            ..Default::default()
        },
    );
}
#[test]
fn wld_placement_raw_z_axis() {
    check_case(
        "raw-z",
        ActorSpec {
            rotation: [128., 0., 0.],
            ..Default::default()
        },
    );
}
#[test]
fn wld_placement_noncommuting_rotations() {
    check_case(
        "mixed",
        ActorSpec {
            rotation: [73., 109., 47.],
            ..Default::default()
        },
    );
}
#[test]
fn wld_placement_small_positive_scale_is_not_radius() {
    check_case(
        "small-scale",
        ActorSpec {
            scale: Some(0.005),
            radius: Some(2.),
            ..Default::default()
        },
    );
}
#[test]
fn wld_placement_radius_is_optional() {
    check_case(
        "absent-radius",
        ActorSpec {
            radius: None,
            ..Default::default()
        },
    );
}
#[test]
fn wld_placement_heading_only_preserves_matrix_bits() {
    let dir = tempfile::tempdir().unwrap();
    let spec = ActorSpec {
        rotation: [57.3, 0., 0.],
        ..Default::default()
    };
    let path = dir.path().join("heading.s3d");
    main_archive(&path, &spec);
    let actual = read_placements(&path).unwrap()[0].1;
    let expected = placement_matrix((17., 23., -11.), spec.rotation[0] / 512. * -360., 1.5);
    assert_eq!(
        actual.map(|c| c.map(f32::to_bits)),
        expected.map(|c| c.map(f32::to_bits))
    );
}

#[test]
fn wld_placement_invalid_transforms_have_source_context() {
    let mut cases = Vec::new();
    cases.push((
        ActorSpec {
            location: false,
            ..Default::default()
        },
        "missing location",
    ));
    cases.push((
        ActorSpec {
            scale: None,
            ..Default::default()
        },
        "missing scale",
    ));
    for scale in [0., -0., -1., f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        cases.push((
            ActorSpec {
                scale: Some(scale),
                ..Default::default()
            },
            "scale",
        ));
    }
    for axis in 0..3 {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut spec = ActorSpec::default();
            spec.position[axis] = value;
            cases.push((spec, "location"));
            let mut spec = ActorSpec::default();
            spec.rotation[axis] = value;
            cases.push((spec, "rotation"));
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.s3d");
    for (spec, reason) in cases {
        main_archive(&path, &spec);
        let error = format!("{:#}", read_placements(&path).unwrap_err());
        for context in [
            "invalid.s3d",
            "objects.wld",
            "Actor slot 1",
            "PROBE_ACTORDEF",
            reason,
        ] {
            assert!(error.contains(context), "expected {context}: {error}");
        }
    }
}

#[test]
fn wld_placement_nonfinite_radius_does_not_scale_geometry() {
    check_case(
        "nonfinite-radius",
        ActorSpec {
            radius: Some(f32::NAN),
            ..Default::default()
        },
    );
}

#[test]
fn wld_placement_truncated_required_scale_is_not_empty_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("truncated-scale.s3d");
    let mut body = actor(&ActorSpec::default());
    // Flags promise location, radius and scale; stop halfway through scale,
    // before its remaining two bytes and the four-byte user-data length.
    body.truncate(body.len() - 6);
    let bytes = document(&[(0x9999, vec![]), (0x15, body)], "\0PROBE_ACTORDEF\0");
    let parsed = libeq_wld::parser::WldDoc::parse(&bytes).unwrap();
    assert!(matches!(
        parsed.at(1),
        Some(libeq_wld::parser::FragmentType::Unknown(unknown)) if unknown.type_id == 0x15
    ));
    archive(&path, "objects.wld", &bytes);
    let error = format!("{:#}", read_placements(&path).unwrap_err());
    for context in [
        "truncated-scale.s3d",
        "objects.wld",
        "Actor slot 1",
        "unavailable Actor body",
        "0x15",
    ] {
        assert!(error.contains(context), "expected {context}: {error}");
    }
}

#[test]
fn wld_placement_unrelated_unknown_is_still_tolerated() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("unrelated-unknown.s3d");
    let bytes = document(&[(0x9999, vec![])], "\0");
    archive(&path, "objects.wld", &bytes);
    assert!(read_placements(&path).unwrap().is_empty());
}

fn reference_document(references: &[(i32, ActorSpec)], typed_definition: bool) -> Vec<u8> {
    // Offset 1 names PROBE; fragment reference 1 instead targets DEFINITION.
    let target = if typed_definition {
        (0x14, words(&[(-16_i32) as u32, 0, 0, 0, 0, 0, 0]))
    } else {
        (0x16, words(&[0, 0])) // A typed Sphere, not an ActorDef.
    };
    let mut fragments = vec![target, (0x9999, vec![])];
    for (reference, spec) in references {
        let mut body = actor(spec);
        body[4..8].copy_from_slice(&reference.to_le_bytes());
        fragments.push((0x15, body));
    }
    document(&fragments, "\0PROBE_ACTORDEF\0DEFINITION\0")
}

#[test]
fn wld_reference_positive_index_does_not_alias_named_placement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mixed-reference.s3d");
    let bytes = reference_document(
        &[(1, ActorSpec::default()), (-1, ActorSpec::default())],
        true,
    );
    let doc = libeq_wld::parser::WldDoc::parse(&bytes).unwrap();
    let Some(libeq_wld::parser::FragmentType::ActorDef(target)) = doc.at(0) else {
        panic!("fixture must have a typed ActorDef at reference 1");
    };
    assert_eq!(doc.get_string(target.name_reference), Some("DEFINITION"));
    assert_eq!(
        doc.get_string(libeq_wld::parser::StringReference(1)),
        Some("PROBE_ACTORDEF")
    );
    archive(&path, "objects.wld", &bytes);
    let placements = read_placements(&path).unwrap();
    assert_eq!(placements.len(), 1, "only the direct name is selected");
    assert_eq!(placements[0].0, "PROBE");
}

#[test]
fn wld_reference_positive_missing_and_wrong_kind_indexes_are_omitted() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("unsupported-reference.s3d");
    for (reference, typed_definition) in [(1, false), (2, true), (16, true), (i32::MAX, true)] {
        // Missing index 16 also aliases an occupied string offset (DEFINITION).
        let bytes = reference_document(&[(reference, ActorSpec::default())], typed_definition);
        archive(&path, "objects.wld", &bytes);
        assert!(
            read_placements(&path).unwrap().is_empty(),
            "positive reference {reference}, typed definition {typed_definition}"
        );
    }
}

#[test]
fn wld_reference_nonpositive_selection_is_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("direct-reference.s3d");
    for (reference, expected) in [
        (-1, Some("PROBE")),
        (0, Some("")),
        (-2, None),
        (-1_000_000, None),
        (i32::MIN, None),
    ] {
        let bytes = reference_document(&[(reference, ActorSpec::default())], true);
        archive(&path, "objects.wld", &bytes);
        let placements = read_placements(&path).unwrap();
        assert_eq!(
            placements.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(),
            expected.into_iter().collect::<Vec<_>>(),
            "nonpositive reference {reference}"
        );
    }
}

#[test]
fn wld_reference_bake_has_no_false_node_and_preserves_direct_transform() {
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("mixed-reference.s3d");
    let model = directory.path().join("model.s3d");
    let glb = directory.path().join("visual.glb");
    let direct = ActorSpec {
        rotation: [73., 109., 47.],
        scale: Some(0.005),
        radius: Some(2.),
        ..Default::default()
    };
    let bytes = reference_document(&[(1, ActorSpec::default()), (-1, direct.clone())], true);
    archive(&main, "objects.wld", &bytes);
    model_archive(&model);
    bake_zone(&main, Some(&model), &glb).unwrap();
    let (doc, buffers, _) = gltf::import(&glb).unwrap();
    let nodes: Vec<_> = doc.nodes().collect();
    assert_eq!(
        nodes.len(),
        1,
        "positive alias must not create a false node"
    );
    let node = &nodes[0];
    assert_eq!(node.mesh().unwrap().name(), Some("PROBE"));
    let matrix = node.transform().matrix();
    let primitive = node.mesh().unwrap().primitives().next().unwrap();
    let reader = primitive.reader(|b| Some(&buffers[b.index()]));
    let positions: Vec<_> = reader.read_positions().unwrap().collect();
    assert_eq!(positions.len(), 3);
    for (vertex, want) in positions.into_iter().zip(expected(&direct)) {
        for axis in 0..3 {
            let actual = matrix[0][axis] * vertex[0]
                + matrix[1][axis] * vertex[1]
                + matrix[2][axis] * vertex[2]
                + matrix[3][axis];
            assert!((actual - want[axis]).abs() < 0.00005);
        }
    }
}

#[test]
fn wld_reference_omission_diagnostic_has_original_source_context() {
    const CHILD_ARCHIVE: &str = "WLD_REFERENCE_DIAGNOSTIC_CHILD_ARCHIVE";
    if let Some(path) = std::env::var_os(CHILD_ARCHIVE) {
        assert!(read_placements(Path::new(&path)).unwrap().is_empty());
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("diagnostic-reference.s3d");
    let references: Vec<_> = [1, 2, 16, i32::MAX]
        .into_iter()
        .map(|reference| (reference, ActorSpec::default()))
        .collect();
    archive(&path, "objects.wld", &reference_document(&references, true));
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "zone::wld_placement_tests::wld_reference_omission_diagnostic_has_original_source_context",
            "--nocapture",
        ])
        .env(CHILD_ARCHIVE, &path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("diagnostic child exceeded 10 seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stderr}");
    for (slot, reference) in [(2, 1), (3, 2), (4, 16), (5, i32::MAX)] {
        let line = stderr
            .lines()
            .find(|line| line.contains(&format!("Actor slot {slot}:")))
            .unwrap_or_else(|| panic!("missing original Actor slot {slot}: {stderr}"));
        for context in [
            path.display().to_string(),
            "objects.wld".to_string(),
            format!("raw reference {reference}"),
            "unsupported fragment-reference".to_string(),
            "omitting".to_string(),
        ] {
            assert!(line.contains(&context), "expected {context}: {line}");
        }
    }
}
