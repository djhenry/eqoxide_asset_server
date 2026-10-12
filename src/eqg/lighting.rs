//! Bounded source-lighting diagnostics. Packed words remain uninterpreted.
use super::BinaryZoneScene;
use anyhow::{Context, Result, bail, ensure};
use libeq_eqg::lighting as raw;
use libeq_eqg::mesh::MeshKind;
use std::collections::BTreeMap;

/// Named bytes supplied by the caller; provenance is an opaque caller label.
#[derive(Debug, Clone, Copy)]
pub struct ExternalResource<'a> {
    pub name: &'a str,
    pub provenance: &'a str,
    pub bytes: &'a [u8],
}

/// Incremental work limits; these do not bound earlier source assembly.
#[derive(Debug, Clone, Copy)]
pub struct DiagnosticLimits {
    pub max_targets: usize,
    pub max_resources: usize,
    pub max_logical_records: usize,
    pub max_name_bytes: usize,
    pub max_input_bytes: usize,
    pub max_words: usize,
}
impl Default for DiagnosticLimits {
    fn default() -> Self {
        Self {
            max_targets: 100_000,
            max_resources: 10_000,
            max_logical_records: 16_000_000,
            max_name_bytes: 16 * 1024 * 1024,
            max_input_bytes: 128 * 1024 * 1024,
            max_words: 16_777_216,
        }
    }
}

#[derive(Debug)]
pub struct LightingDiagnostics<'a> {
    pub source_archive: &'a std::path::Path,
    pub source_descriptor_name: &'a str,
    pub source_descriptor_location: &'a str,
    pub source_version: u32,
    pub targets: Vec<TargetDiagnostic<'a>>,
    /// Every supplied resource, including unused resources, parsed at most once.
    pub resources: Vec<ResourceDiagnostic<'a>>,
}
#[derive(Debug)]
pub struct ResourceDiagnostic<'a> {
    pub input: ExternalResource<'a>,
    pub parsed: std::result::Result<raw::Lighting<'a>, raw::ParseError>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Terrain,
    Placement,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountComparison {
    Empty,
    DiffersFromCurrentPool,
    MatchesCurrentPoolOnly,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRefusal {
    Empty,
    NonUtf8,
    UnsafePathOrControl,
    InvalidTerrainExtension,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnassignedReason {
    NullModelSlot,
    NullResolvedMesh,
    RepeatedTerrainPlacement,
}
#[derive(Debug)]
pub enum Candidate<'a> {
    External {
        requested_name: String,
        resource_index: usize,
    },
    MissingInExplicitInputs {
        requested_name: String,
    },
    RefusedName(NameRefusal),
    /// Record zero supplies terrain, regardless of its model reference.
    Inline {
        record_ordinal: usize,
        packed_bytes: &'a [u8],
    },
    UnsupportedMissingVersionTwoRecordZero,
}
#[derive(Debug)]
pub enum TargetOutcome {
    Unassigned(UnassignedReason),
    InvalidOrUnsupportedClassicResource,
    NoCandidate,
    Words {
        count: usize,
        comparison: CountComparison,
    },
}
/// Names are candidates under this diagnostic's explicit ASCII case-fold policy.
/// Terrain uses the resolved member spelling, not a claimed native loader request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingBasis {
    ResolvedTerrainMemberDiagnostic,
    RawPlacementName,
    InlineRecord,
}
/// A shape comparison never certifies a native consumer or its complete inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumerScope {
    CurrentPoolOnlyNativeContextUnresolved,
    HierarchicalConsumerUnsupported,
    UnassignedTarget,
}
#[derive(Debug)]
pub struct TargetDiagnostic<'a> {
    pub kind: TargetKind,
    pub naming_basis: NamingBasis,
    pub consumer_scope: ConsumerScope,
    pub placement_ordinal: Option<usize>,
    pub raw_placement_name: Option<&'a [u8]>,
    pub original_model_slot: Option<u32>,
    /// Placement's resolved identity (including v2 metadata); not necessarily its target.
    pub placement_mesh_index: Option<usize>,
    pub target_mesh_index: Option<usize>,
    pub source_mesh_name: Option<&'a str>,
    pub current_pool_count: Option<usize>,
    pub candidate: Candidate<'a>,
    pub outcome: TargetOutcome,
}

