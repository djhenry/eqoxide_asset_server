//! Optional source-assembly acceptance; no native game data is bundled.
use eqoxide_asset_server::eqg::{DescriptorSource, load_binary_zone};

#[test]
#[ignore = "requires LIBEQ_TEST_RAW_DIR containing Crescent, Guild Hall, and Anguish assets"]
fn native_binary_zones_resolve_their_model_tables() {
    let root = std::env::var_os("LIBEQ_TEST_RAW_DIR")
        .expect("set LIBEQ_TEST_RAW_DIR to a client installation");
    let root = std::path::Path::new(&root);
    for (
        name,
        version,
        slots,
        meshes,
        placements,
        regions,
        lights,
        extensions,
        vertices,
        bad_uv1,
    ) in [
        ("crescent", 2, 176, 176, 2342, 58, 0, 646120, 169552, 5),
        ("guildhall", 2, 51, 51, 87, 0, 46, 76293, 53223, 0),
        ("anguish", 1, 215, 214, 696, 2, 452, 0, 345165, 0),
    ] {
        let archive = root.join(format!("{name}.eqg"));
        let loose = root.join(format!("{name}.zon"));
        let source = if name == "anguish" {
            DescriptorSource::Archive("anguish.zon")
        } else {
            DescriptorSource::Loose(&loose)
        };
        let scene = load_binary_zone(&archive, source).unwrap();
        assert_eq!(scene.version, version, "{name}");
        assert_eq!(scene.model_slots.len(), slots, "{name}");
        assert_eq!(scene.meshes.len(), meshes, "{name}");
        assert_eq!(scene.placements.len(), placements, "{name}");
        assert_eq!(scene.regions.len(), regions, "{name}");
        assert_eq!(scene.lights.len(), lights, "{name}");
        assert_eq!(
            scene
                .placements
                .iter()
                .map(|p| p.extension_words().count())
                .sum::<usize>(),
            extensions,
            "{name}"
        );
        assert_eq!(
            scene.meshes.iter().map(|m| m.vertices.len()).sum::<usize>(),
            vertices,
            "{name}"
        );
        let secondary_invalid = scene
            .meshes
            .iter()
            .flat_map(|m| &m.vertices)
            .filter(|v| v.uv1.is_some_and(|uv| uv.iter().any(|x| !x.is_finite())))
            .count();
        assert_eq!(secondary_invalid, bad_uv1, "{name}");
        assert!(scene.trailing_data.is_empty(), "{name}");
        assert!(
            scene.meshes.iter().all(|m| m.trailing_data.is_empty()),
            "{name}"
        );
        eprintln!(
            "{name}: {slots} model slots -> {meshes} meshes, {placements} placement records, {vertices} unique-mesh vertices"
        );
    }
}
