use eqoxide_asset_server::static_collision::*;
use glam::{Mat4, Quat, Vec3};
use serde_json::Value;

fn fixture() -> StaticCollisionScene {
    StaticCollisionScene::Available {
        meshes: vec![StaticCollisionMesh {
            positions: vec![[1., 2., 3.], [4., 2., 3.], [1., 6., 3.]],
            primitives: vec![StaticCollisionPrimitive {
                indices: vec![0, 1, 2],
            }],
        }],
        instances: vec![StaticCollisionInstance {
            owner_id: "authored-floor".into(),
            mesh_index: 0,
            matrix: Mat4::IDENTITY.to_cols_array_2d(),
        }],
    }
}
fn write(scene: &StaticCollisionScene, limits: &CollisionWriteLimits) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("collision.glb");
    let req = write_static_collision(
        scene,
        &"a".repeat(64),
        InspectionQueryPolicy::FrontFaces,
        &out,
        limits,
    )
    .unwrap();
    assert_eq!(req.reader_version, 2);
    assert_eq!(req.capabilities, ["static-collision-triangles-v1"]);
    std::fs::read(out).unwrap()
}
fn json(bytes: &[u8]) -> Value {
    let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + len]).unwrap()
}
#[test]
fn position_only_output_preserves_orientation() {
    let bytes = write(&fixture(), &CollisionWriteLimits::default());
    let (doc, buffers, images) = gltf::import_slice(&bytes).unwrap();
    assert!(images.is_empty());
    let primitive = doc.meshes().next().unwrap().primitives().next().unwrap();
    assert_eq!(primitive.attributes().count(), 1);
    let reader = primitive.reader(|b| Some(&buffers[b.index()]));
    let positions: Vec<_> = reader.read_positions().unwrap().collect();
    assert_eq!(positions, [[1., 3., -2.], [4., 3., -2.], [1., 3., -6.]]);
    assert_eq!(
        reader
            .read_indices()
            .unwrap()
            .into_u32()
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let n = (Vec3::from_array(positions[1]) - Vec3::from_array(positions[0]))
        .cross(Vec3::from_array(positions[2]) - Vec3::from_array(positions[0]));
    assert!(n.y > 0.);
    let j = json(&bytes);
    assert_eq!(j["extras"]["eqoxideAsset"]["role"], "collision");
    assert_eq!(j["extras"]["eqoxideCollision"]["availability"], "available");
    assert_eq!(
        j["extras"]["eqoxideCollision"]["queryPolicy"],
        "inspection-segment-front-v1"
    );
    assert_eq!(
        j["nodes"][0]["extras"]["eqoxideCollisionInstance"]["ownerId"],
        "authored-floor"
    );
    assert!(j.get("materials").is_none());
    assert_eq!(j["accessors"].as_array().unwrap().len(), 2);
    assert_eq!(j["bufferViews"].as_array().unwrap().len(), 2);
}
#[test]
fn explicit_empty_has_no_binary_payload() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("empty.glb");
    write_static_collision(
        &StaticCollisionScene::AvailableEmpty,
        &"a".repeat(64),
        InspectionQueryPolicy::BothFaces,
        &out,
        &CollisionWriteLimits::default(),
    )
    .unwrap();
    let bytes = std::fs::read(out).unwrap();
    let j = json(&bytes);
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 20 + json_len);
    // Core scene nodes are optional; empty arrays are omitted, not serialized.
    assert_eq!(&bytes[..4], b"glTF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
    assert_eq!(
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
        bytes.len()
    );
    assert_eq!(&bytes[16..20], b"JSON");
    assert_eq!(j["scenes"], serde_json::json!([{}]));
    assert_eq!(
        j["extras"]["eqoxideCollision"]["availability"],
        "available-empty"
    );
    assert_eq!(
        j["extras"]["eqoxideCollision"]["selectedNodes"],
        serde_json::json!([])
    );
    assert_eq!(
        j["extras"]["eqoxideCollision"]["queryPolicy"],
        "inspection-segment-both-v1"
    );
    for key in ["buffers", "bufferViews", "accessors", "nodes", "meshes"] {
        assert!(j.get(key).is_none(), "{key}");
    }
}
fn edit(
    scene: &mut StaticCollisionScene,
) -> (
    &mut Vec<StaticCollisionMesh>,
    &mut Vec<StaticCollisionInstance>,
) {
    match scene {
        StaticCollisionScene::Available { meshes, instances } => (meshes, instances),
        _ => unreachable!(),
    }
}
fn refuses_preserving(scene: &StaticCollisionScene, revision: &str, limits: &CollisionWriteLimits) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("collision.glb");
    std::fs::write(&out, b"sentinel").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(write_static_collision(
        scene,
        revision,
        InspectionQueryPolicy::FrontFaces,
        &out,
        limits
    )
    .is_err());
    assert_eq!(std::fs::read(&out).unwrap(), b"sentinel");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&out).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