/// Inspect source identities and raw shapes without discovering providers or applying lighting.
pub fn diagnose<'a>(
    scene: &'a BinaryZoneScene,
    inputs: &[ExternalResource<'a>],
    limits: &DiagnosticLimits,
) -> Result<LightingDiagnostics<'a>> {
    // No report arrays, generated names or resource parsing until all budgets and
    // every source reference this function uses have passed preflight.
    preflight(scene, inputs, limits)?;
    let mut index = BTreeMap::new();
    for (i, input) in inputs.iter().enumerate() {
        ensure!(
            index.insert(input.name.to_ascii_lowercase(), i).is_none(),
            "ambiguous ASCII-case-equivalent external resource names"
        );
    }
    let resources: Vec<_> = inputs
        .iter()
        .map(|input| ResourceDiagnostic {
            input: *input,
            parsed: raw::parse_with_limits(
                input.bytes,
                &raw::Limits {
                    max_sample_count: limits.max_words,
                    max_payload_bytes: limits.max_input_bytes,
                },
            ),
        })
        .collect();
    let terrain = scene
        .meshes
        .iter()
        .position(|m| m.kind == MeshKind::Terrain)
        .context("missing terrain mesh")?;
    let mut targets = Vec::with_capacity(target_count(scene)?);
    let first = (scene.version == 2)
        .then(|| scene.placements.first())
        .flatten();
    let terrain_candidate = if scene.version == 2 {
        first.map_or(Candidate::UnsupportedMissingVersionTwoRecordZero, |p| {
            Candidate::Inline {
                record_ordinal: 0,
                packed_bytes: &p.extension_data,
            }
        })
    } else {
        external_candidate(terrain_name(&scene.meshes[terrain].source_name), &index)
    };
    targets.push(target(
        scene,
        TargetKind::Terrain,
        first.map(|_| 0),
        Some(terrain),
        terrain_candidate,
        &resources,
    ));
    for (ordinal, p) in scene
        .placements
        .iter()
        .enumerate()
        .skip(usize::from(scene.version == 2))
    {
        let mi = p
            .model_index
            .and_then(|slot| scene.model_slots[slot as usize]);
        let candidate = if scene.version == 2 {
            Candidate::Inline {
                record_ordinal: ordinal,
                packed_bytes: &p.extension_data,
            }
        } else {
            external_candidate(
                placement_name(source_string(&scene.string_table, p.name_offset)?),
                &index,
            )
        };
        targets.push(target(
            scene,
            TargetKind::Placement,
            Some(ordinal),
            mi,
            candidate,
            &resources,
        ));
    }
    Ok(LightingDiagnostics {
        source_archive: &scene.archive_path,
        source_descriptor_name: &scene.descriptor_name,
        source_descriptor_location: scene.descriptor_location,
        source_version: scene.version,
        targets,
        resources,
    })
}

