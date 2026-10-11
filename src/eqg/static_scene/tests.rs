use super::*;
use crate::{
    eqg::{ZoneMesh, ZonePlacement},
    static_scene::StaticAlphaMode,
};
use glam::{Mat4, Vec3};
use libeq_eqg::mesh::{Material, MeshKind, Triangle, Vertex};

fn mesh(name: &str, kind: MeshKind) -> ZoneMesh {
    ZoneMesh {
        source_name: name.into(),
        kind,
        version: 1,
        string_table: b"m\0s\0".to_vec(),
        materials: vec![Material {
            index: 42,
            name_offset: 0,
            shader_offset: 2,
            properties: vec![],
        }],
        vertices: [[1., 2., 3.], [4., 2., 4.], [1., 6., 5.]]
            .map(|position| Vertex {
                position,
                normal: [-2. / 7., -3. / 7., 6. / 7.],
                color: None,
                uv0: [0.25, 0.75],
                uv1: None,
            })
            .into(),
        triangles: vec![Triangle {
            vertex_indices: [0, 1, 2],
            material_index: 0,
            flags: 3,
        }],
        bone_count: if kind == MeshKind::Model {
            Some(0)
        } else {
            None
        },
        uv_marker: None,
        trailing_data: vec![],
    }
}
fn source() -> BinaryZoneScene {
    BinaryZoneScene {
        archive_path: "synthetic.eqg".into(),
        descriptor_name: "synthetic.zon".into(),
        descriptor_location: "synthetic",
        version: 1,
        string_table: b"object.mod\0terrain.ter\0instance\0".to_vec(),
        model_name_offsets: vec![Some(0), Some(11), Some(0), None],
        model_slots: vec![Some(0), Some(1), Some(0), None],
        meshes: vec![
            mesh("object.mod", MeshKind::Model),
            mesh("terrain.ter", MeshKind::Terrain),
        ],
        placements: vec![ZonePlacement {
            model_index: Some(2),
            name_offset: 23,
            position: [13., -7., 5.],
            rotation: [0.37, -0.61, 0.83],
            scale: 2.25,
            extension_data: vec![],
        }],
        regions: vec![],
        lights: vec![],
        trailing_data: vec![],
    }
}
fn resolved() -> ResolvedZoneVisual {
    ResolvedZoneVisual {
        materials: vec![StaticMaterial {
            name: "authored".into(),
            base_color: [0.8, 0.6, 0.3, 1.],
            texture_index: None,
            alpha_mode: StaticAlphaMode::Opaque,
            double_sided: false,
        }],
        textures: vec![],
        bindings: [((0, 0), binding()), ((1, 0), binding())].into(),
    }
}
fn binding() -> MaterialBinding {
    MaterialBinding {
        material_index: 0,
        colors: ResolvedColors::Unused,
        secondary_uv: SecondaryUvUse::Unused,
    }
}
fn build(s: &BinaryZoneScene) -> BridgedStaticScene {
    build_static_scene(s, resolved(), &BridgeLimits::default()).unwrap()
}
#[test]
fn axes_normals_winding_and_source_ordinal_are_preserved_once() {
    let s = source();
    let b = build(&s);
    let m = &b.scene.meshes[b.report.source_to_visual_mesh[0].unwrap()];
    assert_eq!(m.positions[0], [2., 1., 3.]);
    assert_eq!(m.normals[0], [-3. / 7., -2. / 7., 6. / 7.]);
    assert_eq!(m.primitives[0].indices, [0, 2, 1]);
    let p = m
        .positions
        .iter()
        .map(|p| Vec3::from_array(*p))
        .collect::<Vec<_>>();
    assert!(
        (p[2] - p[0])
            .cross(p[1] - p[0])
            .dot(Vec3::from_array(m.normals[0]))
            > 0.
    );
    assert_eq!(s.meshes[0].materials[0].index, 42);
    assert_eq!(s.meshes[0].triangles[0].flags, 3);
}
fn expected_world(p: [f32; 3], placement: &ZonePlacement) -> [f64; 3] {
    let [mut x, mut y, mut z] = p.map(|v| v as f64 * placement.scale as f64);
    let (sx, cx) = (placement.rotation[2] as f64).sin_cos();
    (y, z) = (cx * y - sx * z, sx * y + cx * z);
    let (sy, cy) = (placement.rotation[1] as f64).sin_cos();
    (x, z) = (cy * x + sy * z, -sy * x + cy * z);
    let (sz, cz) = (placement.rotation[0] as f64).sin_cos();
    (x, y) = (cz * x - sz * y, sz * x + cz * y);
    [
        y + placement.position[1] as f64,
        x + placement.position[0] as f64,
        z + placement.position[2] as f64,
    ]
}
#[test]
fn noncommuting_rotation_scale_translation_and_slot_indirection() {
    let s = source();
    let b = build(&s);
    assert_eq!(
        b.report.instance_sources,
        [InstanceSource::Terrain, InstanceSource::Placement(0)]
    );
    assert_eq!(
        b.scene.instances[0].matrix,
        Mat4::IDENTITY.to_cols_array_2d()
    );
    let i = &b.scene.instances[1];
    let m = &b.scene.meshes[i.mesh_index];
    for (raw, server) in s.meshes[0].vertices.iter().zip(&m.positions) {
        let got = Mat4::from_cols_array_2d(&i.matrix)
            .transform_point3(Vec3::from_array(*server))
            .to_array();
        let expected = expected_world(raw.position, &s.placements[0]);
        for k in 0..3 {
            assert!(
                (got[k] as f64 - expected[k]).abs() < 1e-5,
                "{got:?} vs {expected:?}"
            );
        }
    }
}
#[test]
fn descriptor_v2_first_placement_is_metadata_not_a_visual_default() {
    let mut s = source();
    s.version = 2;
    s.placements[0].scale = -1.;
    s.placements[0].extension_data = vec![9; 4];
    let b = build(&s);
    assert_eq!(b.scene.instances.len(), 1);
    assert_eq!(
        b.report.excluded_placements[0].role,
        ExcludedPlacementRole::LightingMetadata
    );
    assert_eq!(b.report.excluded_placements[0].extension_bytes, 4);
    assert_eq!(b.report.source_to_visual_mesh, [None, Some(0)]);
}

