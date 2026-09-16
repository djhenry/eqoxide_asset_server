//! Optional native conversion regression checks. No game assets are bundled.
use eqoxide_asset_server::convert::eqg_to_glb_model;

#[test]
#[ignore = "requires LIBEQ_TEST_RAW_DIR containing row.eqg and shi.eqg"]
fn native_boats_keep_geometry_textures_and_deterministic_output() {
    let root = std::env::var_os("LIBEQ_TEST_RAW_DIR")
        .expect("set LIBEQ_TEST_RAW_DIR to a client installation");
    let root = std::path::Path::new(&root);
    let output = tempfile::tempdir().unwrap();
    for (name, vertex_count, index_counts, bounds) in [
        (
            "row",
            393,
            vec![378, 192, 198],
            (
                [-14.881587, -3.9823167, -8.30496],
                [22.851353, 5.9628544, 8.413661],
            ),
        ),
        (
            "shi",
            5422,
            vec![3579, 3297, 84, 24, 156, 438, 12],
            (
                [-113.52693, -24.166739, -46.376976],
                [134.14412, 195.70358, 46.97227],
            ),
        ),
    ] {
        let source = root.join(format!("{name}.eqg"));
        let first = output.path().join(format!("{name}-first.glb"));
        let second = output.path().join(format!("{name}-second.glb"));
        eqg_to_glb_model(&source, &first).unwrap();
        eqg_to_glb_model(&source, &second).unwrap();
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&second).unwrap(),
            "{name}: repeated exports differ"
        );
        let (document, buffers, images) = gltf::import(&first).unwrap();
        assert_eq!(document.meshes().len(), 1, "{name}");
        assert_eq!(document.materials().len(), index_counts.len(), "{name}");
        assert_eq!(images.len(), index_counts.len(), "{name}");
        assert_eq!(document.skins().len(), 0, "boat conversion remains static");
        let mesh = document.meshes().next().unwrap();
        assert_eq!(mesh.primitives().len(), index_counts.len(), "{name}");
        for (material_index, (primitive, expected_indices)) in
            mesh.primitives().zip(index_counts).enumerate()
        {
            assert_eq!(primitive.material().index(), Some(material_index), "{name}");
            assert!(
                primitive
                    .material()
                    .pbr_metallic_roughness()
                    .base_color_texture()
                    .is_some(),
                "{name}: diffuse texture missing"
            );
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
            let positions: Vec<_> = reader.read_positions().unwrap().collect();
            assert_eq!(positions.len(), vertex_count, "{name}");
            let indices: Vec<_> = reader.read_indices().unwrap().into_u32().collect();
            assert_eq!(indices.len(), expected_indices, "{name}");
            assert!(
                indices.iter().all(|&index| index < vertex_count as u32),
                "{name}"
            );
            assert_eq!(
                reader.read_normals().unwrap().count(),
                vertex_count,
                "{name}"
            );
            assert_eq!(
                reader.read_tex_coords(0).unwrap().into_f32().count(),
                vertex_count,
                "{name}"
            );
            for axis in 0..3 {
                let min = positions
                    .iter()
                    .map(|p| p[axis])
                    .fold(f32::INFINITY, f32::min);
                let max = positions
                    .iter()
                    .map(|p| p[axis])
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!(
                    (min - bounds.0[axis]).abs() < 0.0001,
                    "{name}: min axis {axis}"
                );
                assert!(
                    (max - bounds.1[axis]).abs() < 0.0001,
                    "{name}: max axis {axis}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires LIBEQ_TEST_RAW_DIR containing anguish.eqg"]
fn native_version_two_model_exports_primary_uvs_and_texture() {
    use libeq_pfs::{PfsReader, PfsWriter};
    use std::{fs::File, io::Cursor};
    let root = std::env::var_os("LIBEQ_TEST_RAW_DIR")
        .expect("set LIBEQ_TEST_RAW_DIR to a client installation");
    let output = tempfile::tempdir().unwrap();
    let mut source =
        PfsReader::open(File::open(std::path::Path::new(&root).join("anguish.eqg")).unwrap())
            .unwrap();
    // Isolate a known v2 model so the single-model selection heuristic selects it.
    let input = output.path().join("arch.eqg");
    let mut archive = PfsWriter::create(File::create(&input).unwrap()).unwrap();
    for member in ["obj_arch01.mod", "av_prison04_c.dds"] {
        let bytes = source
            .get(member)
            .unwrap()
            .expect("required fixture member");
        archive.insert(member, Cursor::new(bytes)).unwrap();
    }
    archive.finish().unwrap();
    let glb = output.path().join("arch.glb");
    eqg_to_glb_model(&input, &glb).unwrap();
    let (document, buffers, images) = gltf::import(&glb).unwrap();
    assert_eq!(document.meshes().len(), 1);
    assert_eq!(document.materials().len(), 1);
    assert_eq!(images.len(), 1);
    let mesh = document.meshes().next().unwrap();
    assert_eq!(mesh.primitives().len(), 1);
    let primitive = mesh.primitives().next().unwrap();
    let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
    assert_eq!(reader.read_positions().unwrap().count(), 234);
    assert_eq!(reader.read_indices().unwrap().into_u32().count(), 288);
    let first_uv = reader
        .read_tex_coords(0)
        .unwrap()
        .into_f32()
        .next()
        .unwrap();
    assert_eq!(first_uv, [0.7523012, -0.78412485]);
    assert!(
        primitive
            .material()
            .pbr_metallic_roughness()
            .base_color_texture()
            .is_some()
    );
}
