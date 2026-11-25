use super::*;

/// Only the existing preview's exact shader/property policy is assessed.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum MaterialSchemaAssessment {
    Compatible,
    Unsupported { diagnostic: String },
    UninterpretedUnused,
}
/// Directory lookup facts only. Present bytes have not been read or decoded.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ArchiveMemberLookup {
    Present {
        exact_name: String,
        advertised_bytes: usize,
    },
    Missing,
    UnsafeName,
}
#[derive(Debug, Serialize)]
pub struct MaterialPropertyInventory {
    pub raw_name_offset: u32,
    pub name: String,
    pub kind: u32,
    pub value_word: u32,
    /// Exact source string for kind 2; all other kinds retain only their raw word.
    pub string_value: Option<String>,
    /// Only selected material kind-2 references are looked up, without assigning roles.
    pub archive_lookup: Option<ArchiveMemberLookup>,
}
#[derive(Debug, Serialize)]
pub struct MaterialInventoryRow {
    pub source_mesh_index: usize,
    pub source_material_ordinal: usize,
    pub source_declared_id: u32,
    pub raw_name_offset: u32,
    pub raw_shader_offset: Option<u32>,
    pub name: String,
    pub shader: Option<String>,
    pub required: bool,
    pub schema: MaterialSchemaAssessment,
    pub properties: Vec<MaterialPropertyInventory>,
}
/// Schema and directory metadata, never a conversion or rendering admission result.
#[derive(Debug, Serialize)]
pub struct EqgPbrMaterialInventory {
    pub schema: &'static str,
    pub scope: &'static str,
    pub readiness: &'static str,
    pub inventory_revision: String,
    pub implementation_revision: String,
    pub archive_blake3: String,
    pub descriptor: DescriptorIdentity,
    pub descriptor_version: u32,
    pub models: Vec<report::SourceModelIdentity>,
    pub model_slots: Vec<Option<usize>>,
    pub required_materials: usize,
    pub unsupported_required_materials: usize,
    pub materials: Vec<MaterialInventoryRow>,
    pub budget: report::BudgetReport,
    pub limitations: Vec<&'static str>,
}

