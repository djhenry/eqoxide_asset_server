//! Pure normalization of explicitly selected WLD terrain collision candidates.
//! Source preparation does not establish ownership, world completeness or native query parity.
use crate::{compatibility::token, static_collision::*};
use libeq_wld::parser::{DmSpriteDef2, FragmentRef, FragmentType, WldDoc};
use std::collections::{BTreeMap, BTreeSet};

pub const NORMALIZATION_POLICY: &str = "wld-selected-terrain-coincident-exclusion-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WldCollisionBudget {
    DocumentVisits,
    SelectedOwners,
    SourceVertices,
    SourceFaces,
    RetainedIndices,
    SourceGeometryBytes,
    OwnedPayloadBytes,
    ResourceEntries,
    FaceVisits,
    DecodedVertices,
}
/// Caller-lowerable engineering limits; allocator overhead and prior parsing are excluded.
///
/// These fixed logical byte charges are accounting units, not Rust layout or RSS sizes:
///
/// | Item | Logical bytes |
/// | --- | ---: |
/// | Result, report, usage and result collection handles | 256 |
/// | Prepared container / borrowed entry | 24 / 80 |
/// | Each duplicate-set container / slot entry / borrowed owner entry | 24 / 8 / 16 |
/// | One reusable histogram bitset | 8192 |
/// | Owner report, including its text and histogram handles | 160 |
/// | Histogram output entry: u16 flag and u64 occurrence count | 10 |
/// | Temporary histogram map container / entry | 24 / 16 |
/// | Contributing mesh / primitive / instance | 48 / 24 / 96 |
/// | Output position / retained index | 12 / 4 |
/// | Copied context or owner text | UTF-8 byte length |
///
/// Charges are cumulative, including temporary containers dropped before return.
/// Context text is copied once; each report owner once; contributing instance owners
/// once more. For S selections, H total distinct per-owner face flags and C contributors,
/// resource usage is exactly `4 + 5*S + 2*H + 3*C`: prepared container and entries,
/// both duplicate sets and entries, bitset, owner reports, temporary histogram map
/// containers and entries, histogram output entries, meshes, primitives and instances.
///
/// Count-only preflight charges every allocation/copy before execution and finishes
/// all selected records before copying identity text or decoding output positions.
/// Document visits include coverage plus selected lookups. Source vertices/faces
/// are charged once; face visits are `2*all_faces + contributing_faces`. Every selected
/// vertex, including empty owners' unused vertices, is decoded exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WldCollisionLimits {
    pub document_visits: usize,
    pub selected_owners: usize,
    pub source_vertices: usize,
    pub source_faces: usize,
    pub retained_indices: usize,
    pub source_geometry_bytes: usize,
    pub owned_payload_bytes: usize,
    pub resource_entries: usize,
    pub face_visits: usize,
    pub decoded_vertices: usize,
}
impl WldCollisionLimits {
    pub const HARD: Self = Self {
        document_visits: 100000,
        selected_owners: 100000,
        source_vertices: 5000000,
        source_faces: 5000000,
        retained_indices: 15000000,
        source_geometry_bytes: 134217728,
        owned_payload_bytes: 134217728,
        resource_entries: 100000,
        face_visits: 15000000,
        decoded_vertices: 5000000,
    };
}
impl Default for WldCollisionLimits {
    fn default() -> Self {
        Self::HARD
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WldCollisionUsage {
    pub document_visits: usize,
    pub selected_owners: usize,
    pub source_vertices: usize,
    pub source_faces: usize,
    pub retained_indices: usize,
    pub source_geometry_bytes: usize,
    pub owned_payload_bytes: usize,
    pub resource_entries: usize,
    pub face_visits: usize,
    pub decoded_vertices: usize,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WldCollisionError {
    #[error("invalid limit: {0:?}")]
    InvalidLimits(WldCollisionBudget),
    #[error("budget exhausted: {budget:?}, attempted {attempted}, maximum {maximum}")]
    BudgetExceeded {
        budget: WldCollisionBudget,
        attempted: usize,
        maximum: usize,
    },
    #[error("invalid selection: {0}")]
    InvalidSelection(&'static str),
    #[error("unsupported required source at slot {slot}: {reason}")]
    UnsupportedRequiredSource { slot: usize, reason: &'static str },
    #[error("inconsistent source at slot {slot}: {field}")]
    InconsistentSource { slot: usize, field: &'static str },
    #[error("invalid geometry at slot {slot}: {reason}")]
    InvalidGeometry { slot: usize, reason: &'static str },
    #[error("common triangle admission refused source at slot {slot}")]
    CommonGeometryRefusal { slot: usize },
}
#[derive(Debug, Clone, Copy)]
pub struct WldTerrainSelection<'a> {
    pub fragment_slot: usize,
    pub owner_id: &'a str,
}
pub struct WldTerrainInput<'a> {
    pub document: &'a WldDoc,
    /// Opaque caller identity; no content digest is verified here.
    pub source_context: &'a str,
    pub selections: &'a [WldTerrainSelection<'a>],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WldSourceReference {
    Name(i32),
    Index(u32),
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WldFaceCounts {
    pub source_faces: usize,
    pub source_flag_excluded: usize,
    pub raw_candidates: usize,
    pub coincident_source_positions_excluded: usize,
    pub retained_faces: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WldFaceFlagCount {
    pub flags: u16,
    pub count: u64,
}
#[derive(Debug)]
pub struct WldOwnerReport {
    pub owner_id: String,
    pub fragment_slot: usize,
    pub exponent: u16,
    pub source_center: [f32; 3],
    pub raw_mesh_flags: u32,
    pub fragment3: WldSourceReference,
    pub fragment4: WldSourceReference,
    pub faces: WldFaceCounts,
    pub raw_face_flags: Vec<WldFaceFlagCount>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WldCoverage {
    pub unselected_modern_meshes: usize,
    pub old_meshes: usize,
    pub unknown: usize,
    pub actors: usize,
    pub other: usize,
}
#[derive(Debug)]
pub struct WldTerrainReport {
    pub source_context: String,
    pub normalization_policy: &'static str,
    pub ownership_verified: bool,
    pub world_complete: bool,
    pub full_native_query_preparation_applied: bool,
    pub coverage: WldCoverage,
    pub faces: WldFaceCounts,
    pub owners: Vec<WldOwnerReport>,
}
#[derive(Debug)]
pub struct NormalizedWldTerrain {
    pub scene: StaticCollisionScene,
    pub report: WldTerrainReport,
    pub usage: WldCollisionUsage,
}

const RESULT_BYTES: usize = 256;
const PREPARED_BYTES: usize = 80;
const OWNER_REPORT_BYTES: usize = 160;
const BITSET_BYTES: usize = 8192;
const CONTAINER_BYTES: usize = 24;
const MESH_BYTES: usize = 48;
const PRIMITIVE_BYTES: usize = 24;
const INSTANCE_BYTES: usize = 96;

fn checked_product(
    a: usize,
    b: usize,
    budget: WldCollisionBudget,
    maximum: usize,
) -> Result<usize, WldCollisionError> {
    a.checked_mul(b).ok_or(WldCollisionError::BudgetExceeded {
        budget,
        attempted: usize::MAX,
        maximum,
    })
}
struct Budget<'a> {
    limits: &'a WldCollisionLimits,
    usage: WldCollisionUsage,
}
impl Budget<'_> {
    fn charge(&mut self, kind: WldCollisionBudget, n: usize) -> Result<(), WldCollisionError> {
        let (total, maximum) = match kind {
            WldCollisionBudget::DocumentVisits => {
                (&mut self.usage.document_visits, self.limits.document_visits)
            }
            WldCollisionBudget::SelectedOwners => {
                (&mut self.usage.selected_owners, self.limits.selected_owners)
            }
            WldCollisionBudget::SourceVertices => {
                (&mut self.usage.source_vertices, self.limits.source_vertices)
            }
            WldCollisionBudget::SourceFaces => {
                (&mut self.usage.source_faces, self.limits.source_faces)
            }
            WldCollisionBudget::RetainedIndices => (
                &mut self.usage.retained_indices,
                self.limits.retained_indices,
            ),
            WldCollisionBudget::SourceGeometryBytes => (
                &mut self.usage.source_geometry_bytes,
                self.limits.source_geometry_bytes,
            ),
            WldCollisionBudget::OwnedPayloadBytes => (
                &mut self.usage.owned_payload_bytes,
                self.limits.owned_payload_bytes,
            ),
            WldCollisionBudget::ResourceEntries => (
                &mut self.usage.resource_entries,
                self.limits.resource_entries,
            ),
            WldCollisionBudget::FaceVisits => {
                (&mut self.usage.face_visits, self.limits.face_visits)
            }
            WldCollisionBudget::DecodedVertices => (
                &mut self.usage.decoded_vertices,
                self.limits.decoded_vertices,
            ),
        };
        let attempted = total.checked_add(n).unwrap_or(usize::MAX);
        if attempted > maximum {
            return Err(WldCollisionError::BudgetExceeded {
                budget: kind,
                attempted,
                maximum,
            });
        }
        *total = attempted;
        Ok(())
    }
    fn payload(&mut self, bytes: usize, entries: usize) -> Result<(), WldCollisionError> {
        self.charge(WldCollisionBudget::OwnedPayloadBytes, bytes)?;
        self.charge(WldCollisionBudget::ResourceEntries, entries)
    }
    fn scaled_payload(&mut self, count: usize, width: usize) -> Result<(), WldCollisionError> {
        let bytes = checked_product(
            count,
            width,
            WldCollisionBudget::OwnedPayloadBytes,
            self.limits.owned_payload_bytes,
        )?;
        self.payload(bytes, 0)
    }
}
impl WldCollisionLimits {
    fn validate(&self) -> Result<(), WldCollisionError> {
        if self.document_visits > Self::HARD.document_visits {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::DocumentVisits,
            ));
        }
        if self.selected_owners > Self::HARD.selected_owners {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::SelectedOwners,
            ));
        }
        if self.source_vertices > Self::HARD.source_vertices {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::SourceVertices,
            ));
        }
        if self.source_faces > Self::HARD.source_faces {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::SourceFaces,
            ));
        }
        if self.retained_indices > Self::HARD.retained_indices {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::RetainedIndices,
            ));
        }
        if self.source_geometry_bytes > Self::HARD.source_geometry_bytes {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::SourceGeometryBytes,
            ));
        }
        if self.owned_payload_bytes > Self::HARD.owned_payload_bytes {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::OwnedPayloadBytes,
            ));
        }
        if self.resource_entries > Self::HARD.resource_entries {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::ResourceEntries,
            ));
        }
        if self.face_visits > Self::HARD.face_visits {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::FaceVisits,
            ));
        }
        if self.decoded_vertices > Self::HARD.decoded_vertices {
            return Err(WldCollisionError::InvalidLimits(
                WldCollisionBudget::DecodedVertices,
            ));
        }

        Ok(())
    }
}
struct Prepared<'a> {
    selection: &'a WldTerrainSelection<'a>,
    record: &'a DmSpriteDef2,
    counts: WldFaceCounts,
    histogram: Vec<WldFaceFlagCount>,
}
#[derive(PartialEq)]
enum Face {
    SourceFlagExcluded,
    Coincident,
    Retained,
}
fn classify(
    record: &DmSpriteDef2,
    face: &libeq_wld::parser::DmSpriteDef2FaceEntry,
    slot: usize,
) -> Result<Face, WldCollisionError> {
    let (a, b, c) = face.vertex_indexes;
    let indices = [a as usize, b as usize, c as usize];
    if indices.iter().any(|&i| i >= record.positions.len()) {
        return Err(WldCollisionError::InvalidGeometry {
            slot,
            reason: "face index out of bounds",
        });
    }
    if face.flags & 0x10 != 0 {
        return Ok(Face::SourceFlagExcluded);
    }
    let [a, b, c] = indices.map(|i| record.positions[i]);
    if a == b || b == c || c == a {
        return Ok(Face::Coincident);
    }
    let u = [
        i64::from(b.0) - i64::from(a.0),
        i64::from(b.1) - i64::from(a.1),
        i64::from(b.2) - i64::from(a.2),
    ];
    let v = [
        i64::from(c.0) - i64::from(a.0),
        i64::from(c.1) - i64::from(a.1),
        i64::from(c.2) - i64::from(a.2),
    ];
    if [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ] == [0; 3]
    {
        return Err(WldCollisionError::InvalidGeometry {
            slot,
            reason: "distinct source positions are collinear",
        });
    }
    Ok(Face::Retained)
}
fn reference<T>(r: &FragmentRef<T>) -> WldSourceReference {
    match r {
        FragmentRef::Name(n, _) => WldSourceReference::Name(n.0),
        FragmentRef::Index(n, _) => WldSourceReference::Index(*n),
    }
}
fn decode(
    record: &DmSpriteDef2,
    p: (i16, i16, i16),
    slot: usize,
) -> Result<[f32; 3], WldCollisionError> {
    let unit = 2.0f64.powi(-i32::from(record.scale));
    let source = [
        (f64::from(p.0) * unit + f64::from(record.center.0)) as f32,
        (f64::from(p.1) * unit + f64::from(record.center.1)) as f32,
        (f64::from(p.2) * unit + f64::from(record.center.2)) as f32,
    ];
    if !source.iter().all(|v| v.is_finite()) {
        return Err(WldCollisionError::InvalidGeometry {
            slot,
            reason: "nonfinite reconstructed position",
        });
    }
    Ok([source[1], source[0], source[2]])
}
/// Normalize only explicitly selected records; errors return no partial scene or report.
/// Actor coverage counts both Actor and ActorDef records without resolving either.
pub fn normalize_selected_wld_terrain(
    input: WldTerrainInput<'_>,
    limits: &WldCollisionLimits,
) -> Result<NormalizedWldTerrain, WldCollisionError> {
    limits.validate()?;
    let mut budget = Budget {
        limits,
        usage: WldCollisionUsage::default(),
    };
    if !token(input.source_context) {
        return Err(WldCollisionError::InvalidSelection(
            "invalid source context token",
        ));
    }
    if input.selections.is_empty() {
        return Err(WldCollisionError::InvalidSelection(
            "selection must be nonempty",
        ));
    }
    budget.charge(WldCollisionBudget::SelectedOwners, input.selections.len())?;
    budget.charge(
        WldCollisionBudget::DocumentVisits,
        input.document.fragment_count(),
    )?;
    budget.payload(RESULT_BYTES, 0)?;
    budget.payload(CONTAINER_BYTES * 3 + BITSET_BYTES, 4)?;
    budget.scaled_payload(input.selections.len(), PREPARED_BYTES + 8 + 16)?;
    budget.charge(
        WldCollisionBudget::ResourceEntries,
        checked_product(
            input.selections.len(),
            3,
            WldCollisionBudget::ResourceEntries,
            limits.resource_entries,
        )?,
    )?;
    budget.payload(input.source_context.len(), 0)?;
    let mut slots = BTreeSet::new();
    let mut owner_ids = BTreeSet::new();
    for selection in input.selections {
        if !token(selection.owner_id) {
            return Err(WldCollisionError::InvalidSelection("invalid owner token"));
        }
        if !slots.insert(selection.fragment_slot) {
            return Err(WldCollisionError::InvalidSelection(
                "duplicate fragment slot",
            ));
        }
        if !owner_ids.insert(selection.owner_id) {
            return Err(WldCollisionError::InvalidSelection("duplicate owner token"));
        }
    }
    let mut coverage = WldCoverage::default();
    for (slot, fragment) in input.document.iter().enumerate() {
        if slots.contains(&slot) {
            continue;
        }
        match fragment {
            FragmentType::DmSpriteDef2(_) => coverage.unselected_modern_meshes += 1,
            FragmentType::DmSpriteDef(_) => coverage.old_meshes += 1,
            FragmentType::Unknown(_) => coverage.unknown += 1,
            FragmentType::Actor(_) | FragmentType::ActorDef(_) => coverage.actors += 1,
            _ => coverage.other += 1,
        }
    }
    let mut prepared = Vec::with_capacity(input.selections.len());
    let mut bitset = vec![0u8; BITSET_BYTES];
    let mut contributors = 0;
    let mut totals = WldFaceCounts::default();
    for selection in input.selections {
        budget.charge(WldCollisionBudget::DocumentVisits, 1)?;
        let slot = selection.fragment_slot;
        let record = match input.document.at(slot) {
            Some(FragmentType::DmSpriteDef2(record)) => record,
            Some(_) => {
                return Err(WldCollisionError::UnsupportedRequiredSource {
                    slot,
                    reason: "selected record is not a parsed modern mesh",
                })
            }
            None => {
                return Err(WldCollisionError::InvalidSelection(
                    "fragment slot out of bounds",
                ))
            }
        };
        validate_record(record, slot)?;
        let v = record.positions.len();
        let f = record.faces.len();
        budget.charge(WldCollisionBudget::SourceVertices, v)?;
        budget.charge(WldCollisionBudget::SourceFaces, f)?;
        budget.charge(
            WldCollisionBudget::SourceGeometryBytes,
            v.checked_mul(6)
                .and_then(|a| f.checked_mul(8).and_then(|b| a.checked_add(b)))
                .unwrap_or(usize::MAX),
        )?;
        budget.charge(WldCollisionBudget::DecodedVertices, v)?;
        budget.charge(WldCollisionBudget::FaceVisits, f)?;
        let mut counts = WldFaceCounts {
            source_faces: f,
            ..WldFaceCounts::default()
        };
        let mut distinct_flags = 0;
        for face in &record.faces {
            let flag = usize::from(face.flags);
            let mask = 1u8 << (flag % 8);
            if bitset[flag / 8] & mask == 0 {
                distinct_flags += 1;
                bitset[flag / 8] |= mask;
            }
            match classify(record, face, slot)? {
                Face::SourceFlagExcluded => counts.source_flag_excluded += 1,
                Face::Coincident => {
                    counts.raw_candidates += 1;
                    counts.coincident_source_positions_excluded += 1;
                }
                Face::Retained => {
                    counts.raw_candidates += 1;
                    counts.retained_faces += 1;
                }
            }
        }
        let indices = checked_product(
            counts.retained_faces,
            3,
            WldCollisionBudget::RetainedIndices,
            limits.retained_indices,
        )?;
        budget.charge(WldCollisionBudget::RetainedIndices, indices)?;
        budget.payload(OWNER_REPORT_BYTES + CONTAINER_BYTES, 2)?;
        budget.scaled_payload(distinct_flags, 10 + 16)?;
        budget.charge(
            WldCollisionBudget::ResourceEntries,
            checked_product(
                distinct_flags,
                2,
                WldCollisionBudget::ResourceEntries,
                limits.resource_entries,
            )?,
        )?;
        budget.payload(selection.owner_id.len(), 0)?;
        if counts.retained_faces > 0 {
            contributors += 1;
            budget.charge(WldCollisionBudget::FaceVisits, f)?;
            budget.payload(MESH_BYTES + PRIMITIVE_BYTES + INSTANCE_BYTES, 3)?;
            budget.scaled_payload(v, 12)?;
            budget.scaled_payload(indices, 4)?;
            budget.payload(selection.owner_id.len(), 0)?;
        }
        budget.charge(WldCollisionBudget::FaceVisits, f)?;
        let mut histogram = BTreeMap::<u16, u64>::new();
        for face in &record.faces {
            let flag = usize::from(face.flags);
            bitset[flag / 8] &= !(1u8 << (flag % 8));
            *histogram.entry(face.flags).or_default() += 1;
        }
        totals.source_faces += counts.source_faces;
        totals.source_flag_excluded += counts.source_flag_excluded;
        totals.raw_candidates += counts.raw_candidates;
        totals.coincident_source_positions_excluded += counts.coincident_source_positions_excluded;
        totals.retained_faces += counts.retained_faces;
        prepared.push(Prepared {
            selection,
            record,
            counts,
            histogram: histogram
                .into_iter()
                .map(|(flags, count)| WldFaceFlagCount { flags, count })
                .collect(),
        });
    }
    let mut meshes = Vec::with_capacity(contributors);
    let mut instances = Vec::with_capacity(contributors);
    let mut owners = Vec::with_capacity(prepared.len());
    for Prepared {
        selection,
        record,
        counts,
        histogram,
    } in prepared
    {
        let slot = selection.fragment_slot;
        let mut positions = Vec::with_capacity(if counts.retained_faces > 0 {
            record.positions.len()
        } else {
            0
        });
        for &p in &record.positions {
            let p = decode(record, p, slot)?;
            if counts.retained_faces > 0 {
                positions.push(p);
            }
        }
        if counts.retained_faces > 0 {
            let mut indices = Vec::with_capacity(counts.retained_faces * 3);
            for face in &record.faces {
                if classify(record, face, slot)? != Face::Retained {
                    continue;
                }
                let (a, b, c) = face.vertex_indexes;
                let t = [
                    positions[a as usize],
                    positions[b as usize],
                    positions[c as usize],
                ];
                crate::static_collision::triangle(t)
                    .and_then(|_| {
                        crate::static_collision::triangle(t.map(crate::static_scene::map_axes))
                    })
                    .map_err(|_| WldCollisionError::CommonGeometryRefusal { slot })?;
                indices.extend([u32::from(a), u32::from(b), u32::from(c)]);
            }
            instances.push(StaticCollisionInstance {
                owner_id: selection.owner_id.to_owned(),
                mesh_index: meshes.len(),
                matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
            });
            meshes.push(StaticCollisionMesh {
                positions,
                primitives: vec![StaticCollisionPrimitive { indices }],
            });
        }
        owners.push(WldOwnerReport {
            owner_id: selection.owner_id.to_owned(),
            fragment_slot: slot,
            exponent: record.scale,
            source_center: [record.center.0, record.center.1, record.center.2],
            raw_mesh_flags: record.flags,
            fragment3: reference(&record.fragment3),
            fragment4: reference(&record.fragment4),
            faces: counts,
            raw_face_flags: histogram,
        });
    }
    Ok(NormalizedWldTerrain {
        scene: if contributors == 0 {
            StaticCollisionScene::AvailableEmpty
        } else {
            StaticCollisionScene::Available { meshes, instances }
        },
        report: WldTerrainReport {
            source_context: input.source_context.to_owned(),
            normalization_policy: NORMALIZATION_POLICY,
            ownership_verified: false,
            world_complete: false,
            full_native_query_preparation_applied: false,
            coverage,
            faces: totals,
            owners,
        },
        usage: budget.usage,
    })
}
fn validate_record(record: &DmSpriteDef2, slot: usize) -> Result<(), WldCollisionError> {
    for (declared, actual, field) in [
        (record.position_count, record.positions.len(), "positions"),
        (
            record.texture_coordinate_count,
            record.texture_coordinates.len(),
            "texture coordinates",
        ),
        (record.normal_count, record.vertex_normals.len(), "normals"),
        (record.color_count, record.vertex_colors.len(), "colors"),
        (record.face_count, record.faces.len(), "faces"),
        (
            record.skin_assignment_groups_count,
            record.skin_assignment_groups.len(),
            "skin assignments",
        ),
        (
            record.face_material_groups_count,
            record.face_material_groups.len(),
            "face materials",
        ),
        (
            record.vertex_material_groups_count,
            record.vertex_material_groups.len(),
            "vertex materials",
        ),
        (record.meshop_count, record.meshops.len(), "mesh operations"),
    ] {
        if usize::from(declared) != actual {
            return Err(WldCollisionError::InconsistentSource { slot, field });
        }
    }
    if record.flags & 1 == 0 || record.flags & !0x1c003 != 0 {
        return Err(WldCollisionError::UnsupportedRequiredSource {
            slot,
            reason: "mesh flags outside static extraction subset",
        });
    }
    if !matches!(&record.animation_ref,FragmentRef::Name(n,_) if n.0==0)
        || !record.skin_assignment_groups.is_empty()
        || !record.meshops.is_empty()
    {
        return Err(WldCollisionError::UnsupportedRequiredSource {
            slot,
            reason: "unresolved dynamic geometry",
        });
    }
    if record.scale > 15 {
        return Err(WldCollisionError::UnsupportedRequiredSource {
            slot,
            reason: "unsupported position exponent",
        });
    }
    if ![record.center.0, record.center.1, record.center.2]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(WldCollisionError::InvalidGeometry {
            slot,
            reason: "nonfinite source center",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use libeq_wld::parser::{StringReference, TextureCoordinates};
    use std::marker::PhantomData;
    fn record() -> DmSpriteDef2 {
        DmSpriteDef2 {
            name_reference: StringReference(0),
            flags: 0x18003,
            material_list_ref: FragmentRef::new(i32::MAX),
            animation_ref: FragmentRef::new(0),
            fragment3: FragmentRef::new(-1),
            fragment4: FragmentRef::new(1),
            center: (0., 0., 0.),
            params2: (0, 0, 0),
            max_distance: f32::NAN,
            min: (f32::NAN, 0., 0.),
            max: (f32::INFINITY, 0., 0.),
            position_count: 0,
            texture_coordinate_count: 0,
            normal_count: 0,
            color_count: 0,
            face_count: 0,
            skin_assignment_groups_count: 0,
            face_material_groups_count: 0,
            vertex_material_groups_count: 0,
            meshop_count: 0,
            scale: 0,
            positions: vec![],
            texture_coordinates: TextureCoordinates::Old(vec![]),
            vertex_normals: vec![],
            vertex_colors: vec![],
            faces: vec![],
            skin_assignment_groups: vec![],
            face_material_groups: vec![],
            vertex_material_groups: vec![],
            meshops: vec![],
        }
    }
    #[test]
    fn direct_record_counts_and_canonical_null() {
        validate_record(&record(), 7).unwrap();
        for field in 0..9 {
            let mut r = record();
            match field {
                0 => r.position_count = 1,
                1 => r.texture_coordinate_count = 1,
                2 => r.normal_count = 1,
                3 => r.color_count = 1,
                4 => r.face_count = 1,
                5 => r.skin_assignment_groups_count = 1,
                6 => r.face_material_groups_count = 1,
                7 => r.vertex_material_groups_count = 1,
                8 => r.meshop_count = 1,
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    validate_record(&r, 7),
                    Err(WldCollisionError::InconsistentSource { slot: 7, .. })
                ),
                "{field}"
            );
        }
        let mut r = record();
        r.animation_ref = FragmentRef::Index(0, PhantomData);
        assert!(matches!(
            validate_record(&r, 7),
            Err(WldCollisionError::UnsupportedRequiredSource { .. })
        ));
    }
    #[test]
    fn direct_actual_payload_mismatches_and_dynamic_markers_refuse() {
        use libeq_wld::parser::{DmSpriteDef2FaceEntry, DmSpriteDef2MeshOpEntry};
        for field in 0..9 {
            let mut r = record();
            match field {
                0 => r.positions.push((0, 0, 0)),
                1 => {
                    r.texture_coordinates = TextureCoordinates::New(vec![(f32::NAN, f32::INFINITY)])
                }
                2 => r.vertex_normals.push((0, 0, 0)),
                3 => r.vertex_colors.push(0),
                4 => r.faces.push(DmSpriteDef2FaceEntry {
                    flags: 0,
                    vertex_indexes: (0, 0, 0),
                }),
                5 => r.skin_assignment_groups.push((0, 0)),
                6 => r.face_material_groups.push((0, 0)),
                7 => r.vertex_material_groups.push((0, 0)),
                8 => r.meshops.push(DmSpriteDef2MeshOpEntry {
                    index1: None,
                    index2: None,
                    offset: Some(0.),
                    param1: 0,
                    type_field: 4,
                }),
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    validate_record(&r, 7),
                    Err(WldCollisionError::InconsistentSource { .. })
                ),
                "{field}"
            );
        }
        for animation in [
            FragmentRef::new(-1),
            FragmentRef::new(1),
            FragmentRef::Index(0, PhantomData),
        ] {
            let mut r = record();
            r.animation_ref = animation;
            assert!(matches!(
                validate_record(&r, 7),
                Err(WldCollisionError::UnsupportedRequiredSource { .. })
            ));
        }
        let mut r = record();
        r.skin_assignment_groups_count = 1;
        r.skin_assignment_groups.push((0, 0));
        assert!(matches!(
            validate_record(&r, 7),
            Err(WldCollisionError::UnsupportedRequiredSource { .. })
        ));
        let mut r = record();
        r.meshop_count = 1;
        r.meshops.push(DmSpriteDef2MeshOpEntry {
            index1: None,
            index2: None,
            offset: Some(0.),
            param1: 0,
            type_field: 4,
        });
        assert!(matches!(
            validate_record(&r, 7),
            Err(WldCollisionError::UnsupportedRequiredSource { .. })
        ));
        let mut r = record();
        r.texture_coordinate_count = 1;
        r.texture_coordinates = TextureCoordinates::New(vec![(f32::NAN, f32::INFINITY)]);
        validate_record(&r, 7).unwrap();
    }
}
