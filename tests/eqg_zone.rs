use eqoxide_asset_server::eqg::{DescriptorSource, load_binary_zone};
use libeq_pfs::PfsWriter;
use std::{io::Cursor, path::Path};

fn words(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut pfs = PfsWriter::create(std::fs::File::create(path).unwrap()).unwrap();
    for (name, data) in entries {
        pfs.insert(*name, Cursor::new(data)).unwrap();
    }
    pfs.finish().unwrap();
}
fn descriptor(version: u32, scale: f32) -> Vec<u8> {
    let strings = b"shape.mod\0SHAPE.MOD\0placement\0";
    let mut out = b"EQGZ".to_vec();
    words(&mut out, &[version, strings.len() as u32, 3, 2, 1, 1]);
    out.extend(strings);
    words(&mut out, &[0, u32::MAX, 10]);
    for model in [2, u32::MAX] {
        words(
            &mut out,
            &[
                model,
                20,
                1f32.to_bits(),
                2f32.to_bits(),
                3f32.to_bits(),
                4f32.to_bits(),
                5f32.to_bits(),
                6f32.to_bits(),
                scale.to_bits(),
            ],
        );
        if version == 2 {
            words(&mut out, &[2, 0x11223344, 0x55667788]);
        }
    }
    words(&mut out, &[20, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    words(&mut out, &[20, 11, 12, 13, 14, 15, 16, 17]);
    out.extend(b"zone suffix");
    out
}
fn mesh() -> Vec<u8> {
    let mut out = b"EQGM".to_vec();
    words(&mut out, &[3, 0, 0, 1, 1, 7]);
    words(
        &mut out,
        &[
            1f32.to_bits(),
            2f32.to_bits(),
            3f32.to_bits(),
            0,
            0,
            1f32.to_bits(),
            0xaabbccdd,
            0,
            1f32.to_bits(),
            2f32.to_bits(),
            3f32.to_bits(),
        ],
    );
    words(&mut out, &[0, 0, 0, u32::MAX, 0x76543210]);
    out.extend(b"bone suffix");
    out
}
#[test]
fn assembles_loose_and_embedded_versions_preserving_slots_and_source_fields() {
    for version in [1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zone.eqg");
        let zon = descriptor(version, 2.0);
        let geometry = mesh();
        archive(&path, &[("Shape.Mod", &geometry), ("Zone.ZON", &zon)]);
        let loose = dir.path().join("loose.zon");
        std::fs::write(&loose, &zon).unwrap();
        for source in [
            DescriptorSource::Loose(&loose),
            DescriptorSource::Archive("zone.zon"),
        ] {
            let scene = load_binary_zone(&path, source).unwrap();
            assert_eq!(scene.version, version);
            assert_eq!(scene.model_name_offsets, vec![Some(0), None, Some(10)]);
            assert_eq!(scene.model_slots, vec![Some(0), None, Some(0)]);
            assert_eq!(scene.meshes.len(), 1);
            let mesh = &scene.meshes[0];
            assert_eq!(mesh.source_name, "Shape.Mod");
            assert_eq!(mesh.bone_count, Some(7));
            assert_eq!(mesh.vertices[0].color, Some(0xaabbccdd));
            assert_eq!(mesh.vertices[0].uv1, Some([2., 3.]));
            assert_eq!(mesh.triangles[0].material_index, u32::MAX);
            assert_eq!(mesh.triangles[0].flags, 0x76543210);
            assert_eq!(mesh.trailing_data, b"bone suffix");
            assert_eq!(scene.placements[0].model_index, Some(2));
            assert_eq!(scene.placements[1].model_index, None);
            assert_eq!(scene.placements[0].rotation, [4., 5., 6.]);
            assert_eq!(scene.placements[0].scale, 2.);
            assert_eq!(
                scene.placements[0].extension_words().collect::<Vec<_>>(),
                if version == 2 {
                    vec![0x11223344, 0x55667788]
                } else {
                    vec![]
                }
            );
            assert_eq!(scene.regions[0].data, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
            assert_eq!(scene.lights[0].data, [11, 12, 13, 14, 15, 16, 17]);
            assert_eq!(scene.trailing_data, b"zone suffix");
        }
    }
}
#[test]
fn requested_missing_ambiguous_and_invalid_models_have_context() {
    for (entries, expected) in [
        (vec![], "missing"),
        (
            vec![("Shape.Mod", mesh()), ("shape.mod", mesh())],
            "ambiguous",
        ),
        (vec![("shape.mod", b"bad mesh".to_vec())], "magic"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zone.eqg");
        let zon = descriptor(1, 1.);
        let mut refs = vec![("zone.zon", zon.as_slice())];
        refs.extend(entries.iter().map(|(n, b)| (*n, b.as_slice())));
        archive(&path, &refs);
        let error = format!(
            "{:#}",
            load_binary_zone(&path, DescriptorSource::Archive("zone.zon")).unwrap_err()
        );
        for context in ["zone.zon", "slot 0", "shape.mod", expected] {
            assert!(error.contains(context), "{error}");
        }
    }
}
#[test]
fn rejects_nonfinite_scene_values() {
    for (zon, geometry, context) in [
        (descriptor(2, f32::NAN), mesh(), "placement 0"),
        (
            descriptor(1, 1.),
            {
                let mut m = mesh();
                m[28..32].copy_from_slice(&f32::INFINITY.to_le_bytes());
                m
            },
            "vertex 0",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zone.eqg");
        archive(&path, &[("zone.zon", &zon), ("shape.mod", &geometry)]);
        let error = format!(
            "{:#}",
            load_binary_zone(&path, DescriptorSource::Archive("zone.zon")).unwrap_err()
        );
        assert!(error.contains(context), "{error}");
    }
}
#[test]
fn rejects_nonfinite_normals_and_primary_uvs() {
    for offset in [40, 56] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zone.eqg");
        let zon = descriptor(1, 1.);
        let mut geometry = mesh();
        geometry[offset..offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        archive(&path, &[("zone.zon", &zon), ("shape.mod", &geometry)]);
        let error = format!(
            "{:#}",
            load_binary_zone(&path, DescriptorSource::Archive("zone.zon")).unwrap_err()
        );
        assert!(error.contains("vertex 0"), "{error}");
    }
}
#[test]
fn resolves_unused_models_and_retains_reference_to_null_slot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zone.eqg");
    let mut zon = descriptor(1, 1.);
    // Both placement records now reference the null table slot; the model table
    // still names geometry that must be resolved, even without an actor reference.
    let placements = 28 + 30 + 12;
    for offset in [placements, placements + 36] {
        zon[offset..offset + 4].copy_from_slice(&1u32.to_le_bytes());
    }
    let geometry = mesh();
    archive(&path, &[("zone.zon", &zon), ("shape.mod", &geometry)]);
    let scene = load_binary_zone(&path, DescriptorSource::Archive("zone.zon")).unwrap();
    assert_eq!(scene.meshes.len(), 1);
    assert_eq!(scene.placements[0].model_index, Some(1));
    archive(&path, &[("zone.zon", &zon)]);
    let error = format!(
        "{:#}",
        load_binary_zone(&path, DescriptorSource::Archive("zone.zon")).unwrap_err()
    );
    assert!(error.contains("missing archive member"), "{error}");
}
#[test]
fn rejects_unsupported_geometry_and_ambiguous_descriptors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zone.eqg");
    let zon = descriptor(1, 1.);
    let mut geometry = mesh();
    geometry[4..8].copy_from_slice(&99u32.to_le_bytes());
    archive(&path, &[("zone.zon", &zon), ("shape.mod", &geometry)]);
    let error = format!(
        "{:#}",
        load_binary_zone(&path, DescriptorSource::Archive("zone.zon")).unwrap_err()
    );
    for context in [
        "zone.zon",
        "slot 0",
        "shape.mod",
        "unsupported mesh version 99",
    ] {
        assert!(error.contains(context), "{error}");
    }
    archive(&path, &[("zone.zon", &zon), ("ZONE.ZON", &zon)]);
    let error = format!(
        "{:#}",
        load_binary_zone(&path, DescriptorSource::Archive("zone.zon")).unwrap_err()
    );
    assert!(error.contains("ambiguous archive member"), "{error}");
}

#[test]
fn preserves_and_reports_nonfinite_secondary_uvs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zone.eqg");
    let zon = descriptor(1, 1.);
    let mut geometry = mesh();
    let nan = 0x7fc01234u32;
    geometry[64..68].copy_from_slice(&nan.to_le_bytes());
    archive(&path, &[("zone.zon", &zon), ("shape.mod", &geometry)]);
    let scene = load_binary_zone(&path, DescriptorSource::Archive("zone.zon")).unwrap();
    assert_eq!(scene.meshes[0].vertices[0].uv1.unwrap()[0].to_bits(), nan);
    let report = eqoxide_asset_server::eqg::report::summary(&scene);
    assert_eq!(
        report["meshes"][0]["vertices_with_nonfinite_secondary_uv"],
        1
    );
}

#[test]
fn retains_material_strings_and_unknown_properties() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zone.eqg");
    let strings = b"mat\0shader\0name\0tex\0";
    let mut geometry = b"EQGM".to_vec();
    words(&mut geometry, &[3, strings.len() as u32, 1, 1, 1, 7]);
    geometry.extend(strings);
    words(&mut geometry, &[42, 0, 4, 2, 11, 2, 16, 11, 77, 0x7fc01234]);
    geometry.extend(&mesh()[28..]);
    let zon = descriptor(1, 1.);
    archive(&path, &[("zone.zon", &zon), ("shape.mod", &geometry)]);
    let scene = load_binary_zone(&path, DescriptorSource::Archive("zone.zon")).unwrap();
    let model = &scene.meshes[0];
    assert_eq!(model.string_table, strings);
    let material = &model.materials[0];
    assert_eq!(
        (material.index, material.name_offset, material.shader_offset),
        (42, 0, 4)
    );
    assert_eq!(material.properties.len(), 2);
    assert_eq!(
        (
            material.properties[0].name_offset,
            material.properties[0].kind,
            material.properties[0].value
        ),
        (11, 2, 16)
    );
    assert_eq!(
        (
            material.properties[1].name_offset,
            material.properties[1].kind,
            material.properties[1].value
        ),
        (11, 77, 0x7fc01234)
    );
}
