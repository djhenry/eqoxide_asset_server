//! Optional render-preview acceptance; native assets are not bundled.
use eqoxide_asset_server::eqg::{DescriptorSource, export::export_preview, load_binary_zone};

#[test]
#[ignore = "requires LIBEQ_TEST_RAW_DIR with Crescent, Guild Hall, and Anguish assets"]
fn native_binary_zones_export_shared_geometry_and_resolved_textures() {
    let root = std::env::var_os("LIBEQ_TEST_RAW_DIR")
        .expect("set LIBEQ_TEST_RAW_DIR to a client installation");
    let root = std::path::Path::new(&root);
    let output = tempfile::tempdir().unwrap();
    for (name, meshes, instances, materials, textures, vertices, omitted) in [
        ("crescent", 176, 2341, 575, 122, 169552, 1854),
        ("guildhall", 51, 86, 92, 58, 53223, 160),
        ("anguish", 214, 695, 446, 89, 345165, 480),
    ] {
        let archive = root.join(format!("{name}.eqg"));
        let loose = root.join(format!("{name}.zon"));
        let descriptor = if name == "anguish" {
            DescriptorSource::Archive("anguish.zon")
        } else {
            DescriptorSource::Loose(&loose)
        };
        let scene = load_binary_zone(&archive, descriptor).unwrap();
        let path = output.path().join(format!("{name}.glb"));
        let report = export_preview(&scene, &path).unwrap();
        assert_eq!(
            (
                report.meshes,
                report.terrain_nodes,
                report.instance_nodes,
                report.materials,
                report.textures,
                report.omitted_triangles
            ),
            (meshes, 1, instances, materials, textures, omitted),
            "{name}"
        );
        assert_eq!(report.skipped_placements.len(), 1, "{name}");
        assert_eq!(report.skipped_placements[0].index, 0, "{name}");
        assert!(report.omitted_meshes.is_empty(), "{name}");
        assert_eq!(
            report.instance_nodes + report.skipped_placements.len(),
            scene.placements.len(),
            "{name}: unaccounted placement records"
        );
        let (document, buffers, images) = gltf::import(&path).unwrap();
        assert_eq!(document.meshes().len(), meshes, "{name}");
        assert_eq!(document.nodes().len(), instances + 1, "{name}");
        assert_eq!(document.materials().len(), materials, "{name}");
        let masked: Vec<_> = document
            .materials()
            .filter(|m| m.alpha_mode() == gltf::material::AlphaMode::Mask)
            .collect();
        assert_eq!(
            masked.len(),
            if name == "crescent" {
                24
            } else {
                usize::from(name == "guildhall")
            },
            "{name}"
        );
        assert_eq!(
            report.vertex_alpha_materials.len(),
            if name == "crescent" { 24 } else { 0 }
        );
        if name == "crescent" {
            let mut colored_primitives = 0;
            for mesh in document.meshes() {
                for primitive in mesh.primitives() {
                    let reader = primitive.reader(|b| Some(&buffers[b.index()].0));
                    if let Some(colors) = reader.read_colors(0) {
                        colored_primitives += 1;
                        assert!(
                            (primitive.material().alpha_cutoff().unwrap() - 96. / 255.).abs()
                                < 1e-7
                        );
                        let colors: Vec<_> = colors.into_rgba_u8().collect();
                        for index in reader.read_indices().unwrap().into_u32() {
                            assert_eq!(colors[index as usize], [255; 4]);
                        }
                    }
                }
            }
            assert_eq!(colored_primitives, 24);
        }
        assert_eq!(report.cutout_materials.len(), masked.len());
        if name == "guildhall" {
            assert_eq!(masked[0].name(), Some("obj_chandelier.mod:dark_chain"));
            assert!((masked[0].alpha_cutoff().unwrap() - 192. / 255.).abs() < 1e-7);
            let chain = document
                .images()
                .find(|i| i.name() == Some("dark_chain_c.dds"))
                .unwrap();
            let chain = &images[chain.index()];
            assert_eq!((chain.width, chain.height), (64, 64));
            assert_eq!(chain.format, gltf::image::Format::R8G8B8A8);
            assert_eq!(
                chain
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[3] < 192)
                    .count(),
                1124
            );
            assert_eq!(
                chain
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[3] == 192)
                    .count(),
                2
            );
        }
        assert_eq!(images.len(), textures, "{name}");
        let unique_vertices: usize = document
            .meshes()
            .map(|mesh| {
                mesh.primitives()
                    .next()
                    .unwrap()
                    .reader(|b| Some(&buffers[b.index()]))
                    .read_positions()
                    .unwrap()
                    .count()
            })
            .sum();
        assert_eq!(unique_vertices, vertices, "{name}");
        for node in document.nodes() {
            assert_eq!(node.children().len(), 0, "{name}");
            assert!(
                node.transform()
                    .matrix()
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite()),
                "{name}"
            );
        }
        if name == "anguish" {
            let water = document
                .images()
                .find(|image| image.name() == Some("ra_watertest_c_01.dds"))
                .expect("water texture");
            let image = &images[water.index()];
            assert_eq!((image.width, image.height), (256, 256));
            assert_eq!(&image.pixels[..4], &[67, 91, 141, 255]);
        }
        eprintln!(
            "{name}: {meshes} shared meshes, {instances} object instances, {textures} textures"
        );
    }
}