fn target<'a>(
    scene: &'a BinaryZoneScene,
    kind: TargetKind,
    ordinal: Option<usize>,
    mi: Option<usize>,
    candidate: Candidate<'a>,
    resources: &[ResourceDiagnostic<'a>],
) -> TargetDiagnostic<'a> {
    let placement = ordinal.map(|i| &scene.placements[i]);
    let placement_mesh_index = placement
        .and_then(|p| p.model_index)
        .and_then(|slot| scene.model_slots[slot as usize]);
    let mesh = mi.map(|i| &scene.meshes[i]);
    let current_pool_count = mesh.map(|m| m.vertices.len());
    let reason = if kind == TargetKind::Placement {
        if placement.unwrap().model_index.is_none() {
            Some(UnassignedReason::NullModelSlot)
        } else if mesh.is_none() {
            Some(UnassignedReason::NullResolvedMesh)
        } else if mesh.unwrap().kind == MeshKind::Terrain {
            Some(UnassignedReason::RepeatedTerrainPlacement)
        } else {
            None
        }
    } else {
        None
    };
    let outcome = if let Some(reason) = reason {
        TargetOutcome::Unassigned(reason)
    } else {
        let count = match &candidate {
            Candidate::Inline { packed_bytes, .. } => Some(packed_bytes.len() / 4),
            Candidate::External { resource_index, .. } => resources[*resource_index]
                .parsed
                .as_ref()
                .ok()
                .map(|p| p.sample_count() as usize),
            _ => None,
        };
        if let Some(count) = count {
            TargetOutcome::Words {
                count,
                comparison: if count == 0 {
                    CountComparison::Empty
                } else if Some(count) == current_pool_count {
                    CountComparison::MatchesCurrentPoolOnly
                } else {
                    CountComparison::DiffersFromCurrentPool
                },
            }
        } else if matches!(candidate, Candidate::External { .. }) {
            TargetOutcome::InvalidOrUnsupportedClassicResource
        } else {
            TargetOutcome::NoCandidate
        }
    };
    let naming_basis = if scene.version == 2 {
        NamingBasis::InlineRecord
    } else if kind == TargetKind::Terrain {
        NamingBasis::ResolvedTerrainMemberDiagnostic
    } else {
        NamingBasis::RawPlacementName
    };
    let consumer_scope = if reason.is_some() {
        ConsumerScope::UnassignedTarget
    } else if mesh.is_some_and(|m| m.bone_count.is_some_and(|count| count != 0)) {
        ConsumerScope::HierarchicalConsumerUnsupported
    } else {
        ConsumerScope::CurrentPoolOnlyNativeContextUnresolved
    };
    TargetDiagnostic {
        kind,
        naming_basis,
        consumer_scope,
        placement_ordinal: ordinal,
        // Preflight validated all offsets and termination.
        raw_placement_name: placement
            .map(|p| source_string(&scene.string_table, p.name_offset).unwrap()),
        original_model_slot: placement.and_then(|p| p.model_index),
        placement_mesh_index,
        target_mesh_index: mi,
        source_mesh_name: mesh.map(|m| m.source_name.as_str()),
        current_pool_count,
        candidate,
        outcome,
    }
}
fn external_candidate<'a>(
    name: std::result::Result<String, NameRefusal>,
    index: &BTreeMap<String, usize>,
) -> Candidate<'a> {
    match name {
        Err(reason) => Candidate::RefusedName(reason),
        Ok(requested_name) => match index.get(&requested_name.to_ascii_lowercase()) {
            Some(&resource_index) => Candidate::External {
                requested_name,
                resource_index,
            },
            None => Candidate::MissingInExplicitInputs { requested_name },
        },
    }
}
fn safe_name(bytes: &[u8]) -> std::result::Result<&str, NameRefusal> {
    let s = std::str::from_utf8(bytes).map_err(|_| NameRefusal::NonUtf8)?;
    if s.is_empty() {
        return Err(NameRefusal::Empty);
    }
    if bytes
        .iter()
        .any(|b| b.is_ascii_control() || matches!(b, b'/' | b'\\' | b':'))
        || s == "."
        || s == ".."
    {
        return Err(NameRefusal::UnsafePathOrControl);
    }
    Ok(s)
}
fn terrain_name(name: &str) -> std::result::Result<String, NameRefusal> {
    let s = safe_name(name.as_bytes())?;
    // The generic source convention removes four BYTES. We deliberately limit
    // this diagnostic candidate to a nonempty basename with a .ter extension.
    if s.len() <= 4 || !s.as_bytes()[s.len() - 4..].eq_ignore_ascii_case(b".ter") {
        return Err(NameRefusal::InvalidTerrainExtension);
    }
    Ok(format!("{}.LIT", &s[..s.len() - 4]))
}
fn placement_name(name: &[u8]) -> std::result::Result<String, NameRefusal> {
    Ok(format!("{}.LIT", safe_name(name)?))
}
fn target_count(scene: &BinaryZoneScene) -> Result<usize> {
    // V2 record zero belongs to the terrain diagnostic instead of an object row.
    1usize
        .checked_add(
            scene
                .placements
                .len()
                .saturating_sub(usize::from(scene.version == 2)),
        )
        .context("target count overflow")
}
fn charge(total: &mut usize, amount: usize, limit: usize, what: &str) -> Result<()> {
    *total = total
        .checked_add(amount)
        .with_context(|| format!("{what} limit arithmetic overflow"))?;
    ensure!(*total <= limit, "{what} limit exceeded");
    Ok(())
}
fn multiplied(n: usize, factor: usize) -> Result<usize> {
    n.checked_mul(factor)
        .context("name limit arithmetic overflow")
}
fn source_string(table: &[u8], offset: u32) -> Result<&[u8]> {
    let bytes = table
        .get(usize::try_from(offset)?..)
        .context("invalid source string offset")?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .context("unterminated source string")?;
    Ok(&bytes[..end])
}
fn bounded_string<'a>(
    table: &'a [u8],
    offset: u32,
    names: &mut usize,
    limit: usize,
) -> Result<&'a [u8]> {
    let bytes = table
        .get(usize::try_from(offset)?..)
        .context("invalid source string offset")?;
    // Bound the actual NUL search, even when thousands of offsets repeat a huge
    // string. The remaining units account for later scans and generated-name copies.
    let available = limit.saturating_sub(*names);
    let scan = &bytes[..bytes.len().min(available)];
    let end = match scan.iter().position(|&b| b == 0) {
        Some(end) => end,
        None if scan.len() < bytes.len() => bail!("name/scan limit exceeded"),
        None => bail!("unterminated source string"),
    };
    charge(names, multiplied(end + 1, 6)?, limit, "name/scan")?;
    Ok(&bytes[..end])
}
fn preflight(
    scene: &BinaryZoneScene,
    inputs: &[ExternalResource<'_>],
    limits: &DiagnosticLimits,
) -> Result<()> {
    ensure!(
        target_count(scene)? <= limits.max_targets,
        "target limit exceeded"
    );
    ensure!(
        inputs.len() <= limits.max_resources,
        "resource limit exceeded"
    );
    let mut records = 0usize;
    for count in [
        scene.meshes.len(),
        scene.model_slots.len(),
        scene.model_name_offsets.len(),
        scene.placements.len(),
        scene.regions.len(),
        scene.lights.len(),
        inputs.len(),
    ] {
        charge(
            &mut records,
            count,
            limits.max_logical_records,
            "logical record",
        )?;
    }
    let mut bytes = 0usize;
    let mut names = 0usize;
    let mut words = 0usize;
    charge(
        &mut bytes,
        scene.string_table.len(),
        limits.max_input_bytes,
        "input byte",
    )?;
    charge(
        &mut bytes,
        scene.trailing_data.len(),
        limits.max_input_bytes,
        "input byte",
    )?;
    charge(
        &mut names,
        scene.string_table.len(),
        limits.max_name_bytes,
        "name/scan",
    )?;
    charge(
        &mut names,
        scene.archive_path.as_os_str().len(),
        limits.max_name_bytes,
        "name/scan",
    )?;
    charge(
        &mut names,
        scene.descriptor_name.len(),
        limits.max_name_bytes,
        "name/scan",
    )?;
    charge(
        &mut names,
        scene.descriptor_location.len(),
        limits.max_name_bytes,
        "name/scan",
    )?;
    charge(
        &mut names,
        multiplied(target_count(scene)?, 8)?,
        limits.max_name_bytes,
        "name/scan",
    )?;
    for mesh in &scene.meshes {
        for count in [
            mesh.vertices.len(),
            mesh.triangles.len(),
            mesh.materials.len(),
        ] {
            charge(
                &mut records,
                count,
                limits.max_logical_records,
                "logical record",
            )?;
        }
        for material in &mesh.materials {
            charge(
                &mut records,
                material.properties.len(),
                limits.max_logical_records,
                "logical record",
            )?;
        }
        charge(
            &mut bytes,
            mesh.string_table.len(),
            limits.max_input_bytes,
            "input byte",
        )?;
        charge(
            &mut bytes,
            mesh.trailing_data.len(),
            limits.max_input_bytes,
            "input byte",
        )?;
        charge(
            &mut names,
            multiplied(mesh.source_name.len(), 4)?,
            limits.max_name_bytes,
            "name/scan",
        )?;
    }
    for p in &scene.placements {
        charge(
            &mut bytes,
            p.extension_data.len(),
            limits.max_input_bytes,
            "input byte",
        )?;
        charge(
            &mut words,
            p.extension_data.len() / 4,
            limits.max_words,
            "word",
        )?;
        charge(
            &mut records,
            p.extension_data.len() / 4,
            limits.max_logical_records,
            "logical record",
        )?;
    }
    for input in inputs {
        charge(
            &mut bytes,
            input.bytes.len(),
            limits.max_input_bytes,
            "input byte",
        )?;
        charge(
            &mut names,
            multiplied(input.name.len(), 2)?,
            limits.max_name_bytes,
            "name/scan",
        )?;
        charge(
            &mut names,
            input.provenance.len(),
            limits.max_name_bytes,
            "name/scan",
        )?;
        if input.bytes.len() >= 8 && &input.bytes[..4] == b"EQGP" {
            let count = u32::from_le_bytes(input.bytes[4..8].try_into().unwrap());
            charge(
                &mut words,
                usize::try_from(count)?,
                limits.max_words,
                "word",
            )?;
            charge(
                &mut records,
                usize::try_from(count)?,
                limits.max_logical_records,
                "logical record",
            )?;
        }
    }
    ensure!(
        matches!(scene.version, 1 | 2),
        "unsupported source zone version"
    );
    ensure!(
        scene.model_slots.len() == scene.model_name_offsets.len(),
        "model slot/offset cardinality mismatch"
    );
    let mut terrains = 0usize;
    for mesh in &scene.meshes {
        ensure!(
            (1..=3).contains(&mesh.version),
            "unsupported source mesh version"
        );
        terrains += usize::from(mesh.kind == MeshKind::Terrain);
    }
    ensure!(
        terrains == 1,
        "diagnostics require exactly one distinct terrain mesh"
    );
    for (offset, mi) in scene.model_name_offsets.iter().zip(&scene.model_slots) {
        ensure!(
            offset.is_some() == mi.is_some(),
            "inconsistent nullable model slot"
        );
        if let Some(offset) = offset {
            bounded_string(
                &scene.string_table,
                *offset,
                &mut names,
                limits.max_name_bytes,
            )?;
        }
        if let Some(mi) = mi {
            ensure!(*mi < scene.meshes.len(), "invalid model mesh index");
        }
    }
    for p in &scene.placements {
        bounded_string(
            &scene.string_table,
            p.name_offset,
            &mut names,
            limits.max_name_bytes,
        )?;
        if let Some(slot) = p.model_index {
            ensure!(
                usize::try_from(slot)? < scene.model_slots.len(),
                "invalid placement model slot"
            );
        }
        ensure!(
            p.extension_data.len() % 4 == 0,
            "unaligned inline word bytes"
        );
        ensure!(
            scene.version == 2 || p.extension_data.is_empty(),
            "version one has inline extension bytes"
        );
    }
    for offset in scene
        .regions
        .iter()
        .map(|r| r.name_offset)
        .chain(scene.lights.iter().map(|l| l.name_offset))
    {
        bounded_string(
            &scene.string_table,
            offset,
            &mut names,
            limits.max_name_bytes,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
