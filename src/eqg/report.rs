//! Deterministic source-assembly diagnostics. These are not bake-readiness results.
use super::BinaryZoneScene;
use serde_json::{Value, json};
use std::collections::BTreeMap;

// Raw bytes are authoritative; UTF-8 is only a convenience for readable names.
fn string_details(table: &[u8], offset: u32) -> Value {
    let bytes = table.get(offset as usize..).and_then(|tail| {
        tail.iter()
            .position(|&byte| byte == 0)
            .map(|end| &tail[..end])
    });
    json!({
        "bytes": bytes,
        "utf8": bytes.and_then(|bytes| std::str::from_utf8(bytes).ok()),
    })
}

pub fn summary(scene: &BinaryZoneScene) -> Value {
    let meshes: Vec<_> = scene.meshes.iter().map(|mesh| {
        let material_details: Vec<_> = mesh.materials.iter().enumerate().map(|(ordinal, material)| {
            let properties: Vec<_> = material.properties.iter().map(|property| {
                json!({
                    "name": string_details(&mesh.string_table, property.name_offset),
                    "kind": property.kind,
                    "value_bits": property.value,
                    "string_value": (property.kind == 2)
                        .then(|| string_details(&mesh.string_table, property.value)),
                })
            }).collect();
            json!({
                "ordinal": ordinal,
                "index": material.index,
                "name": string_details(&mesh.string_table, material.name_offset),
                "shader": string_details(&mesh.string_table, material.shader_offset),
                "properties": properties,
            })
        }).collect();
        let mut groups = BTreeMap::new();
        for triangle in &mesh.triangles {
            *groups.entry((triangle.material_index, triangle.flags)).or_insert(0usize) += 1;
        }
        let triangle_surface_groups: Vec<_> = groups.into_iter().map(|((material_index, flags), triangles)| {
            json!({"material_index": material_index, "flags": flags, "triangles": triangles})
        }).collect();
        json!({
            "member": mesh.source_name,
            "material_details": material_details,
            "triangle_surface_groups": triangle_surface_groups,
            "surface_assessments": mesh.triangles.iter().map(|triangle| {
                (triangle.material_index, triangle.flags)
            }).collect::<std::collections::BTreeSet<_>>().into_iter().map(|(material_index, flags)| {
                json!({"material_index": material_index, "flags": flags,
                    "assessment": super::surface::assess(material_index, mesh.materials.len(), flags)})
            }).collect::<Vec<_>>(),
            "format": match mesh.kind {
                libeq_eqg::mesh::MeshKind::Terrain => "eqgt",
                libeq_eqg::mesh::MeshKind::Model => "eqgm",
            },
            "version": mesh.version,
            "materials": mesh.materials.len(),
            "material_properties": mesh.materials.iter().map(|m| m.properties.len()).sum::<usize>(),
            "vertices": mesh.vertices.len(),
            "triangles": mesh.triangles.len(),
            "triangles_without_table_material": mesh.triangles.iter()
                .filter(|t| t.material_index as u64 >= mesh.materials.len() as u64).count(),
            "vertices_with_color": mesh.vertices.iter().filter(|v| v.color.is_some()).count(),
            "vertices_with_nonfinite_secondary_uv": mesh.vertices.iter()
                .filter(|v| v.uv1.is_some_and(|uv| uv.iter().any(|x| !x.is_finite()))).count(),
            "vertices_with_secondary_uv": mesh.vertices.iter().filter(|v| v.uv1.is_some()).count(),
            "bone_count": mesh.bone_count,
            "trailing_bytes": mesh.trailing_data.len(),
        })
    }).collect();
    let dependencies: Vec<_> = scene.model_name_offsets.iter().enumerate().map(|(slot, offset)| {
        let requested = offset.and_then(|offset| scene.string_table.get(offset as usize..))
            .and_then(|bytes| bytes.iter().position(|&b| b == 0).map(|end| &bytes[..end]))
            .and_then(|bytes| std::str::from_utf8(bytes).ok());
        let mesh_index = scene.model_slots.get(slot).copied().flatten();
        let member = mesh_index.and_then(|index| scene.meshes.get(index)).map(|mesh| &mesh.source_name);
        json!({"slot": slot, "requested": requested, "mesh_index": mesh_index, "member": member})
    }).collect();
    json!({
        "archive": scene.archive_path.to_string_lossy(),
        "descriptor": {"name": scene.descriptor_name, "location": scene.descriptor_location},
        "model_dependencies": dependencies,
        "stage": "binary_source_assembly",
        "version": scene.version,
        "model_slots": scene.model_slots,
        "unique_meshes": scene.meshes.len(),
        "placement_records": scene.placements.len(),
        "placement_extension_words": scene.placements.iter()
            .map(|p| p.extension_data.len() / 4).sum::<usize>(),
        "regions": scene.regions.len(),
        "lights": scene.lights.len(),
        "descriptor_trailing_bytes": scene.trailing_data.len(),
        "meshes": meshes,
        "limitations": [
            "Transforms and placement roles remain in source form; no world-space scene is emitted.",
            "Material properties are retained; texture dependencies and rendering effects are not resolved.",
            "Collision, region, light, skeletal, and extension semantics are not applied.",
            "This report does not establish bake readiness or publish assets."
        ]
    })
}
