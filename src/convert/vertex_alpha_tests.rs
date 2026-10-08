use super::*;
use std::collections::BTreeMap;

fn fixture() -> (Vec<MeshData>, Vec<MaterialData>, Vec<NodeDef>) {
    (
        vec![MeshData {
            name: "mixed".into(),
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
            normals: vec![[0., 0., 1.]; 4],
            uvs: vec![[0., 0.]; 4],
            primitives: vec![
                PrimitiveData {
                    indices: vec![0, 1, 2],
                    material_idx: 0,
                    extras: None,
                },
                PrimitiveData {
                    indices: vec![1, 3, 2],
                    material_idx: 0,
                    extras: None,
                },
            ],
        }],
        vec![MaterialData {
            name: "mat".into(),
            texture_idx: None,
            base_color: [1.; 4],
            alpha_mode: AlphaMode::Cutout(192),
            anim: None,
        }],
        vec![NodeDef {
            mesh_idx: 0,
            matrix: None,
        }],
    )
}

#[test]
fn vertex_alpha_is_normalized_white_rgba_and_scoped_to_primitive() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("mixed.glb");
    let (meshes, materials, nodes) = fixture();
    let alpha = BTreeMap::from([((0, 0), vec![0, 191, 192, 255])]);
    write_glb_instanced_with_vertex_alpha(&out, &meshes, &materials, &[], &nodes, &alpha).unwrap();
    let (doc, buffers, _) = gltf::import(&out).unwrap();
    let mesh = doc.meshes().next().unwrap();
    let primitives: Vec<_> = mesh.primitives().collect();
    let accessor = primitives[0].get(&gltf::Semantic::Colors(0)).unwrap();
    assert!(accessor.normalized());
    assert_eq!(accessor.data_type(), gltf::accessor::DataType::U8);
    assert_eq!(accessor.dimensions(), gltf::accessor::Dimensions::Vec4);
    let colors: Vec<_> = primitives[0]
        .reader(|b| Some(&buffers[b.index()]))
        .read_colors(0)
        .unwrap()
        .into_rgba_u8()
        .collect();
    assert_eq!(
        colors,
        vec![
            [255, 255, 255, 0],
            [255, 255, 255, 191],
            [255, 255, 255, 192],
            [255, 255, 255, 255]
        ]
    );
    assert!(primitives[1].get(&gltf::Semantic::Colors(0)).is_none());
}

#[test]
fn invalid_vertex_alpha_rejects_before_replacing_output() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("existing.glb");
    let (meshes, materials, nodes) = fixture();
    for alpha in [
        BTreeMap::from([((1, 0), vec![255; 4])]),
        BTreeMap::from([((0, 2), vec![255; 4])]),
        BTreeMap::from([((0, 0), vec![255; 3])]),
    ] {
        fs::write(&out, b"existing").unwrap();
        assert!(
            write_glb_instanced_with_vertex_alpha(&out, &meshes, &materials, &[], &nodes, &alpha)
                .is_err()
        );
        assert_eq!(fs::read(&out).unwrap(), b"existing");
    }
}

#[test]
fn empty_vertex_alpha_preserves_default_writer_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.glb");
    let b = dir.path().join("b.glb");
    let (meshes, materials, nodes) = fixture();
    write_glb_instanced(&a, &meshes, &materials, &[], &nodes).unwrap();
    write_glb_instanced_with_vertex_alpha(&b, &meshes, &materials, &[], &nodes, &BTreeMap::new())
        .unwrap();
    assert_eq!(fs::read(a).unwrap(), fs::read(b).unwrap());
}