/// Inventory explicit binary descriptor v1/v2 materials without decoding resources
/// or constructing geometry/lighting. Schema failures accumulate per required row;
/// Parse/assembly errors, invalid required material references, ambiguous archive
/// names and exceeded limits refuse. Unselected triangle material references and
/// geometry completeness are not checked. Exactly one distinct terrain is required.
/// Source-record bounds apply after the existing source assembler's allocations.
/// Inputs must be trusted, stable regular files; PFS inflation is a dependency ceiling.
pub fn inspect_eqg_pbr_materials(
    path: &Path,
    descriptor: DescriptorSource<'_>,
    l: &EqgPbrLimits,
) -> Result<EqgPbrMaterialInventory> {
    l.validate()?;
    let mut archive = archive::Archive::open(path, l)?;
    let (bytes, identity, mut loose, selected) = read_descriptor(&mut archive, descriptor, l)?;
    let (source, models) = assemble_source(
        path,
        descriptor,
        &bytes,
        selected.as_deref(),
        &mut archive,
        l,
    )?;
    let mut budget = source_budget(&source, l)?;
    budget.archive_bytes = std::fs::metadata(path)?.len();
    budget.archive_advertised_bytes = archive.advertised_bytes;
    budget.descriptor_bytes = bytes.len();
    let required = used_materials(&source)?;
    let mut rows = Vec::new();
    let mut unsupported = 0;
    for (mi, mesh) in source.meshes.iter().enumerate() {
        for (ordinal, mat) in mesh.materials.iter().enumerate() {
            let is_required = required.contains(&(mi, ordinal));
            let shader = mat
                .shader_offset
                .map(|o| text(&mesh.string_table, o, l))
                .transpose()?;
            let mut properties = Vec::new();
            let mut policy_properties = Vec::new();
            for p in &mat.properties {
                let name = text(&mesh.string_table, p.name_offset, l)?;
                let string_value = (p.kind == 2)
                    .then(|| text(&mesh.string_table, p.value, l))
                    .transpose()?;
                let archive_lookup = if is_required {
                    string_value
                        .as_deref()
                        .map(|requested| -> Result<ArchiveMemberLookup> {
                            charge(
                                &mut budget.report_records,
                                1,
                                l.max_report_records,
                                "inventory lookup record",
                            )?;
                            let fact = if archive::safe_name(requested, l).is_err() {
                                ArchiveMemberLookup::UnsafeName
                            } else if let Some((exact_name, advertised_bytes)) =
                                archive.members.get(&requested.to_ascii_lowercase())
                            {
                                charge(
                                    &mut budget.report_string_bytes,
                                    exact_name.len(),
                                    l.max_total_name_bytes,
                                    "inventory lookup string",
                                )?;
                                ArchiveMemberLookup::Present {
                                    exact_name: exact_name.clone(),
                                    advertised_bytes: *advertised_bytes,
                                }
                            } else {
                                ArchiveMemberLookup::Missing
                            };
                            Ok(fact)
                        })
                        .transpose()?
                } else {
                    None
                };
                policy_properties.push(report::PropertyReport {
                    raw_name_offset: p.name_offset,
                    name: name.clone(),
                    kind: p.kind,
                    value_word: p.value,
                    resource_index: None,
                });
                properties.push(MaterialPropertyInventory {
                    raw_name_offset: p.name_offset,
                    name,
                    kind: p.kind,
                    value_word: p.value,
                    string_value,
                    archive_lookup,
                });
            }
            let schema = if !is_required {
                MaterialSchemaAssessment::UninterpretedUnused
            } else {
                let assessment = shader
                    .as_deref()
                    .context("missing/null required shader")
                    .and_then(|sh| materials::validate(sh, &policy_properties));
                match assessment {
                    Ok(()) => MaterialSchemaAssessment::Compatible,
                    Err(e) => {
                        unsupported += 1;
                        let diagnostic = e.to_string();
                        charge(
                            &mut budget.report_string_bytes,
                            diagnostic.len(),
                            l.max_total_name_bytes,
                            "inventory diagnostic string",
                        )?;
                        MaterialSchemaAssessment::Unsupported { diagnostic }
                    }
                }
            };
            rows.push(MaterialInventoryRow {
                source_mesh_index: mi,
                source_material_ordinal: ordinal,
                source_declared_id: mat.index,
                raw_name_offset: mat.name_offset,
                raw_shader_offset: mat.shader_offset,
                name: text(&mesh.string_table, mat.name_offset, l)?,
                shader,
                required: is_required,
                schema,
                properties,
            });
        }
    }
    archive.snapshot.verify()?;
    if let Some(snapshot) = &mut loose {
        snapshot.verify()?;
    }
    let mut report = EqgPbrMaterialInventory {
        schema: "eqg-pbr-material-inventory-v1", scope: "material-schema-and-archive-directory-only",
        readiness: "not-evaluated", inventory_revision: String::new(), implementation_revision: implementation_revision(),
        archive_blake3: archive.snapshot.hash.clone(), descriptor: identity, descriptor_version: source.version,
        models, model_slots: source.model_slots, required_materials: required.len(), unsupported_required_materials: unsupported,
        materials: rows, budget, limitations: vec![
            "Readiness not evaluated: schema compatibility and archive-member presence do not validate textures, geometry or lighting.",
            "Descriptor v1 is not admitted to the existing v2-only PBR preview.",
            "Unused materials and unknown/null shader records remain uninterpreted; kind-2 lookup facts assign no shader semantics.",
            "No resources decoded, no geometry bridged, no lighting evaluated, no visual artifact produced.",
            "Unselected triangle material references and geometry completeness are not checked.",
            "Trusted stable local files with observed before/after content checks; no authentication or hostile concurrent mutate-and-restore guarantee.",
            "All advertised archive bytes preflighted; logical source limits apply after assembly. PFS inflation does not establish peak RAM bounds.",
        ],
    };
    report.inventory_revision = hash(&bounded_json(&report, l.max_report_json_bytes)?);
    bounded_json(&report, l.max_report_json_bytes)?;
    Ok(report)
}
pub(super) fn assemble_source(
    path: &Path,
    descriptor: DescriptorSource<'_>,
    bytes: &[u8],
    selected: Option<&str>,
    archive: &mut archive::Archive,
    l: &EqgPbrLimits,
) -> Result<(BinaryZoneScene, Vec<report::SourceModelIdentity>)> {
    let (name, location) = match descriptor {
        DescriptorSource::Loose(p) => (p.display().to_string(), "loose"),
        DescriptorSource::Archive(_) => (
            selected.context("missing selected descriptor")?.to_owned(),
            "archive",
        ),
    };
    struct Members<'a> {
        archive: &'a mut archive::Archive,
        limits: &'a EqgPbrLimits,
        models: Vec<report::SourceModelIdentity>,
        model_records: usize,
        model_string_bytes: usize,
    }
    impl super::super::zon::ZoneMembers for Members<'_> {
        fn exact(&self, requested: &str) -> Result<String> {
            self.archive
                .exact(requested, self.limits)
                .map(|(name, _)| name)
        }
        fn read(&mut self, exact: &str) -> Result<Vec<u8>> {
            // Streaming identities precede the full source projection preflight.
            charge(
                &mut self.model_records,
                1,
                self.limits.max_report_records,
                "inventory model record",
            )?;
            charge(
                &mut self.model_string_bytes,
                exact.len(),
                self.limits.max_total_name_bytes,
                "inventory model string",
            )?;
            charge(
                &mut self.model_string_bytes,
                64,
                self.limits.max_total_name_bytes,
                "inventory model string",
            )?;
            let bytes = self.archive.read(exact, self.limits.max_member_bytes)?;
            self.models.push(report::SourceModelIdentity {
                mesh_index: self.models.len(),
                exact_name: exact.to_owned(),
                blake3: hash(&bytes),
            });
            Ok(bytes)
        }
    }
    let mut members = Members {
        archive,
        limits: l,
        models: Vec::new(),
        model_records: 0,
        model_string_bytes: 0,
    };
    let source =
        super::super::zon::assemble_binary_zone(path, name.clone(), location, bytes, &mut members)
            .with_context(|| {
                format!(
                    "binary zone descriptor {name:?} in archive {}",
                    path.display()
                )
            })?;
    let models = members.models;
    Ok((source, models))
}

fn implementation_revision() -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"eqg-pbr-material-inventory-implementation-v1");
    for bytes in [
        include_bytes!("inventory.rs").as_slice(),
        include_bytes!("../pbr_visual.rs"),
        include_bytes!("archive.rs"),
        include_bytes!("materials.rs"),
        include_bytes!("report.rs"),
        include_bytes!("../zon.rs"),
        include_bytes!("../surface.rs"),
        include_bytes!("../../../Cargo.lock"),
        include_bytes!("../mod.rs"),
    ] {
        h.update(&(bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    h.finalize().to_hex().to_string()
}
