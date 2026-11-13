#[allow(dead_code)]
#[path = "../examples/static_pbr_fixture.rs"]
mod fixture;

use eqoxide_asset_server::static_scene::write_static_visual_with_material_policy;
use glam::{Mat4, Vec3};

#[test]
fn synthetic_swatches_have_curved_ccw_unit_normals_shared_instances_and_real_material_intent() {
    let (scene, policy) = fixture::fixture_scene();
    assert_eq!(scene.meshes.len(), 5);
    assert_eq!(scene.materials.len(), 5);
    assert_eq!(scene.instances.len(), 7);
    assert!(scene.textures.is_empty());
    assert_eq!(
        scene.instances.iter().filter(|i| i.mesh_index == 0).count(),
        3
    );
    for mesh in &scene.meshes {
        assert!(mesh.normals.iter().any(|n| n != &mesh.normals[0]));
        for normal in &mesh.normals {
            assert!((Vec3::from_array(*normal).length_squared() - 1.).abs() < 1e-5);
        }
        for triangle in mesh.primitives[0].indices.chunks(3) {
            let [a, b, c] = triangle.try_into().unwrap();
            let positions = [a, b, c].map(|i: u32| Vec3::from_array(mesh.positions[i as usize]));
            let outward = Vec3::from_array(mesh.normals[a as usize]);
            assert!(
                (positions[1] - positions[0])
                    .cross(positions[2] - positions[0])
                    .dot(outward)
                    > 1e-6
            );
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pbr.glb");
    let report =
        write_static_visual_with_material_policy(&scene, &policy, &"a".repeat(64), &path).unwrap();
    assert_eq!(
        report.requirements.capabilities,
        ["pbr-metallic-roughness-factors-v1", "static-visual-v1"]
    );
    let bytes = std::fs::read(path).unwrap();
    let glb = gltf::Gltf::from_slice_without_validation(&bytes).unwrap();
    let bin = glb.blob.as_ref().unwrap();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for node in glb.nodes() {
        let matrix = Mat4::from_cols_array_2d(&node.transform().matrix());
        for primitive in node.mesh().unwrap().primitives() {
            let reader = primitive.reader(|_| Some(bin.as_slice()));
            for position in reader.read_positions().unwrap() {
                let p = matrix.transform_point3(Vec3::from_array(position));
                min = min.min(p);
                max = max.max(p);
            }
            for normal in reader.read_normals().unwrap() {
                let n = matrix
                    .transform_vector3(Vec3::from_array(normal))
                    .normalize();
                assert!(n.is_finite());
            }
        }
    }
    assert!(min.abs().max_element() < 8.);
    assert!(max.abs().max_element() < 8.);
    assert!(min.y < -3. && max.y >= 1.);
}