#[test]
fn invalid_states_and_all_declared_geometry_are_refused() {
    let limits = CollisionWriteLimits::default();
    for revision in ["", "aa", &"A".repeat(64), &"g".repeat(64)] {
        refuses_preserving(&fixture(), revision, &limits);
    }
    let mut cases = Vec::new();
    let mut s = fixture();
    edit(&mut s).0.clear();
    cases.push(s);
    let mut s = fixture();
    edit(&mut s).1.clear();
    cases.push(s);
    for owner in ["", "Upper", "not/a-token", "-first", &"a".repeat(129)] {
        let mut s = fixture();
        edit(&mut s).1[0].owner_id = owner.into();
        cases.push(s);
    }
    let mut s = fixture();
    let (_, i) = edit(&mut s);
    i.push(i[0].clone());
    cases.push(s);
    let mut s = fixture();
    edit(&mut s).1[0].mesh_index = 10;
    cases.push(s);
    for indices in [vec![], vec![0, 1], vec![0, 1, 9], vec![0, 1, 1]] {
        let mut s = fixture();
        edit(&mut s).0[0].primitives[0].indices = indices;
        cases.push(s);
    }
    let mut s = fixture();
    edit(&mut s).0[0].positions.push([f32::NAN, 0., 0.]);
    cases.push(s);
    let mut s = fixture();
    edit(&mut s).0[0].primitives.clear();
    cases.push(s);
    let mut s = fixture();
    edit(&mut s).0[0].positions.clear();
    cases.push(s);
    // Neither an unused mesh nor its primitive is exempt from validation.
    let mut s = fixture();
    let (m, _) = edit(&mut s);
    let mut bad = m[0].clone();
    bad.primitives[0].indices = vec![0, 0, 0];
    m.push(bad);
    cases.push(s);
    let mut s = fixture();
    edit(&mut s).0[0].primitives.push(StaticCollisionPrimitive {
        indices: vec![0, 1, u32::MAX],
    });
    cases.push(s);
    for (n, s) in cases.iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            write_static_collision(
                s,
                &"a".repeat(64),
                InspectionQueryPolicy::FrontFaces,
                &dir.path().join("bad"),
                &limits
            )
            .is_err(),
            "case {n}"
        );
        refuses_preserving(s, &"a".repeat(64), &limits);
    }
}
#[test]
fn bad_transforms_and_rounded_world_collapse_are_refused() {
    let mut matrices = vec![
        Mat4::from_scale(Vec3::new(1., 2., 1.)).to_cols_array_2d(),
        Mat4::from_scale(Vec3::new(-1., 1., 1.)).to_cols_array_2d(),
        Mat4::from_scale(Vec3::ZERO).to_cols_array_2d(),
        Mat4::from_translation(Vec3::splat(1e20)).to_cols_array_2d(),
        Mat4::from_scale(Vec3::splat(f32::MAX)).to_cols_array_2d(),
        Mat4::from_scale(Vec3::splat(f32::from_bits(1))).to_cols_array_2d(),
    ];
    let mut m = Mat4::IDENTITY.to_cols_array_2d();
    m[0][3] = 1e-9;
    matrices.push(m);
    let mut m = Mat4::IDENTITY.to_cols_array_2d();
    m[1][0] = 0.2;
    matrices.push(m);
    let mut m = Mat4::IDENTITY.to_cols_array_2d();
    m[3][0] = f32::INFINITY;
    matrices.push(m);
    // The tiny scale case collapses a triangle whose edge is too small for f32.
    for (n, matrix) in matrices.into_iter().enumerate() {
        let mut s = fixture();
        edit(&mut s).1[0].matrix = matrix;
        if n == 5 {
            edit(&mut s).0[0].positions = vec![[0., 0., 0.], [0.1, 0., 0.], [0., 0.1, 0.]];
        }
        refuses_preserving(&s, &"a".repeat(64), &CollisionWriteLimits::default());
    }
    let mut s = fixture();
    edit(&mut s).1[0].matrix = Mat4::from_scale(Vec3::splat(1e-20)).to_cols_array_2d();
    write(&s, &CollisionWriteLimits::default());
}
#[test]
fn reused_geometry_has_distinct_transformed_instances() {
    let mut s = fixture();
    let (_, instances) = edit(&mut s);
    let transform = Mat4::from_scale_rotation_translation(
        Vec3::splat(2.),
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
        Vec3::new(10., 20., 30.),
    );
    instances.push(StaticCollisionInstance {
        owner_id: "authored-second".into(),
        mesh_index: 0,
        matrix: transform.to_cols_array_2d(),
    });
    let bytes = write(&s, &CollisionWriteLimits::default());
    let j = json(&bytes);
    assert_eq!(j["meshes"].as_array().unwrap().len(), 1);
    assert_eq!(j["accessors"].as_array().unwrap().len(), 2);
    assert_eq!(j["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(
        j["extras"]["eqoxideCollision"]["selectedNodes"],
        serde_json::json!([0, 1])
    );
    let (doc, buffers, _) = gltf::import_slice(&bytes).unwrap();
    let positions: Vec<_> = doc
        .meshes()
        .next()
        .unwrap()
        .primitives()
        .next()
        .unwrap()
        .reader(|b| Some(&buffers[b.index()]))
        .read_positions()
        .unwrap()
        .collect();
    let node = doc.nodes().nth(1).unwrap();
    let matrix = Mat4::from_cols_array_2d(&node.transform().matrix());
    for (asset, source) in positions
        .iter()
        .zip([[1., 2., 3.], [4., 2., 3.], [1., 6., 3.]])
    {
        let server = transform.transform_point3(Vec3::from_array(source));
        let expected = Vec3::new(server.x, server.z, -server.y);
        assert!(matrix
            .transform_point3(Vec3::from_array(*asset))
            .abs_diff_eq(expected, 1e-5));
    }
}
#[test]
fn every_limit_is_enforced_and_exact_output_boundaries_pass() {
    let base = CollisionWriteLimits::default();
    for which in 0..8 {
        let mut l = base.clone();
        match which {
            0 => l.max_glb_bytes = 0,
            1 => l.max_json_bytes = 0,
            2 => l.max_geometry_bytes = 47,
            3 => l.max_resources = 1,
            4 => l.max_vertices = 2,
            5 => l.max_indices = 2,
            6 => l.max_expanded_triangles = 0,
            7 => l.max_world_vertices = 2,
            _ => unreachable!(),
        }
        refuses_preserving(&fixture(), &"a".repeat(64), &l);
    }
    let bytes = write(&fixture(), &base);
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut exact = base.clone();
    exact.max_glb_bytes = bytes.len();
    exact.max_json_bytes = json_length;
    exact.max_geometry_bytes = 48;
    exact.max_resources = 2;
    exact.max_vertices = 3;
    exact.max_indices = 3;
    exact.max_expanded_triangles = 1;
    exact.max_world_vertices = 3;
    assert_eq!(write(&fixture(), &exact), bytes);
    let mut l = exact.clone();
    l.max_glb_bytes -= 1;
    refuses_preserving(&fixture(), &"a".repeat(64), &l);
    let mut l = exact;
    l.max_json_bytes -= 1;
    refuses_preserving(&fixture(), &"a".repeat(64), &l);
    let mut s = fixture();
    let (_, i) = edit(&mut s);
    let mut second = i[0].clone();
    second.owner_id = "second".into();
    i.push(second);
    let mut l = base.clone();
    l.max_expanded_triangles = 1;
    refuses_preserving(&s, &"a".repeat(64), &l);
    let mut l = base.clone();
    l.max_world_vertices = 5;
    refuses_preserving(&s, &"a".repeat(64), &l);
    l.max_world_vertices = 6;
    l.max_expanded_triangles = 2;
    write(&s, &l);
}
#[test]
fn publication_sets_readable_permissions_and_failed_replace_keeps_destination() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("collision.glb");
    write_static_collision(
        &fixture(),
        &"a".repeat(64),
        InspectionQueryPolicy::FrontFaces,
        &out,
        &CollisionWriteLimits::default(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&out).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
    let destination = dir.path().join("directory");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("keep"), b"old").unwrap();
    assert!(write_static_collision(
        &fixture(),
        &"a".repeat(64),
        InspectionQueryPolicy::FrontFaces,
        &destination,
        &CollisionWriteLimits::default()
    )
    .is_err());
    assert_eq!(std::fs::read(destination.join("keep")).unwrap(), b"old");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}
