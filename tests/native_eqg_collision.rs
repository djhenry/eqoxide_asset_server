//! Optional collision-export regression; native assets are not bundled.
use eqoxide_asset_server::eqg::{DescriptorSource, collision::export_collision, load_binary_zone};
use libeq_eqg::mesh::MeshKind;

#[test]
#[ignore = "requires LIBEQ_TEST_RAW_DIR with Crescent assets"]
fn crescent_collision_counts_match_source_candidates_and_glb() {
    let root = std::env::var_os("LIBEQ_TEST_RAW_DIR")
        .expect("set LIBEQ_TEST_RAW_DIR to a client installation");
    let root = std::path::Path::new(&root);
    let scene = load_binary_zone(
        &root.join("crescent.eqg"),
        DescriptorSource::Loose(&root.join("crescent.zon")),
    ).unwrap();
    // Count eligible source faces independently of the export report, expanded
    // over actual object placements and terrain once. ZON v2 record zero is lighting.
    let terrain = scene.meshes.iter().position(|m| m.kind == MeshKind::Terrain).unwrap();
    let eligible = |index: usize| scene.meshes[index].triangles.iter()
        .filter(|triangle| triangle.flags & 1 == 0).count();
    let mut source_candidates = eligible(terrain);
    for placement in scene.placements.iter().skip(1) {
        if let Some(slot) = placement.model_index {
            let mesh = scene.model_slots[slot as usize].unwrap();
            if mesh != terrain { source_candidates += eligible(mesh); }
        }
    }
    assert_eq!(scene.version, 2);
    assert_eq!(source_candidates, 300_775);

    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("crescent-collision.glb");
    let report = export_collision(&scene, &path).unwrap();
    assert_eq!(report.emitted_triangles, 300_306);
    assert_eq!(report.degenerate_triangles, 469);
    assert_eq!(report.emitted_triangles + report.degenerate_triangles, source_candidates);
    assert_eq!((report.terrain_instances, report.object_instances), (1, 2341));
    let mut reported_candidates = 0;
    for counts in report.placement_expanded_flags.values() {
        assert_eq!(counts.considered, counts.excluded + counts.degenerate + counts.emitted);
        reported_candidates += counts.considered - counts.excluded;
    }
    assert_eq!(reported_candidates, source_candidates);

    let (document, buffers, images) = gltf::import(&path).unwrap();
    let extras: serde_json::Value = serde_json::from_str(
        document.as_json().extras.as_ref().unwrap().get(),
    ).unwrap();
    assert_eq!(extras["eqCollision"], serde_json::json!({
        "version": 1, "coordinates": "eqg_gltf_y_up",
        "scope": "default_static_triangle_candidates", "nodes": [0]
    }));
    assert_eq!(document.nodes().len(), 1);
    assert_eq!(document.meshes().len(), 1);
    assert!(images.is_empty());
    let node = document.nodes().next().unwrap();
    assert_eq!(node.name(), Some("__collision__"));
    assert_eq!(node.transform().matrix(), glam::Mat4::IDENTITY.to_cols_array_2d());
    assert_eq!(node.children().len(), 0);
    assert_eq!(document.default_scene().unwrap().nodes().map(|n| n.index()).collect::<Vec<_>>(), [0]);
    let mesh = node.mesh().unwrap();
    assert_eq!(mesh.primitives().len(), 1);
    let primitive = mesh.primitives().next().unwrap();
    assert_eq!(primitive.mode(), gltf::mesh::Mode::Triangles);
    let reader = primitive.reader(|b| Some(&buffers[b.index()]));
    let positions: Vec<_> = reader.read_positions().unwrap().collect();
    let indices: Vec<_> = reader.read_indices().unwrap().into_u32().collect();
    assert_eq!(positions.len(), report.emitted_triangles * 3);
    assert_eq!(indices.len(), report.emitted_triangles * 3);
    assert!(positions.iter().flatten().all(|v| v.is_finite()));
    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]]
            .map(|i| glam::Vec3::from_array(positions[i as usize]).as_dvec3());
        assert!((b - a).cross(c - a).length_squared() > 0.0);
    }
}
