//! Explicit binary EQG static PBR preview with retained, unapplied source inputs.
//!
//! Bounds cover logical payloads. Source-record limits apply after assembly;
//! texture work is charged after one bounded decoder scratch image at a time.
//! Inputs must be trusted, stable local regular files. The existing PFS inflater
//! remains a dependency ceiling; these guards do not establish peak process RAM.
mod archive;
mod inventory;
mod materials;
mod report;
use super::{
    lighting::DiagnosticLimits,
    load_binary_zone,
    static_scene::{
        self, BridgeLimits, MaterialBinding, ResolvedColors, ResolvedZoneVisual, SecondaryUvUse,
    },
    BinaryZoneScene, DescriptorSource,
};
use crate::{
    source_material::{decode_static_texture, TextureDecodePolicy},
    static_scene::{StaticMaterialPolicy, StaticScene},
};
use anyhow::{bail, ensure, Context, Result};
pub use inventory::{
    inspect_eqg_pbr_materials, ArchiveMemberLookup, EqgPbrMaterialInventory, MaterialInventoryRow,
    MaterialPropertyInventory, MaterialSchemaAssessment,
};
use libeq_eqg::mesh::MeshKind;
pub use report::{bounded_json, DescriptorIdentity, EqgPbrReport, NormalSourceAbsence};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// All maxima may be reduced; larger limits are refused.
#[derive(Debug, Clone)]
pub struct EqgPbrLimits {
    pub max_archive_bytes: usize,
    pub max_descriptor_bytes: usize,
    pub max_member_bytes: usize,
    pub max_archive_uncompressed_bytes: usize,
    pub max_directory_bytes: usize,
    pub max_archive_members: usize,
    pub max_name_bytes: usize,
    pub max_total_name_bytes: usize,
    pub max_preserved_resource_bytes: usize,
    pub max_source_records: usize,
    pub max_report_records: usize,
    pub max_report_json_bytes: usize,
    pub max_normalized_png_bytes: usize,
    pub max_decoded_pixel_bytes: usize,
    pub max_glb_bytes: usize,
    pub bridge: BridgeLimits,
    pub lighting: DiagnosticLimits,
}
impl Default for EqgPbrLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 134217728,
            max_descriptor_bytes: 16777216,
            max_member_bytes: 67108864,
            max_archive_uncompressed_bytes: 268435456,
            max_directory_bytes: 4194304,
            max_archive_members: 100000,
            max_name_bytes: 256,
            max_total_name_bytes: 4194304,
            max_preserved_resource_bytes: 134217728,
            max_source_records: 1000000,
            max_report_records: 1000000,
            max_report_json_bytes: 16777216,
            max_normalized_png_bytes: 100663296,
            max_decoded_pixel_bytes: 268435456,
            max_glb_bytes: 134217728,
            bridge: BridgeLimits::default(),
            lighting: DiagnosticLimits::default(),
        }
    }
}
impl EqgPbrLimits {
    fn validate(&self) -> Result<()> {
        let d = Self::default();
        macro_rules! check {($a:expr,$b:expr,$($f:ident),+)=>{$(ensure!($a.$f<=$b.$f,"limit {} exceeds implementation maximum",stringify!($f));)+}}
        check!(
            self,
            d,
            max_archive_bytes,
            max_descriptor_bytes,
            max_member_bytes,
            max_archive_uncompressed_bytes,
            max_directory_bytes,
            max_archive_members,
            max_name_bytes,
            max_total_name_bytes,
            max_preserved_resource_bytes,
            max_source_records,
            max_report_records,
            max_report_json_bytes,
            max_normalized_png_bytes,
            max_decoded_pixel_bytes,
            max_glb_bytes
        );
        check!(
            self.bridge,
            d.bridge,
            max_input_bytes,
            max_output_bytes,
            max_vertices,
            max_indices,
            max_resources,
            max_transform_visits
        );
        check!(
            self.lighting,
            d.lighting,
            max_targets,
            max_resources,
            max_logical_records,
            max_name_bytes,
            max_input_bytes,
            max_words
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TextureRole {
    Diffuse,
    Normal,
    Coverage,
    Environment,
}
#[derive(Debug, Clone, Serialize)]
pub struct ResourceUse {
    pub mesh_index: usize,
    pub material_ordinal: usize,
    pub property_ordinal: usize,
    pub requested_name: String,
    pub role: TextureRole,
}
#[derive(Debug)]
pub struct PreservedResource {
    pub exact_name: String,
    pub blake3: String,
    pub bytes: Vec<u8>,
    pub uses: Vec<ResourceUse>,
}
#[derive(Debug)]
pub struct ResolvedEqgPbrPreview {
    pub scene: StaticScene,
    pub recommended_material_policy: StaticMaterialPolicy,
    pub report: EqgPbrReport,
    pub source: BinaryZoneScene,
    pub preserved_resources: Vec<PreservedResource>,
}
const NORMAL_ABSENCE_DECISION: &str =
    "source absence: optional e_TextureNormal0 is exact \"None\"; no image requested";
fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
fn charge(total: &mut usize, n: usize, cap: usize, what: &str) -> Result<()> {
    *total = total
        .checked_add(n)
        .with_context(|| format!("{what} arithmetic overflow"))?;
    ensure!(*total <= cap, "{what} limit exceeded");
    Ok(())
}
fn text(table: &[u8], offset: u32, l: &EqgPbrLimits) -> Result<String> {
    let b = table
        .get(offset as usize..)
        .context("invalid string offset")?;
    let end = b
        .iter()
        .take(l.max_name_bytes + 1)
        .position(|&b| b == 0)
        .context("unterminated or oversized string")?;
    ensure!(end <= l.max_name_bytes, "string byte limit exceeded");
    Ok(std::str::from_utf8(&b[..end])?.to_owned())
}
fn source_budget(s: &BinaryZoneScene, l: &EqgPbrLimits) -> Result<report::BudgetReport> {
    let mut b = report::BudgetReport::default();
    for n in [
        s.meshes.len(),
        s.model_slots.len(),
        s.model_name_offsets.len(),
        s.placements.len(),
        s.regions.len(),
        s.lights.len(),
    ] {
        charge(
            &mut b.source_records,
            n,
            l.max_source_records,
            "source record",
        )?;
        charge(
            &mut b.report_records,
            n,
            l.max_report_records,
            "report record",
        )?;
    }
    charge(
        &mut b.report_string_bytes,
        s.string_table.len(),
        l.max_total_name_bytes,
        "report/source string",
    )?;
    // Conservative projection work units reserve generated rows/decision entries
    // and bounded strings before the owned report/resource-use projections.
    for (count, factor) in [(s.meshes.len(), 3usize), (s.placements.len(), 4), (1, 1)] {
        charge(
            &mut b.report_records,
            count
                .checked_mul(factor)
                .context("report record arithmetic overflow")?,
            l.max_report_records,
            "report projection record",
        )?;
    }
    for (count, factor) in [
        (s.meshes.len(), 512usize),
        (s.placements.len(), 512),
        (1, 4096),
    ] {
        charge(
            &mut b.report_string_bytes,
            count
                .checked_mul(factor)
                .context("report string arithmetic overflow")?,
            l.max_total_name_bytes,
            "report projection string",
        )?;
    }
    for m in &s.meshes {
        for n in [m.vertices.len(), m.triangles.len(), m.materials.len()] {
            charge(
                &mut b.source_records,
                n,
                l.max_source_records,
                "source record",
            )?;
        }
        charge(
            &mut b.report_records,
            m.materials.len(),
            l.max_report_records,
            "report record",
        )?;
        charge(
            &mut b.report_string_bytes,
            m.string_table.len(),
            l.max_total_name_bytes,
            "report/source string",
        )?;
        for mat in &m.materials {
            charge(
                &mut b.report_records,
                8,
                l.max_report_records,
                "material projection record",
            )?;
            charge(
                &mut b.report_string_bytes,
                1024,
                l.max_total_name_bytes,
                "material projection string",
            )?;
            charge(
                &mut b.source_records,
                mat.properties.len(),
                l.max_source_records,
                "source record",
            )?;
            charge(
                &mut b.report_records,
                mat.properties.len(),
                l.max_report_records,
                "report record",
            )?;
            for o in std::iter::once(mat.name_offset)
                .chain(mat.shader_offset)
                .chain(mat.properties.iter().map(|p| p.name_offset))
                .chain(
                    mat.properties
                        .iter()
                        .filter(|p| p.kind == 2)
                        .map(|p| p.value),
                )
            {
                let t = text(&m.string_table, o, l)?;
                charge(
                    &mut b.report_string_bytes,
                    t.len()
                        .checked_mul(3)
                        .context("string arithmetic overflow")?,
                    l.max_total_name_bytes,
                    "report string",
                )?;
            }
        }
    }
    for p in &s.placements {
        charge(
            &mut b.source_records,
            p.extension_data.len() / 4,
            l.max_source_records,
            "source record",
        )?;
    }
    Ok(b)
}
fn used_materials(s: &BinaryZoneScene) -> Result<BTreeSet<(usize, usize)>> {
    let mut terrains = s
        .meshes
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind == MeshKind::Terrain);
    let terrain = terrains.next().context("missing terrain")?.0;
    ensure!(
        terrains.next().is_none(),
        "requires exactly one terrain mesh"
    );
    let mut used = BTreeSet::from([terrain]);
    for p in s.placements.iter().skip(usize::from(s.version == 2)) {
        if let Some(slot) = p.model_index {
            if let Some(mi) = s.model_slots.get(slot as usize).copied().flatten() {
                used.insert(mi);
            }
        }
    }
    let mut keys = BTreeSet::new();
    for mi in used {
        let m = &s.meshes[mi];
        for t in &m.triangles {
            match super::surface::assess(t.material_index, m.materials.len(), t.flags)
                .render_reference
            {
                super::surface::RenderReference::Material => {
                    keys.insert((mi, t.material_index as usize));
                }
                super::surface::RenderReference::Sentinel => {}
                _ => bail!("invalid required material reference"),
            }
        }
    }
    Ok(keys)
}
fn read_descriptor(
    archive: &mut archive::Archive,
    descriptor: DescriptorSource<'_>,
    l: &EqgPbrLimits,
) -> Result<(
    Vec<u8>,
    DescriptorIdentity,
    Option<archive::Snapshot>,
    Option<String>,
)> {
    match descriptor {
        DescriptorSource::Loose(p) => {
            let mut snapshot = archive::Snapshot::open(p, l.max_descriptor_bytes)?;
            let bytes = snapshot.bytes()?;
            let identity = DescriptorIdentity::Loose {
                content_hash: snapshot.hash.clone(),
            };
            Ok((bytes, identity, Some(snapshot), None))
        }
        DescriptorSource::Archive(n) => {
            let (exact, _) = archive.exact(n, l)?;
            let bytes = archive.read(&exact, l.max_descriptor_bytes)?;
            let identity = DescriptorIdentity::ArchiveMember {
                exact_name: exact.clone(),
                content_hash: hash(&bytes),
            };
            Ok((bytes, identity, None, Some(exact)))
        }
    }
}
/// Load only an explicitly selected descriptor/archive under the static preview policy.
/// The original parsed records and selected raw texture bytes remain owned and unchanged.
pub fn load_eqg_pbr_preview(
    path: &Path,
    descriptor: DescriptorSource<'_>,
    l: &EqgPbrLimits,
) -> Result<ResolvedEqgPbrPreview> {
    l.validate()?;
    let mut archive = archive::Archive::open(path, l)?;
    let (descriptor_bytes, identity, mut loose, selected) =
        read_descriptor(&mut archive, descriptor, l)?;
    let parsed = libeq_eqg::zone::parse(&descriptor_bytes)?;
    ensure!(
        parsed.version == 2,
        "static PBR preview requires descriptor v2"
    );
    for o in parsed.models.iter().flatten() {
        archive::safe_name(std::str::from_utf8(parsed.string(*o)?)?, l)?;
    }
    let source = load_binary_zone(
        path,
        match descriptor {
            DescriptorSource::Loose(p) => DescriptorSource::Loose(p),
            DescriptorSource::Archive(_) => DescriptorSource::Archive(selected.as_deref().unwrap()),
        },
    )?;
    let mut budget = source_budget(&source, l)?;
    budget.archive_bytes = std::fs::metadata(path)?.len();
    budget.archive_advertised_bytes = archive.advertised_bytes;
    budget.descriptor_bytes = descriptor_bytes.len();
    let used = used_materials(&source)?;
    let mut material_rows = Vec::new();
    let mut resources: Vec<PreservedResource> = Vec::new();
    let mut resource_index = BTreeMap::new();
    // Validate all material records structurally, including uninterpreted unused records.
    for (mi, m) in source.meshes.iter().enumerate() {
        for (ordinal, mat) in m.materials.iter().enumerate() {
            let name = text(&m.string_table, mat.name_offset, l)?;
            let shader = text(
                &m.string_table,
                mat.shader_offset.context("unresolved shader")?,
                l,
            )?;
            let is_used = used.contains(&(mi, ordinal));
            let mut properties = Vec::new();
            for p in &mat.properties {
                properties.push(report::PropertyReport {
                    raw_name_offset: p.name_offset,
                    name: text(&m.string_table, p.name_offset, l)?,
                    kind: p.kind,
                    value_word: p.value,
                    resource_index: None,
                });
            }
            let mut normal_source_absence = None;
            if is_used {
                materials::validate(&shader, &properties)?;
                for (pi, p) in properties
                    .iter_mut()
                    .enumerate()
                    .filter(|(_, p)| p.kind == 2)
                {
                    let requested = text(&m.string_table, p.value_word, l)?;
                    let role = materials::role(&p.name)?;
                    if role == TextureRole::Normal && requested == "None" {
                        charge(
                            &mut budget.report_records,
                            2,
                            l.max_report_records,
                            "normal absence projection record",
                        )?;
                        charge(
                            &mut budget.report_string_bytes,
                            NORMAL_ABSENCE_DECISION.len() + "ExactNone".len(),
                            l.max_total_name_bytes,
                            "normal absence projection string",
                        )?;
                        normal_source_absence = Some(report::NormalSourceAbsence::ExactNone);
                        continue;
                    }
                    let (exact, len) = archive.exact(&requested, l)?;
                    let ri = if let Some(&i) = resource_index.get(&exact) {
                        i
                    } else {
                        charge(
                            &mut budget.preserved_resource_bytes,
                            len,
                            l.max_preserved_resource_bytes,
                            "preserved resource byte",
                        )?;
                        charge(
                            &mut budget.report_records,
                            1,
                            l.max_report_records,
                            "resource report record",
                        )?;
                        charge(
                            &mut budget.report_string_bytes,
                            exact
                                .len()
                                .checked_mul(4)
                                .and_then(|n| n.checked_add(256))
                                .context("resource string arithmetic overflow")?,
                            l.max_total_name_bytes,
                            "resource projection string",
                        )?;
                        let bytes = archive.read(&exact, l.max_member_bytes)?;
                        let i = resources.len();
                        resource_index.insert(exact.clone(), i);
                        resources.push(PreservedResource {
                            exact_name: exact,
                            blake3: hash(&bytes),
                            bytes,
                            uses: Vec::new(),
                        });
                        i
                    };
                    charge(
                        &mut budget.report_records,
                        2,
                        l.max_report_records,
                        "resource use report record",
                    )?;
                    resources[ri].uses.push(ResourceUse {
                        mesh_index: mi,
                        material_ordinal: ordinal,
                        property_ordinal: pi,
                        requested_name: requested,
                        role,
                    });
                    p.resource_index = Some(ri);
                }
            }
            material_rows.push(report::MaterialReport {
                source_mesh_index: mi,
                source_material_ordinal: ordinal,
                source_declared_id: mat.index,
                raw_name_offset: mat.name_offset,
                raw_shader_offset: mat.shader_offset,
                name,
                shader,
                used: is_used,
                output_material_index: None,
                output_texture_index: None,
                properties,
                coverage: "uninterpreted-unused".into(),
                cutoff: None,
                decisions: Vec::new(),
                normal_source_absence,
                base_color_factor: None,
                surface: None,
                double_sided: None,
            });
        }
    }
    let mut resolved = ResolvedZoneVisual::default();
    let mut resource_rows = Vec::new();
    let mut output_textures = BTreeMap::new();
    for (ri, r) in resources.iter().enumerate() {
        let texture =
            decode_static_texture(&r.exact_name, &r.bytes, TextureDecodePolicy::PreserveRgba)
                .with_context(|| format!("validate required image {:?}", r.exact_name))?;
        ensure!(
            texture.png_bytes.len() >= 24,
            "normalized PNG header missing"
        );
        let width = u32::from_be_bytes(texture.png_bytes[16..20].try_into()?);
        let height = u32::from_be_bytes(texture.png_bytes[20..24].try_into()?);
        let work = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(8))
            .context("decoded pixel work overflow")?;
        charge(
            &mut budget.decoded_pixel_bytes,
            work,
            l.max_decoded_pixel_bytes,
            "decoded pixel work",
        )?;
        charge(
            &mut budget.normalized_png_bytes,
            texture.png_bytes.len(),
            l.max_normalized_png_bytes,
            "normalized PNG byte",
        )?;
        let png_bytes = texture.png_bytes.len();
        let out = if r.uses.iter().any(|u| u.role == TextureRole::Diffuse) {
            let i = resolved.textures.len();
            resolved.textures.push(texture);
            output_textures.insert(ri, i);
            Some(i)
        } else {
            None
        };
        resource_rows.push(report::ResourceReport {
            exact_name: r.exact_name.clone(),
            blake3: r.blake3.clone(),
            raw_bytes: r.bytes.len(),
            uses: r.uses.clone(),
            output_texture_index: out,
            width,
            height,
            normalized_png_bytes: png_bytes,
        });
    }
    let mut policy = StaticMaterialPolicy::default();
    for row in material_rows.iter_mut().filter(|m| m.used) {
        let diffuse = row
            .properties
            .iter()
            .find(|p| p.name == "e_TextureDiffuse0")
            .and_then(|p| p.resource_index)
            .context("missing resolved diffuse")?;
        let ti = output_textures[&diffuse];
        let i = resolved.materials.len();
        let mat = materials::material(row.name.clone(), &row.shader, ti);
        let surface = materials::surface(&row.shader);
        if surface != policy.default {
            policy.overrides.insert(i, surface);
        }
        row.coverage = match mat.alpha_mode {
            crate::static_scene::StaticAlphaMode::Opaque => "OPAQUE",
            crate::static_scene::StaticAlphaMode::Mask { cutoff } => {
                row.cutoff = Some(cutoff);
                "MASK"
            }
            crate::static_scene::StaticAlphaMode::Blend => "BLEND",
        }
        .into();
        row.output_material_index = Some(i);
        row.output_texture_index = Some(ti);
        row.base_color_factor = Some(mat.base_color);
        row.double_sided = Some(mat.double_sided);
        row.surface = Some(surface);
        row.decisions=vec!["direct: preserved diffuse RGBA and UV0".into(),"default: editable scalar nonmetal PBR".into(),"omitted: packed vertex RGB/alpha, source prelighting and UV1".into(),"omitted: source scalar/color arithmetic and normal/coverage/environment sampling; raw inputs retained".into()];
        if row.normal_source_absence.is_some() {
            row.decisions.push(NORMAL_ABSENCE_DECISION.into());
        }
        if row.shader == "Opaque_MaxWater.fx" {
            row.decisions.push("approximated: static water alpha 0.55, roughness 0.2, double sided; motion/Fresnel/refraction/environment omitted".into());
        } else {
            row.decisions
                .push("default: factor RGBA 1, roughness 1, single sided".into());
        }
        resolved.materials.push(mat);
        resolved.bindings.insert(
            (row.source_mesh_index, row.source_material_ordinal),
            MaterialBinding {
                material_index: i,
                colors: ResolvedColors::Unused,
                secondary_uv: SecondaryUvUse::Unused,
            },
        );
    }
    let bridged = static_scene::build_static_scene_with_unapplied_v2_lighting(
        &source,
        resolved,
        &l.bridge,
        &l.lighting,
    )?;
    let lighting = report::lighting(&bridged.lighting);
    let geometry = report::geometry(&source, &bridged.geometry, used.len())?;
    drop(bridged.lighting);
    let scene = bridged.geometry.scene;
    let mut models = Vec::new();
    for (i, m) in source.meshes.iter().enumerate() {
        let bytes = archive.read(&m.source_name, l.max_member_bytes)?;
        models.push(report::SourceModelIdentity {
            mesh_index: i,
            exact_name: m.source_name.clone(),
            blake3: hash(&bytes),
        });
    }
    archive.snapshot.verify()?;
    if let Some(s) = &mut loose {
        s.verify()?;
    }
    let mut report=EqgPbrReport{schema:"eqg-static-pbr-preview-v1",profile:"scalar-pbr-hdr-blend-v1",scope:"static-source-geometry-preview",conversion_revision:String::new(),implementation_revision:implementation_revision(),archive_blake3:archive.snapshot.hash.clone(),descriptor:identity,models,materials:material_rows,resources:resource_rows,lighting,geometry,budget,limitations:vec!["Static source geometry only; not gameplay, dynamic actor or full native-effect readiness.","Water is an editable static approximation; waves, refraction, Fresnel and environment arithmetic are omitted.","Raw packed colors, secondary UVs, source parameters and prelighting are preserved and unapplied; count equality does not certify a native lighting consumer.","Trusted stable local inputs with before/after content checks; no authentication or hostile concurrent ABA guarantee.","All advertised archive bytes preflighted; logical records bounded after assembly. Existing PFS inflation and single-image decoder scratch do not establish peak RAM bounds."]};
    let bytes = bounded_json(&report, l.max_report_json_bytes)?;
    report.conversion_revision = hash(&bytes);
    bounded_json(&report, l.max_report_json_bytes)?;
    Ok(ResolvedEqgPbrPreview {
        scene,
        recommended_material_policy: policy,
        report,
        source,
        preserved_resources: resources,
    })
}
fn implementation_revision() -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"eqg-static-pbr-preview-implementation-v1");
    for b in [
        include_bytes!("pbr_visual.rs").as_slice(),
        include_bytes!("pbr_visual/archive.rs"),
        include_bytes!("pbr_visual/materials.rs"),
        include_bytes!("pbr_visual/report.rs"),
        include_bytes!("zon.rs"),
        include_bytes!("static_scene.rs"),
        include_bytes!("lighting.rs"),
        include_bytes!("../convert/mod.rs"),
        include_bytes!("../compatibility.rs"),
        include_bytes!("surface.rs"),
        include_bytes!("../source_material.rs"),
        include_bytes!("../convert/texture.rs"),
        include_bytes!("../convert/dds.rs"),
        include_bytes!("../../Cargo.lock"),
    ] {
        h.update(&(b.len() as u64).to_le_bytes());
        h.update(b);
    }
    h.finalize().to_hex().to_string()
}
#[cfg(test)]
mod tests;