#[test]
fn unused_vertices_and_multiple_primitives_share_one_position_accessor() {
    let mut s = fixture();
    let (meshes, _) = edit(&mut s);
    meshes[0].positions.push([10., 20., 30.]);
    meshes[0].primitives.push(StaticCollisionPrimitive {
        indices: vec![2, 1, 0],
    });
    let j = json(&write(&s, &CollisionWriteLimits::default()));
    assert_eq!(j["accessors"].as_array().unwrap().len(), 3);
    assert_eq!(
        j["meshes"][0]["primitives"][0]["attributes"]["POSITION"],
        j["meshes"][0]["primitives"][1]["attributes"]["POSITION"]
    );
    assert_eq!(j["accessors"][0]["min"], serde_json::json!([1., 3., -20.]));
    assert_eq!(j["accessors"][0]["max"], serde_json::json!([10., 30., -2.]));
    let mut l = CollisionWriteLimits::default();
    l.max_resources = 2;
    refuses_preserving(&s, &"a".repeat(64), &l);
}
#[test]
fn known_empty_still_obeys_scene_json_and_container_limits() {
    let mut l = CollisionWriteLimits::default();
    l.max_resources = 0;
    refuses_preserving(&StaticCollisionScene::AvailableEmpty, &"a".repeat(64), &l);
    l = CollisionWriteLimits::default();
    l.max_json_bytes = 0;
    refuses_preserving(&StaticCollisionScene::AvailableEmpty, &"a".repeat(64), &l);
    l = CollisionWriteLimits::default();
    l.max_glb_bytes = 19;
    refuses_preserving(&StaticCollisionScene::AvailableEmpty, &"a".repeat(64), &l);
    let bytes = write(
        &StaticCollisionScene::AvailableEmpty,
        &CollisionWriteLimits::default(),
    );
    l = CollisionWriteLimits::default();
    l.max_glb_bytes = bytes.len();
    l.max_json_bytes = bytes.len() - 20;
    l.max_geometry_bytes = 0;
    l.max_vertices = 0;
    l.max_indices = 0;
    l.max_expanded_triangles = 0;
    l.max_world_vertices = 0;
    l.max_resources = 1;
    assert_eq!(write(&StaticCollisionScene::AvailableEmpty, &l), bytes);
}