fn error(s: &BinaryZoneScene, r: ResolvedZoneVisual, needle: &str) {
    let e = build_static_scene(s, r, &BridgeLimits::default()).unwrap_err();
    let text = format!("{e:#}");
    assert!(text.contains(needle), "expected {needle:?}, got {text:?}");
}
#[test]
fn reusable_meshes_null_unused_slots_and_explicit_skip_roles() {
    let mut s = source();
    s.placements.push(ZonePlacement {
        model_index: Some(0),
        name_offset: 23,
        position: [-2., 5., 1.],
        rotation: [-0.6, 0.2, 0.7],
        scale: 0.5,
        extension_data: vec![],
    });
    s.placements.push(ZonePlacement {
        model_index: Some(1),
        name_offset: 23,
        position: [99., 88., 77.],
        rotation: [0.; 3],
        scale: 0.,
        extension_data: vec![],
    });
    s.placements.push(ZonePlacement {
        model_index: None,
        name_offset: 23,
        position: [0.; 3],
        rotation: [0.; 3],
        scale: -2.,
        extension_data: vec![],
    });
    let b = build(&s);
    assert_eq!(b.scene.meshes.len(), 2);
    assert_eq!(b.scene.instances.len(), 3);
    assert_eq!(
        b.scene.instances[1].mesh_index,
        b.scene.instances[2].mesh_index
    );
    assert_eq!(
        b.report
            .excluded_placements
            .iter()
            .map(|p| p.role)
            .collect::<Vec<_>>(),
        [
            ExcludedPlacementRole::RepeatedTerrain,
            ExcludedPlacementRole::Unassigned
        ]
    );
    for (instance, placement) in [
        (&b.scene.instances[1], &s.placements[0]),
        (&b.scene.instances[2], &s.placements[1]),
    ] {
        let got = Mat4::from_cols_array_2d(&instance.matrix)
            .transform_point3(Vec3::from_array([2., 1., 3.]))
            .to_array();
        let expected = expected_world([1., 2., 3.], placement);
        for k in 0..3 {
            assert!((got[k] as f64 - expected[k]).abs() < 1e-5);
        }
    }
    s.placements[0].model_index = Some(3);
    error(&s, resolved(), "placed model slot is null");
}
#[test]
fn terrain_count_and_empty_placed_visual_fail() {
    let mut s = source();
    s.meshes[1].kind = MeshKind::Model;
    s.meshes[1].bone_count = Some(0);
    error(&s, resolved(), "missing terrain");
    let mut s = source();
    s.meshes[0].kind = MeshKind::Terrain;
    s.meshes[0].bone_count = None;
    error(&s, resolved(), "multiple distinct terrain");
    let mut s = source();
    s.meshes[0].triangles[0].material_index = u32::MAX;
    error(&s, resolved(), "no visual primitives");
    let mut s = source();
    s.meshes[1].triangles.clear();
    error(&s, resolved(), "no visual primitives");
}
#[test]
fn sentinel_exclusion_does_not_hide_malformed_indices_or_materials() {
    let mut s = source();
    s.meshes[0].triangles.push(Triangle {
        vertex_indices: [0, 2, 1],
        material_index: u32::MAX,
        flags: 0,
    });
    let b = build(&s);
    assert_eq!(b.report.sentinel_triangles, [1, 0]);
    assert_eq!(b.scene.meshes[0].primitives[0].indices.len(), 3);
    s.meshes[0].triangles[1].vertex_indices[0] = 3;
    error(&s, resolved(), "triangle vertex out of range");
    s.meshes[0].triangles[1].vertex_indices[0] = 0;
    s.meshes[0].triangles[1].material_index = 42;
    error(
        &s,
        resolved(),
        "triangle material table ordinal out of range",
    );
}
#[test]
fn material_bindings_are_required_by_table_ordinal_and_validated_when_unused() {
    let s = source();
    let mut r = resolved();
    r.bindings.remove(&(0, 0));
    error(&s, r, "lacks a binding");
    let mut r = resolved();
    r.bindings.insert((0, 42), binding());
    error(&s, r, "binding source material table ordinal");
    let mut s = source();
    s.placements.clear();
    let mut r = resolved();
    r.bindings.get_mut(&(0, 0)).unwrap().material_index = 1;
    error(&s, r, "binding global material");
    let mut r = resolved();
    r.bindings.insert((42, 0), binding());
    error(&s, r, "binding mesh");
    let b = build(&s);
    assert_eq!(b.report.unused_bindings, [(0, 0)]);
    assert_eq!(b.report.source_to_visual_mesh, [None, Some(0)]);
}
#[test]
fn primitive_rgba_isolation_preserves_shared_pool_and_source_facts() {
    let mut s = source();
    s.meshes[0].materials.push(Material {
        index: 99,
        name_offset: 0,
        shader_offset: 2,
        properties: vec![],
    });
    s.meshes[0].triangles.push(Triangle {
        vertex_indices: [0, 2, 1],
        material_index: 1,
        flags: 2,
    });
    let mut r = resolved();
    r.bindings.get_mut(&(0, 0)).unwrap().colors =
        ResolvedColors::Rgba(vec![[0.2, 0.3, 0.4, 0.25]; 3]);
    let mut other = binding();
    other.colors = ResolvedColors::Rgba(vec![[0.8, 0.7, 0.6, 0.75]; 3]);
    r.bindings.insert((0, 1), other);
    let b = build_static_scene(&s, r, &BridgeLimits::default()).unwrap();
    let m = &b.scene.meshes[0];
    assert_eq!(m.positions.len(), 3);
    assert_eq!(m.primitives.len(), 2);
    assert_eq!(
        m.primitives[0].material_index,
        m.primitives[1].material_index
    );
    assert_eq!(
        m.primitives[0].colors.as_ref().unwrap()[0],
        [0.2, 0.3, 0.4, 0.25]
    );
    assert_eq!(
        m.primitives[1].colors.as_ref().unwrap()[0],
        [0.8, 0.7, 0.6, 0.75]
    );
    assert_eq!(s.meshes[0].materials[1].index, 99);
}
#[test]
fn binding_color_shape_range_and_secondary_sampling_fail_closed() {
    let s = source();
    let mut r = resolved();
    r.bindings.get_mut(&(0, 0)).unwrap().colors = ResolvedColors::Rgba(vec![[1.; 4]; 2]);
    error(&s, r, "full-pool RGBA");
    for bad in [f32::NAN, -0.01, 1.01] {
        let mut r = resolved();
        r.bindings.get_mut(&(0, 0)).unwrap().colors =
            ResolvedColors::Rgba(vec![[1., 0.5, 0.2, bad]; 3]);
        error(&s, r, "full-pool RGBA");
    }
    let mut r = resolved();
    r.bindings.get_mut(&(0, 0)).unwrap().secondary_uv = SecondaryUvUse::Required;
    error(&s, r, "secondary UV sampling");
}
#[test]
fn known_uv_header_shapes_keep_explicitly_unused_secondary_bits() {
    for (kind, marker) in [(MeshKind::Model, 1), (MeshKind::Terrain, 2)] {
        let mut s = source();
        let index = if kind == MeshKind::Model { 0 } else { 1 };
        let m = &mut s.meshes[index];
        m.version = 2;
        m.uv_marker = Some(marker);
        for v in &mut m.vertices {
            v.uv1 = Some([f32::NAN, f32::INFINITY]);
        }
        let b = build(&s);
        assert_eq!(b.scene.meshes.len(), 2);
        assert!(s.meshes[index].vertices[0].uv1.unwrap()[0].is_nan());
    }
    let mut s = source();
    s.meshes[0].version = 3;
    for v in &mut s.meshes[0].vertices {
        v.color = Some(0x1234abcd);
        v.uv1 = Some([f32::NAN, 7.]);
    }
    let b = build(&s);
    assert!(b.scene.meshes[0].primitives[0].colors.is_none());
    assert_eq!(s.meshes[0].vertices[0].color, Some(0x1234abcd));
}
#[test]
fn unsupported_versions_uv_markers_skins_and_suffixes_fail() {
    let mut s = source();
    s.version = 3;
    error(&s, resolved(), "descriptor version");
    let mut s = source();
    s.meshes[0].version = 4;
    error(&s, resolved(), "mesh version");
    for (version, marker) in [
        (1, Some(0)),
        (2, Some(2)),
        (2, Some(17)),
        (2, None),
        (3, Some(0)),
    ] {
        let mut s = source();
        s.meshes[0].version = version;
        s.meshes[0].uv_marker = marker;
        error(&s, resolved(), "UV marker/header");
    }
    let mut s = source();
    s.meshes[0].vertices[0].color = Some(1);
    error(&s, resolved(), "attribute context");
    for bones in [None, Some(1)] {
        let mut s = source();
        s.meshes[0].bone_count = bones;
        error(&s, resolved(), "bone header");
    }
    let mut s = source();
    s.meshes[1].bone_count = Some(0);
    error(&s, resolved(), "bone header");
    let mut s = source();
    s.meshes[0].trailing_data = vec![0];
    error(&s, resolved(), "mesh suffix");
    let mut s = source();
    s.trailing_data = vec![0];
    error(&s, resolved(), "descriptor suffix");
}
#[test]
fn unresolved_visual_extensions_fail_but_metadata_is_reported() {
    let mut s = source();
    s.version = 2;
    s.placements.insert(
        0,
        ZonePlacement {
            model_index: None,
            name_offset: 23,
            position: [0.; 3],
            rotation: [0.; 3],
            scale: 0.,
            extension_data: vec![7; 4],
        },
    );
    s.placements[1].extension_data = vec![3; 4];
    error(&s, resolved(), "unresolved extension");
    s.placements[1].extension_data.clear();
    let b = build(&s);
    assert_eq!(b.report.excluded_placements[0].extension_bytes, 4);
    s.placements[0].position[0] = f32::NAN;
    error(&s, resolved(), "nonfinite placement");
    let mut s = source();
    s.placements[0].extension_data = vec![0; 4];
    error(&s, resolved(), "v1 cannot");
}
#[test]
fn malformed_string_slot_and_source_attribute_context_fail_even_unused() {
    let mut s = source();
    s.placements.clear();
    s.meshes[0].materials[0].shader_offset = 99;
    error(&s, resolved(), "string offset");
    let mut s = source();
    s.model_slots[0] = Some(99);
    error(&s, resolved(), "slot mesh out of range");
    let mut s = source();
    s.model_slots[3] = Some(0);
    error(&s, resolved(), "null mismatch");
    let mut s = source();
    s.placements[0].model_index = Some(99);
    error(&s, resolved(), "model slot out of range");
    let mut s = source();
    s.meshes[0].vertices[0].normal = [0., 0., 2.];
    error(&s, resolved(), "unit length");
    let mut s = source();
    s.meshes[0].vertices[0].uv0[0] = f32::NAN;
    error(&s, resolved(), "nonfinite source vertex");
    let mut s = source();
    s.meshes[0].triangles[0].vertex_indices = [0, 0, 2];
    error(&s, resolved(), "degenerate");
}
#[test]
fn positive_scale_and_transformed_bounds_only_for_rendered_instances() {
    for scale in [0., -1., f32::INFINITY] {
        let mut s = source();
        s.placements[0].scale = scale;
        assert!(build_static_scene(&s, resolved(), &BridgeLimits::default()).is_err());
    }
    let mut s = source();
    s.placements[0].scale = f32::MAX;
    error(&s, resolved(), "transformed positions");
}
#[test]
fn all_resource_material_refs_and_png_streams_are_validated() {
    let s = source();
    let mut r = resolved();
    r.materials.push(StaticMaterial {
        name: "unused".into(),
        base_color: [1.; 4],
        texture_index: Some(7),
        alpha_mode: StaticAlphaMode::Opaque,
        double_sided: true,
    });
    error(&s, r, "texture out of range");
    let mut r = resolved();
    r.materials[0].base_color[2] = f32::NAN;
    error(&s, r, "RGBA factor");
    let mut r = resolved();
    r.materials[0].alpha_mode = StaticAlphaMode::Mask { cutoff: 1.01 };
    error(&s, r, "alpha cutoff");
    let mut r = resolved();
    r.textures.push(StaticTexture {
        name: "unused".into(),
        png_bytes: b"bogus".to_vec(),
    });
    error(&s, r, "complete static PNG");
}
#[test]
fn aggregate_limits_cover_counts_bytes_and_transform_work_at_boundaries() {
    let s = source();
    let base = BridgeLimits::default();
    for limits in [
        BridgeLimits {
            max_vertices: 5,
            ..base.clone()
        },
        BridgeLimits {
            max_indices: 5,
            ..base.clone()
        },
        BridgeLimits {
            max_resources: 1,
            ..base.clone()
        },
        BridgeLimits {
            max_input_bytes: 1,
            ..base.clone()
        },
        BridgeLimits {
            max_output_bytes: 1,
            ..base.clone()
        },
        BridgeLimits {
            max_transform_visits: 5,
            ..base.clone()
        },
    ] {
        assert!(build_static_scene(&s, resolved(), &limits).is_err());
    }
    let exact = BridgeLimits {
        max_vertices: 6,
        max_indices: 6,
        max_transform_visits: 6,
        ..base.clone()
    };
    assert_eq!(
        build_static_scene(&s, resolved(), &exact)
            .unwrap()
            .report
            .transform_visits,
        6
    );
    // Find exact logical payload thresholds through the public entry point; one byte less fails.
    for input in [true, false] {
        let mut lower = 0;
        let mut upper = 4096;
        while lower < upper {
            let mid = (lower + upper) / 2;
            let mut limits = base.clone();
            if input {
                limits.max_input_bytes = mid;
            } else {
                limits.max_output_bytes = mid;
            }
            if build_static_scene(&s, resolved(), &limits).is_ok() {
                upper = mid;
            } else {
                lower = mid + 1;
            }
        }
        let mut limits = base.clone();
        if input {
            limits.max_input_bytes = lower;
        } else {
            limits.max_output_bytes = lower;
        }
        assert!(build_static_scene(&s, resolved(), &limits).is_ok());
        if input {
            limits.max_input_bytes -= 1;
        } else {
            limits.max_output_bytes -= 1;
        }
        assert!(build_static_scene(&s, resolved(), &limits).is_err());
    }
}
#[test]
fn common_writer_serializes_glb_axes_and_conjugated_instances_once() {
    let s = source();
    let b = build(&s);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("bridge.glb");
    crate::static_scene::write_static_visual(&b.scene, &"d".repeat(64), &output).unwrap();
    let (doc, buffers, _) = gltf::import(output).unwrap();
    let primitive = doc.meshes().next().unwrap().primitives().next().unwrap();
    let reader = primitive.reader(|b| Some(&buffers[b.index()]));
    assert_eq!(
        reader.read_positions().unwrap().next().unwrap(),
        [2., 3., -1.]
    );
    assert_eq!(
        reader.read_normals().unwrap().next().unwrap(),
        [-3. / 7., 6. / 7., 2. / 7.]
    );
    assert_eq!(
        reader
            .read_indices()
            .unwrap()
            .into_u32()
            .collect::<Vec<_>>(),
        [0, 2, 1]
    );
    let nodes = doc.nodes().collect::<Vec<_>>();
    let got = Mat4::from_cols_array_2d(&nodes[1].transform().matrix())
        .transform_point3(Vec3::new(2., 3., -1.))
        .to_array();
    let expected = expected_world([1., 2., 3.], &s.placements[0]);
    let gltf = [expected[0], expected[2], -expected[1]];
    for k in 0..3 {
        assert!((got[k] as f64 - gltf[k]).abs() < 1e-5);
    }
}
#[test]
fn resolved_resources_are_moved_and_unused_metadata_is_reported() {
    let mut s = source();
    s.regions.push(libeq_eqg::zone::Region {
        name_offset: 23,
        data: [0; 9],
    });
    s.lights.push(libeq_eqg::zone::Light {
        name_offset: 23,
        data: [0; 7],
    });
    let mut r = resolved();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([17, 33, 49, 128]),
    ))
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    r.textures.push(StaticTexture {
        name: "unused PNG".into(),
        png_bytes: png.into_inner(),
    });
    let png_ptr = r.textures[0].png_bytes.as_ptr();
    let colors = vec![[0.2, 0.4, 0.6, 0.8]; 3];
    let colors_ptr = colors.as_ptr();
    r.bindings.get_mut(&(0, 0)).unwrap().colors = ResolvedColors::Rgba(colors);
    r.materials.push(StaticMaterial {
        name: "unused material".into(),
        base_color: [1.; 4],
        texture_index: Some(0),
        alpha_mode: StaticAlphaMode::Opaque,
        double_sided: false,
    });
    let b = build_static_scene(&s, r, &BridgeLimits::default()).unwrap();
    assert_eq!(b.scene.textures[0].png_bytes.as_ptr(), png_ptr);
    assert_eq!(
        b.scene.meshes[0].primitives[0]
            .colors
            .as_ref()
            .unwrap()
            .as_ptr(),
        colors_ptr
    );
    assert_eq!(b.report.unused_materials, [1]);
    assert_eq!(b.report.unused_textures, [0]);
    assert_eq!((b.report.region_count, b.report.light_count), (1, 1));
    assert_eq!(s.regions[0].data, [0; 9]);
}
#[test]
fn source_property_string_references_and_extension_word_shapes_are_checked() {
    let mut s = source();
    s.meshes[0].materials[0]
        .properties
        .push(libeq_eqg::mesh::Property {
            name_offset: 0,
            kind: 2,
            value: 99,
        });
    error(&s, resolved(), "source string offset");
    s.meshes[0].materials[0].properties[0].kind = 1;
    assert!(build_static_scene(&s, resolved(), &BridgeLimits::default()).is_ok());
    let mut s = source();
    s.version = 2;
    s.placements[0].extension_data = vec![0; 3];
    error(&s, resolved(), "extension word is incomplete");
}
#[test]
fn excluded_roles_and_opaque_properties_do_not_change_visual_flags() {
    for flags in [0, 1, 2, 3, 0xffff_ffff] {
        let mut s = source();
        s.meshes[0].triangles[0].flags = flags;
        assert_eq!(build(&s).scene.meshes[0].primitives[0].indices, [0, 2, 1]);
    }
    let mut s = source();
    s.placements.clear();
    s.meshes[0].triangles[0].material_index = u32::MAX;
    let mut r = resolved();
    r.bindings.remove(&(0, 0));
    let b = build_static_scene(&s, r, &BridgeLimits::default()).unwrap();
    assert_eq!(b.report.sentinel_triangles, [1, 0]);
    assert_eq!(b.report.source_to_visual_mesh, [None, Some(0)]);
}
#[test]
fn unused_binding_record_consumes_logical_input_bytes() {
    let mut s = source();
    s.meshes[0].materials.push(Material {
        index: 777,
        name_offset: 0,
        shader_offset: 2,
        properties: vec![],
    });
    fn threshold(s: &BinaryZoneScene, extra: bool) -> usize {
        let mut lo = 0;
        let mut hi = 4096;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let mut r = resolved();
            if extra {
                r.bindings.insert((0, 1), binding());
            }
            let limits = BridgeLimits {
                max_input_bytes: mid,
                ..BridgeLimits::default()
            };
            if build_static_scene(s, r, &limits).is_ok() {
                hi = mid
            } else {
                lo = mid + 1
            }
        }
        lo
    }
    let without = threshold(&s, false);
    let with = threshold(&s, true);
    assert_eq!(with - without, 32, "one fixed record is charged once");
    assert!(
        with > without,
        "caller-owned binding key/material/UV/color decisions occupy nonzero logical input bytes"
    );
}

