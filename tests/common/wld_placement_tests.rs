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