#[test]
fn many_unused_bindings_obey_custom_caps_and_rgba_payload_is_charged_once() {
    let mut s = source();
    for index in 1..=64 {
        s.meshes[0].materials.push(Material {
            index: 500 + index,
            name_offset: 0,
            shader_offset: 2,
            properties: vec![],
        });
    }
    fn resolved_with(count: usize, rgba: bool) -> ResolvedZoneVisual {
        let mut r = resolved();
        for ordinal in 1..=count {
            let mut b = binding();
            if rgba && ordinal == count {
                b.colors = ResolvedColors::Rgba(vec![[0.2, 0.4, 0.6, 0.8]; 3]);
            }
            r.bindings.insert((0, ordinal), b);
        }
        r
    }
    fn threshold(s: &BinaryZoneScene, count: usize, rgba: bool) -> usize {
        let mut lower = 0;
        let mut upper = 16_384;
        while lower < upper {
            let mid = (lower + upper) / 2;
            let limits = BridgeLimits {
                max_input_bytes: mid,
                ..BridgeLimits::default()
            };
            if build_static_scene(s, resolved_with(count, rgba), &limits).is_ok() {
                upper = mid;
            } else {
                lower = mid + 1;
            }
        }
        lower
    }
    let base = threshold(&s, 0, false);
    let unused = threshold(&s, 64, false);
    assert_eq!(unused - base, 64 * 32);
    let colored = threshold(&s, 64, true);
    assert_eq!(
        colored - unused,
        3 * 4 * 4,
        "RGBA samples supplement the fixed record exactly once"
    );
    for (rgba, exact) in [(false, unused), (true, colored)] {
        let mut limits = BridgeLimits {
            max_input_bytes: exact,
            ..BridgeLimits::default()
        };
        let accepted = build_static_scene(&s, resolved_with(64, rgba), &limits).unwrap();
        assert_eq!(accepted.report.unused_bindings.len(), 64);
        limits.max_input_bytes -= 1;
        let err = build_static_scene(&s, resolved_with(64, rgba), &limits).unwrap_err();
        assert!(format!("{err:#}").contains("input payload budget exceeded"));
    }
}
