//! Bounded, observation-only WLD Region associations.
//!
//! This API does not select geometry, certify collision availability or validate a tree graph.
//! See `docs/wld-terrain-association-contract.md` for the logical capacity/work schedule.
use libeq_wld::parser::{FragmentRef, FragmentType, Region, WldDoc, WorldTree};

pub const WLD_ASSOCIATION_SCHEMA: &str = "wld-terrain-association-observations-v1";

/// Scheduled logical counters, independent of allocator layout and prior parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WldAssociationBudget {
    DocumentSlots,
    RegionRecords,
    ModernRecords,
    TreeNodes,
    FlagValues,
    LookupAttempts,
    ResolvedAssociations,
    DocumentVisits,
    ReferenceVisits,
    HistogramComparisons,
    WorkUnits,
    PayloadBytes,
    ResourceEntries,
}
/// Fixed operation tags; errors never copy source diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WldAssociationStage {
    ValidateLimits,
    Header,
    CountDocument,
    ReserveArrays,
    InitializeTargets,
    InspectDocument,
    InspectRegion,
    Histogram,
    ResolveModern,
    FinalizeModern,
    InspectTree,
}
macro_rules! counters {
    ($( $field:ident : $tag:ident = $maximum:expr ),* $(,)?) => {
        /// Caller-lowerable limits. Zero is valid; values above HARD are rejected first.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct WldAssociationLimits { $(pub $field: u64,)* }
        impl WldAssociationLimits {
            pub const HARD: Self = Self { $($field: $maximum,)* };
            fn validate(self) -> Result<(), WldAssociationError> {
                $(if self.$field > Self::HARD.$field {
                    return Err(WldAssociationError::InvalidLimits { counter: WldAssociationBudget::$tag });
                })*
                Ok(())
            }
        }
        impl Default for WldAssociationLimits { fn default() -> Self { Self::HARD } }
        /// Exact cumulative contract charges; capacity charges include unused entries.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct WldAssociationUsage { $(pub $field: u64,)* }
        impl Budget {
            fn charge(&mut self, counter: WldAssociationBudget, amount: u64, stage: WldAssociationStage)
                -> Result<(), WldAssociationError> {
                let (used, limit) = match counter {
                    $(WldAssociationBudget::$tag => (&mut self.usage.$field, self.limits.$field),)*
                };
                let attempted = used.checked_add(amount).ok_or(WldAssociationError::ArithmeticOverflow { counter, stage })?;
                if attempted > limit { return Err(WldAssociationError::BudgetExceeded { counter, stage, attempted, limit }); }
                *used = attempted;
                Ok(())
            }
        }
    }
}
counters! {
    document_slots: DocumentSlots = 100000,
    region_records: RegionRecords = 100000,
    modern_records: ModernRecords = 100000,
    tree_nodes: TreeNodes = 100000,
    flag_values: FlagValues = 256,
    lookup_attempts: LookupAttempts = 100000,
    resolved_associations: ResolvedAssociations = 100000,
    document_visits: DocumentVisits = 300000,
    reference_visits: ReferenceVisits = 400000,
    histogram_comparisons: HistogramComparisons = 1000000,
    work_units: WorkUnits = 2000000,
    payload_bytes: PayloadBytes = 67108864,
    resource_entries: ResourceEntries = 600000,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WldAssociationError {
    #[error("invalid limit: {counter:?}")]
    InvalidLimits { counter: WldAssociationBudget },
    #[error("budget exhausted at {stage:?}: {counter:?}, attempted {attempted}, limit {limit}")]
    BudgetExceeded {
        counter: WldAssociationBudget,
        stage: WldAssociationStage,
        attempted: u64,
        limit: u64,
    },
    #[error("arithmetic overflow at {stage:?}: {counter:?}")]
    ArithmeticOverflow {
        counter: WldAssociationBudget,
        stage: WldAssociationStage,
    },
    #[error("managed reservation failed at {stage:?}: {counter:?}")]
    ReservationFailed {
        counter: WldAssociationBudget,
        stage: WldAssociationStage,
    },
    #[error("header helper did not return seven words")]
    HeaderApiInvariant,
}
struct Budget {
    limits: WldAssociationLimits,
    usage: WldAssociationUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyStatus {
    Typed,
    UnknownBody,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountAgreement {
    Equal,
    Different,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredRegionCountGate {
    Zero,
    Nonzero,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutStatus {
    UnavailableBody,
    UnsupportedLayout,
    AcceptedByteVisibility,
    AcceptedWordVisibility,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyCoverage {
    Unavailable,
    LegacyOrInlinePresent,
    NoLegacyOrInlineObserved,
}
/// Raw wrappers preserve their variant and value, including invalid in-memory values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawReference {
    Name(i32),
    Index(u32),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedVariant {
    DmSpriteDef,
    AmbientLight,
    BlitSpriteDef,
    BlitSprite,
    Region,
    WorldTree,
    Sprite3DDef,
    Sprite3D,
    GlobalAmbientLightDef,
    Sprite4D,
    Sprite4DDef,
    PointLight,
    LightDef,
    Light,
    MaterialDef,
    MaterialPalette,
    DmSpriteDef2,
    DmTrackDef2,
    DmTrack,
    DmSprite,
    TrackDef,
    Track,
    ActorDef,
    Actor,
    ParticleSprite,
    ParticleSpriteDef,
    ParticleCloudDef,
    DefaultPaletteFile,
    PolyhedronDef,
    Polyhedron,
    Zone,
    HierarchicalSpriteDef,
    HierarchicalSprite,
    SphereList,
    SphereListDef,
    SimpleSpriteDef,
    BmInfo,
    BmInfoRtk,
    SimpleSprite,
    Sprite2DDef,
    Sprite2D,
    DmTrackDef,
    DmRGBTrackDef,
    DmRGBTrack,
    WorldVertices,
    Sphere,
    DirectionalLight,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetBody {
    UnknownBody,
    Typed(TypedVariant),
}
/// Exactly one observation per Region. Only ResolvedModern contributes an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssociationStatus {
    UnavailableBody,
    UnsupportedLayout,
    InconsistentOptionality,
    NoModernField,
    NullRequiredReference,
    NamedUnresolved,
    InvalidReferenceRepresentation,
    MissingTarget {
        slot: u64,
    },
    ResolvedModern {
        slot: u64,
    },
    UnknownModernTarget {
        slot: u64,
    },
    OlderGeometryTarget {
        slot: u64,
    },
    WrongKindTarget {
        slot: u64,
        original_kind: u32,
        body: TargetBody,
    },
}
/// Eight declared counts and seven accessible lengths, with no pool traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionPools {
    /// Region vertices, proximal Regions, render vertices, walls, obstacles,
    /// cutting obstacles, visibility nodes, visibility lists.
    pub declared: [u32; 8],
    /// Region vertices, proximal Regions, render vertices, walls, obstacles,
    /// visibility nodes, visibility lists. Cutting obstacles have no exposed pool.
    pub lengths: [u64; 7],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionObservation {
    pub slot: u64,
    pub ordinal: u64,
    pub body: BodyStatus,
    pub flags: Option<u32>,
    pub layout: LayoutStatus,
    pub pools: Option<RegionPools>,
    pub legacy_coverage: LegacyCoverage,
    pub association: AssociationStatus,
    /// Unavailable/unsupported layouts do not expose a purported reference.
    pub reference: Option<RawReference>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModernObservation {
    pub slot: u64,
    pub body: BodyStatus,
    pub resolved_association_count: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagCount {
    pub flags: u32,
    pub count: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrdinalStatus {
    Absent,
    InvalidReferenceRepresentation,
    InvalidOrdinalDomain,
    OutOfRange,
    InRangeNode,
    InRangeRegion { slot: u64, body: BodyStatus },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrdinalObservation {
    pub reference: RawReference,
    pub status: OrdinalStatus,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeObservation {
    pub ordinal: u64,
    pub front: OrdinalObservation,
    pub back: OrdinalObservation,
    pub region: OrdinalObservation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeStatus {
    Missing,
    UnknownBody,
    Typed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeObservation {
    pub original_tree_count: u64,
    pub selected_slot: Option<u64>,
    pub status: TreeStatus,
    pub declared_node_count: Option<u32>,
    pub actual_node_count: Option<u64>,
    pub node_count_agreement: Option<CountAgreement>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphAnalysis {
    NotPerformed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderObservation {
    pub stored_words: [u32; 7],
    pub actual_document_count: u64,
    pub actual_region_count: u64,
    pub stored_region_count_gate: StoredRegionCountGate,
    pub region_count_agreement: CountAgreement,
}
/// Independent coverage counters; none certifies source completeness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssociationScope {
    pub accepted_layout_count: u64,
    pub unsupported_layout_count: u64,
    pub unavailable_layout_count: u64,
    pub no_modern_field_count: u64,
    pub unresolved_region_association_count: u64,
    pub unavailable_body_association_count: u64,
    pub unsupported_layout_association_count: u64,
    pub inconsistent_optionality_count: u64,
    pub null_required_reference_count: u64,
    pub named_unresolved_count: u64,
    pub invalid_reference_representation_count: u64,
    pub missing_target_count: u64,
    pub unknown_modern_target_count: u64,
    pub older_geometry_target_count: u64,
    pub wrong_kind_target_count: u64,
    pub legacy_or_inline_count: u64,
    pub no_legacy_or_inline_observed_count: u64,
    pub unavailable_legacy_count: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub struct WldAssociationReport {
    pub schema: &'static str,
    /// Unverified opaque caller label; this API does not verify a content hash.
    pub context: Option<[u8; 32]>,
    pub header: HeaderObservation,
    pub regions: Vec<RegionObservation>,
    pub flag_histogram: Vec<FlagCount>,
    pub unavailable_flags_count: u64,
    pub modern_records: Vec<ModernObservation>,
    pub tree: TreeObservation,
    pub nodes: Vec<NodeObservation>,
    pub scope: AssociationScope,
    pub graph_analysis: GraphAnalysis,
    pub resolved_association_count: u64,
    pub distinct_resolved_modern_target_count: u64,
    pub typed_modern_zero_observed_association_count: u64,
    pub histogram_capacity: u64,
    pub usage: WldAssociationUsage,
}
/// Inspect one immutable parsed document with atomic refusal on a scheduled budget error.
/// Upstream parsing is outside these limits. The fixed header helper allocates infallibly;
/// allocation aborts there, global OOM and arbitrary upstream panics are not recoverable here.
pub fn inspect_wld_terrain_associations(
    document: &WldDoc,
    limits: WldAssociationLimits,
    context: Option<[u8; 32]>,
) -> Result<WldAssociationReport, WldAssociationError> {
    use WldAssociationBudget as B;
    use WldAssociationStage as S;
    limits.validate()?;
    let mut budget = Budget {
        limits,
        usage: WldAssociationUsage::default(),
    };
    budget.charge(B::PayloadBytes, 512, S::Header)?;
    budget.charge(B::ResourceEntries, 1, S::Header)?;
    budget.charge(B::PayloadBytes, 24, S::Header)?;
    budget.charge(B::ResourceEntries, 1, S::Header)?;
    budget.charge(B::PayloadBytes, 28, S::Header)?;
    // This pinned helper's fixed allocation is infallible upstream; see API caveat.
    let header_bytes = document.header_bytes();
    if header_bytes.len() != 28 {
        return Err(WldAssociationError::HeaderApiInvariant);
    }
    let mut stored_words = [0; 7];
    for (word, bytes) in stored_words.iter_mut().zip(header_bytes.as_chunks::<4>().0) {
        budget.charge(B::WorkUnits, 1, S::Header)?;
        *word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    let d = document.fragment_count() as u64;
    budget.charge(B::DocumentSlots, d, S::CountDocument)?;
    let mut tree_count = 0u64;
    let mut selected_tree = None;
    for (slot, fragment) in document.iter().enumerate() {
        budget.document_visit(S::CountDocument)?;
        match fragment.type_id() {
            0x22 => budget.charge(B::RegionRecords, 1, S::CountDocument)?,
            0x36 => budget.charge(B::ModernRecords, 1, S::CountDocument)?,
            0x21 => {
                tree_count = increment(tree_count, B::DocumentSlots, S::CountDocument)?;
                selected_tree = Some((slot as u64, fragment));
            }
            _ => {}
        }
    }
    let r = budget.usage.region_records;
    let m = budget.usage.modern_records;
    let typed_tree = selected_tree.and_then(|(_, fragment)| match fragment {
        FragmentType::WorldTree(tree) => Some(tree),
        _ => None,
    });
    let n = typed_tree.map_or(0, |tree| tree.world_nodes.len() as u64);
    budget.charge(B::TreeNodes, n, S::CountDocument)?;
    let q = r.min(256);
    budget.charge(B::PayloadBytes, 5 * 24, S::ReserveArrays)?;
    budget.charge(B::ResourceEntries, 5, S::ReserveArrays)?;
    for (count, bytes) in [(r, 256), (m, 32), (n, 96), (q, 16), (d, 8)] {
        let payload = count
            .checked_mul(bytes)
            .ok_or(WldAssociationError::ArithmeticOverflow {
                counter: B::PayloadBytes,
                stage: S::ReserveArrays,
            })?;
        budget.charge(B::PayloadBytes, payload, S::ReserveArrays)?;
        budget.charge(B::ResourceEntries, count, S::ReserveArrays)?;
    }
    let mut regions = reserve(r, B::RegionRecords)?;
    let mut modern_records = reserve(m, B::ModernRecords)?;
    let mut nodes = reserve(n, B::TreeNodes)?;
    let mut flag_histogram: Vec<FlagCount> = reserve(q, B::FlagValues)?;
    let mut targets: Vec<u64> = reserve(d, B::DocumentSlots)?;
    budget.charge(B::WorkUnits, d, S::InitializeTargets)?;
    targets.resize(document.fragment_count(), 0);
    let mut scope = AssociationScope::default();
    let mut unavailable_flags_count = 0u64;
    for (slot, fragment) in document.iter().enumerate() {
        budget.document_visit(S::InspectDocument)?;
        if fragment.type_id() == 0x22 {
            budget.charge(B::WorkUnits, 1, S::InspectRegion)?;
            budget.reference_visit(S::InspectRegion)?;
            let ordinal = increment(regions.len() as u64, B::RegionRecords, S::InspectRegion)?;
            let mut observation = RegionObservation {
                slot: slot as u64,
                ordinal,
                body: BodyStatus::UnknownBody,
                flags: None,
                layout: LayoutStatus::UnavailableBody,
                pools: None,
                legacy_coverage: LegacyCoverage::Unavailable,
                association: AssociationStatus::UnavailableBody,
                reference: None,
            };
            if let FragmentType::Region(region) = fragment {
                let flags = u32::from(region.flags);
                let mut found = false;
                for entry in &mut flag_histogram {
                    budget.charge(B::HistogramComparisons, 1, S::Histogram)?;
                    budget.charge(B::WorkUnits, 1, S::Histogram)?;
                    if entry.flags == flags {
                        entry.count = increment(entry.count, B::RegionRecords, S::Histogram)?;
                        found = true;
                        break;
                    }
                }
                if !found {
                    budget.charge(B::FlagValues, 1, S::Histogram)?;
                    flag_histogram.push(FlagCount { flags, count: 1 });
                }
                observation.body = BodyStatus::Typed;
                observation.flags = Some(flags);
                let pools = region_pools(region);
                observation.legacy_coverage = legacy_coverage(pools);
                observation.pools = Some(pools);
                let (layout, initial_status, reference) = region_layout(region);
                observation.layout = layout;
                observation.reference = reference;
                observation.association = match initial_status {
                    Some(status) => status,
                    None => resolve_modern(
                        reference.expect("accepted required field"),
                        document,
                        &mut targets,
                        &mut budget,
                    )?,
                };
            } else {
                unavailable_flags_count =
                    increment(unavailable_flags_count, B::RegionRecords, S::InspectRegion)?;
            }
            scope.observe(&observation)?;
            regions.push(observation);
        }
        if fragment.type_id() == 0x36 {
            modern_records.push(ModernObservation {
                slot: slot as u64,
                body: if matches!(fragment, FragmentType::DmSpriteDef2(_)) {
                    BodyStatus::Typed
                } else {
                    BodyStatus::UnknownBody
                },
                resolved_association_count: 0,
            });
        }
    }
    let mut distinct = 0u64;
    let mut zero_observed = 0u64;
    for modern in &mut modern_records {
        budget.charge(B::WorkUnits, 1, S::FinalizeModern)?;
        modern.resolved_association_count = targets[modern.slot as usize];
        if modern.body == BodyStatus::Typed {
            if modern.resolved_association_count == 0 {
                zero_observed = increment(zero_observed, B::ModernRecords, S::FinalizeModern)?;
            } else {
                distinct = increment(distinct, B::ModernRecords, S::FinalizeModern)?;
            }
        }
    }
    if let Some(tree) = typed_tree {
        for (index, node) in tree.world_nodes.iter().enumerate() {
            budget.charge(B::WorkUnits, 1, S::InspectTree)?;
            let front = classify_ordinal(&node.front_tree, n, None, &mut budget)?;
            let back = classify_ordinal(&node.back_tree, n, None, &mut budget)?;
            let region = classify_ordinal(&node.region, r, Some(&regions), &mut budget)?;
            nodes.push(NodeObservation {
                ordinal: increment(index as u64, B::TreeNodes, S::InspectTree)?,
                front,
                back,
                region,
            });
        }
    }
    let tree = tree_observation(tree_count, selected_tree.map(|(slot, _)| slot), typed_tree);
    Ok(WldAssociationReport {
        schema: WLD_ASSOCIATION_SCHEMA,
        context,
        header: HeaderObservation {
            stored_words,
            actual_document_count: d,
            actual_region_count: r,
            stored_region_count_gate: if stored_words[3] == 0 {
                StoredRegionCountGate::Zero
            } else {
                StoredRegionCountGate::Nonzero
            },
            region_count_agreement: agreement(u64::from(stored_words[3]), r),
        },
        regions,
        flag_histogram,
        unavailable_flags_count,
        modern_records,
        tree,
        nodes,
        scope,
        graph_analysis: GraphAnalysis::NotPerformed,
        resolved_association_count: budget.usage.resolved_associations,
        distinct_resolved_modern_target_count: distinct,
        typed_modern_zero_observed_association_count: zero_observed,
        histogram_capacity: q,
        usage: budget.usage,
    })
}

fn increment(
    value: u64,
    counter: WldAssociationBudget,
    stage: WldAssociationStage,
) -> Result<u64, WldAssociationError> {
    value
        .checked_add(1)
        .ok_or(WldAssociationError::ArithmeticOverflow { counter, stage })
}
fn reserve<T>(count: u64, counter: WldAssociationBudget) -> Result<Vec<T>, WldAssociationError> {
    let stage = WldAssociationStage::ReserveArrays;
    let capacity = usize::try_from(count)
        .map_err(|_| WldAssociationError::ArithmeticOverflow { counter, stage })?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(capacity)
        .map_err(|_| WldAssociationError::ReservationFailed { counter, stage })?;
    Ok(result)
}
impl Budget {
    fn document_visit(&mut self, stage: WldAssociationStage) -> Result<(), WldAssociationError> {
        self.charge(WldAssociationBudget::DocumentVisits, 1, stage)?;
        self.charge(WldAssociationBudget::WorkUnits, 1, stage)
    }
    fn reference_visit(&mut self, stage: WldAssociationStage) -> Result<(), WldAssociationError> {
        self.charge(WldAssociationBudget::ReferenceVisits, 1, stage)?;
        self.charge(WldAssociationBudget::WorkUnits, 1, stage)
    }
}
fn agreement(left: u64, right: u64) -> CountAgreement {
    if left == right {
        CountAgreement::Equal
    } else {
        CountAgreement::Different
    }
}
fn raw_reference<T>(reference: &FragmentRef<T>) -> RawReference {
    match reference {
        FragmentRef::Name(value, _) => RawReference::Name(value.0),
        FragmentRef::Index(value, _) => RawReference::Index(*value),
    }
}
fn region_pools(region: &Region) -> RegionPools {
    RegionPools {
        declared: [
            region.num_region_vertex,
            region.num_proximal_regions,
            region.num_render_vertices,
            region.num_walls,
            region.num_obstacles,
            region.num_cutting_obstacles,
            region.num_vis_node,
            region.num_vis_list,
        ],
        lengths: [
            region.region_vertices.len() as u64,
            region.proximal_regions.len() as u64,
            region.render_vertices.len() as u64,
            region.walls.len() as u64,
            region.obstacles.len() as u64,
            region.vis_nodes.len() as u64,
            region.visible_lists.len() as u64,
        ],
    }
}
fn legacy_coverage(pools: RegionPools) -> LegacyCoverage {
    if [0, 2, 3, 4, 5]
        .iter()
        .any(|&index| pools.declared[index] != 0)
        || [0, 2, 3, 4].iter().any(|&index| pools.lengths[index] != 0)
    {
        LegacyCoverage::LegacyOrInlinePresent
    } else {
        LegacyCoverage::NoLegacyOrInlineObserved
    }
}
fn region_layout(
    region: &Region,
) -> (
    LayoutStatus,
    Option<AssociationStatus>,
    Option<RawReference>,
) {
    let flags = u32::from(region.flags);
    let layout = match flags {
        0x81 | 0x181 => LayoutStatus::AcceptedByteVisibility,
        0x101 => LayoutStatus::AcceptedWordVisibility,
        _ => {
            return (
                LayoutStatus::UnsupportedLayout,
                Some(AssociationStatus::UnsupportedLayout),
                None,
            )
        }
    };
    let has_modern = flags & 0x100 != 0;
    let reference = region.mesh_reference.as_ref().map(raw_reference);
    if region.sphere.is_none()
        || region.reverb_volume.is_some()
        || region.reverb_offset.is_some()
        || region.mesh_reference.is_some() != has_modern
    {
        return (
            layout,
            Some(AssociationStatus::InconsistentOptionality),
            reference,
        );
    }
    if !has_modern {
        return (layout, Some(AssociationStatus::NoModernField), None);
    }
    (layout, None, reference)
}
fn resolve_modern(
    reference: RawReference,
    document: &WldDoc,
    targets: &mut [u64],
    budget: &mut Budget,
) -> Result<AssociationStatus, WldAssociationError> {
    use WldAssociationBudget as B;
    use WldAssociationStage as S;
    let index = match reference {
        RawReference::Name(0) => return Ok(AssociationStatus::NullRequiredReference),
        RawReference::Name(value) if value < 0 => return Ok(AssociationStatus::NamedUnresolved),
        RawReference::Index(value) if value > 0 && value <= i32::MAX as u32 => value,
        _ => return Ok(AssociationStatus::InvalidReferenceRepresentation),
    };
    let slot = index
        .checked_sub(1)
        .ok_or(WldAssociationError::ArithmeticOverflow {
            counter: B::DocumentSlots,
            stage: S::ResolveModern,
        })?;
    budget.charge(B::LookupAttempts, 1, S::ResolveModern)?;
    budget.document_visit(S::ResolveModern)?;
    let slot = slot as usize;
    Ok(match document.at(slot) {
        None => AssociationStatus::MissingTarget { slot: slot as u64 },
        Some(FragmentType::DmSpriteDef2(_)) => {
            budget.charge(B::ResolvedAssociations, 1, S::ResolveModern)?;
            budget.charge(B::WorkUnits, 1, S::ResolveModern)?;
            // A successful document lookup bounds this scratch access by D.
            let count = targets
                .get_mut(slot)
                .expect("document slot bounds target scratch");
            *count = increment(*count, B::ResolvedAssociations, S::ResolveModern)?;
            AssociationStatus::ResolvedModern { slot: slot as u64 }
        }
        Some(FragmentType::Unknown(fragment)) if fragment.type_id == 0x36 => {
            AssociationStatus::UnknownModernTarget { slot: slot as u64 }
        }
        Some(FragmentType::DmSpriteDef(_)) => {
            AssociationStatus::OlderGeometryTarget { slot: slot as u64 }
        }
        Some(fragment) => AssociationStatus::WrongKindTarget {
            slot: slot as u64,
            original_kind: fragment.type_id(),
            body: target_body(fragment),
        },
    })
}
fn target_body(fragment: &FragmentType) -> TargetBody {
    TargetBody::Typed(match fragment {
        FragmentType::Unknown(_) => return TargetBody::UnknownBody,
        FragmentType::DmSpriteDef(_) => TypedVariant::DmSpriteDef,
        FragmentType::AmbientLight(_) => TypedVariant::AmbientLight,
        FragmentType::BlitSpriteDef(_) => TypedVariant::BlitSpriteDef,
        FragmentType::BlitSprite(_) => TypedVariant::BlitSprite,
        FragmentType::Region(_) => TypedVariant::Region,
        FragmentType::WorldTree(_) => TypedVariant::WorldTree,
        FragmentType::Sprite3DDef(_) => TypedVariant::Sprite3DDef,
        FragmentType::Sprite3D(_) => TypedVariant::Sprite3D,
        FragmentType::GlobalAmbientLightDef(_) => TypedVariant::GlobalAmbientLightDef,
        FragmentType::Sprite4D(_) => TypedVariant::Sprite4D,
        FragmentType::Sprite4DDef(_) => TypedVariant::Sprite4DDef,
        FragmentType::PointLight(_) => TypedVariant::PointLight,
        FragmentType::LightDef(_) => TypedVariant::LightDef,
        FragmentType::Light(_) => TypedVariant::Light,
        FragmentType::MaterialDef(_) => TypedVariant::MaterialDef,
        FragmentType::MaterialPalette(_) => TypedVariant::MaterialPalette,
        FragmentType::DmSpriteDef2(_) => TypedVariant::DmSpriteDef2,
        FragmentType::DmTrackDef2(_) => TypedVariant::DmTrackDef2,
        FragmentType::DmTrack(_) => TypedVariant::DmTrack,
        FragmentType::DmSprite(_) => TypedVariant::DmSprite,
        FragmentType::TrackDef(_) => TypedVariant::TrackDef,
        FragmentType::Track(_) => TypedVariant::Track,
        FragmentType::ActorDef(_) => TypedVariant::ActorDef,
        FragmentType::Actor(_) => TypedVariant::Actor,
        FragmentType::ParticleSprite(_) => TypedVariant::ParticleSprite,
        FragmentType::ParticleSpriteDef(_) => TypedVariant::ParticleSpriteDef,
        FragmentType::ParticleCloudDef(_) => TypedVariant::ParticleCloudDef,
        FragmentType::DefaultPaletteFile(_) => TypedVariant::DefaultPaletteFile,
        FragmentType::PolyhedronDef(_) => TypedVariant::PolyhedronDef,
        FragmentType::Polyhedron(_) => TypedVariant::Polyhedron,
        FragmentType::Zone(_) => TypedVariant::Zone,
        FragmentType::HierarchicalSpriteDef(_) => TypedVariant::HierarchicalSpriteDef,
        FragmentType::HierarchicalSprite(_) => TypedVariant::HierarchicalSprite,
        FragmentType::SphereList(_) => TypedVariant::SphereList,
        FragmentType::SphereListDef(_) => TypedVariant::SphereListDef,
        FragmentType::SimpleSpriteDef(_) => TypedVariant::SimpleSpriteDef,
        FragmentType::BmInfo(_) => TypedVariant::BmInfo,
        FragmentType::BmInfoRtk(_) => TypedVariant::BmInfoRtk,
        FragmentType::SimpleSprite(_) => TypedVariant::SimpleSprite,
        FragmentType::Sprite2DDef(_) => TypedVariant::Sprite2DDef,
        FragmentType::Sprite2D(_) => TypedVariant::Sprite2D,
        FragmentType::DmTrackDef(_) => TypedVariant::DmTrackDef,
        FragmentType::DmRGBTrackDef(_) => TypedVariant::DmRGBTrackDef,
        FragmentType::DmRGBTrack(_) => TypedVariant::DmRGBTrack,
        FragmentType::WorldVertices(_) => TypedVariant::WorldVertices,
        FragmentType::Sphere(_) => TypedVariant::Sphere,
        FragmentType::DirectionalLight(_) => TypedVariant::DirectionalLight,
    })
}
fn classify_ordinal<T>(
    reference: &FragmentRef<T>,
    bound: u64,
    regions: Option<&[RegionObservation]>,
    budget: &mut Budget,
) -> Result<OrdinalObservation, WldAssociationError> {
    budget.reference_visit(WldAssociationStage::InspectTree)?;
    let reference = raw_reference(reference);
    let status = match reference {
        RawReference::Name(0) => OrdinalStatus::Absent,
        RawReference::Name(value) if value < 0 => OrdinalStatus::InvalidOrdinalDomain,
        RawReference::Index(value) if value > 0 && value <= i32::MAX as u32 => {
            if u64::from(value) > bound {
                OrdinalStatus::OutOfRange
            } else if let Some(regions) = regions {
                let target = regions
                    .get((value - 1) as usize)
                    .expect("actual Region ordinal bound");
                OrdinalStatus::InRangeRegion {
                    slot: target.slot,
                    body: target.body,
                }
            } else {
                OrdinalStatus::InRangeNode
            }
        }
        _ => OrdinalStatus::InvalidReferenceRepresentation,
    };
    Ok(OrdinalObservation { reference, status })
}
impl AssociationScope {
    fn observe(&mut self, region: &RegionObservation) -> Result<(), WldAssociationError> {
        use WldAssociationBudget as B;
        use WldAssociationStage as S;
        let count = match region.layout {
            LayoutStatus::UnavailableBody => &mut self.unavailable_layout_count,
            LayoutStatus::UnsupportedLayout => &mut self.unsupported_layout_count,
            _ => &mut self.accepted_layout_count,
        };
        *count = increment(*count, B::RegionRecords, S::InspectRegion)?;
        let count = match region.legacy_coverage {
            LegacyCoverage::Unavailable => &mut self.unavailable_legacy_count,
            LegacyCoverage::LegacyOrInlinePresent => &mut self.legacy_or_inline_count,
            LegacyCoverage::NoLegacyOrInlineObserved => {
                &mut self.no_legacy_or_inline_observed_count
            }
        };
        *count = increment(*count, B::RegionRecords, S::InspectRegion)?;
        match region.association {
            AssociationStatus::ResolvedModern { .. } => {}
            AssociationStatus::NoModernField => {
                self.no_modern_field_count = increment(
                    self.no_modern_field_count,
                    B::RegionRecords,
                    S::InspectRegion,
                )?
            }
            status => {
                self.unresolved_region_association_count = increment(
                    self.unresolved_region_association_count,
                    B::RegionRecords,
                    S::InspectRegion,
                )?;
                let count = match status {
                    AssociationStatus::UnavailableBody => {
                        &mut self.unavailable_body_association_count
                    }
                    AssociationStatus::UnsupportedLayout => {
                        &mut self.unsupported_layout_association_count
                    }
                    AssociationStatus::InconsistentOptionality => {
                        &mut self.inconsistent_optionality_count
                    }
                    AssociationStatus::NullRequiredReference => {
                        &mut self.null_required_reference_count
                    }
                    AssociationStatus::NamedUnresolved => &mut self.named_unresolved_count,
                    AssociationStatus::InvalidReferenceRepresentation => {
                        &mut self.invalid_reference_representation_count
                    }
                    AssociationStatus::MissingTarget { .. } => &mut self.missing_target_count,
                    AssociationStatus::UnknownModernTarget { .. } => {
                        &mut self.unknown_modern_target_count
                    }
                    AssociationStatus::OlderGeometryTarget { .. } => {
                        &mut self.older_geometry_target_count
                    }
                    AssociationStatus::WrongKindTarget { .. } => &mut self.wrong_kind_target_count,
                    AssociationStatus::ResolvedModern { .. } | AssociationStatus::NoModernField => {
                        unreachable!("handled association classes")
                    }
                };
                *count = increment(*count, B::RegionRecords, S::InspectRegion)?;
            }
        }
        Ok(())
    }
}

fn tree_observation(count: u64, slot: Option<u64>, tree: Option<&WorldTree>) -> TreeObservation {
    TreeObservation {
        original_tree_count: count,
        selected_slot: slot,
        status: if tree.is_some() {
            TreeStatus::Typed
        } else if slot.is_some() {
            TreeStatus::UnknownBody
        } else {
            TreeStatus::Missing
        },
        declared_node_count: tree.map(|tree| tree.world_node_count),
        actual_node_count: tree.map(|tree| tree.world_nodes.len() as u64),
        node_count_agreement: tree.map(|tree| {
            agreement(
                u64::from(tree.world_node_count),
                tree.world_nodes.len() as u64,
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libeq_wld::parser::{FragmentParser, StringReference};
    use std::marker::PhantomData;

    fn budget() -> Budget {
        Budget {
            limits: WldAssociationLimits::default(),
            usage: WldAssociationUsage::default(),
        }
    }
    fn region(flags: u32) -> Region {
        let mut words = vec![0, flags, 0];
        words.extend([0; 8]);
        words.extend([0; 4]);
        words.push(0);
        if flags & 0x100 != 0 {
            words.push(1);
        }
        let bytes: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        Region::parse(&bytes).unwrap().1
    }
    fn empty_document() -> WldDoc {
        let bytes: Vec<u8> = [0x54503d02u32, 0x15500, 0, 0, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        WldDoc::parse(&bytes).unwrap()
    }
    #[test]
    fn mutated_optionality_is_observed_before_any_resolution() {
        for change in 0..4 {
            let mut region = region(0x181);
            match change {
                0 => region.sphere = None,
                1 => region.reverb_volume = Some(0.),
                2 => region.reverb_offset = Some(0),
                _ => region.mesh_reference = None,
            }
            let (_, status, reference) = region_layout(&region);
            assert_eq!(status, Some(AssociationStatus::InconsistentOptionality));
            assert_eq!(
                reference,
                if change == 3 {
                    None
                } else {
                    Some(RawReference::Index(1))
                }
            );
        }
        let mut region = region(0x81);
        region.mesh_reference = Some(FragmentRef::new(1));
        assert_eq!(
            region_layout(&region),
            (
                LayoutStatus::AcceptedByteVisibility,
                Some(AssociationStatus::InconsistentOptionality),
                Some(RawReference::Index(1))
            )
        );
    }
    #[test]
    fn mutated_wrapper_representations_are_not_document_or_name_lookups() {
        let document = empty_document();
        let mut budget = budget();
        for reference in [
            RawReference::Index(0),
            RawReference::Index(i32::MAX as u32 + 1),
            RawReference::Index(u32::MAX),
            RawReference::Name(1),
            RawReference::Name(i32::MAX),
        ] {
            assert_eq!(
                resolve_modern(reference, &document, &mut [], &mut budget).unwrap(),
                AssociationStatus::InvalidReferenceRepresentation
            );
        }
        assert_eq!(budget.usage, WldAssociationUsage::default());
        let index: FragmentRef<Region> = FragmentRef::Index(0, PhantomData);
        let name: FragmentRef<Region> = FragmentRef::Name(StringReference::new(3), PhantomData);
        assert_eq!(raw_reference(&index), RawReference::Index(0));
        assert_eq!(raw_reference(&name), RawReference::Name(3));
    }
    #[test]
    fn mutated_ordinals_preserve_invalid_domains_and_original_region_slots() {
        let region = RegionObservation {
            slot: 5,
            ordinal: 1,
            body: BodyStatus::UnknownBody,
            flags: None,
            layout: LayoutStatus::UnavailableBody,
            pools: None,
            legacy_coverage: LegacyCoverage::Unavailable,
            association: AssociationStatus::UnavailableBody,
            reference: None,
        };
        let regions = [
            region,
            RegionObservation {
                slot: 11,
                ordinal: 2,
                body: BodyStatus::Typed,
                ..region
            },
        ];
        let mut budget = budget();
        for (reference, expected) in [
            (RawReference::Name(0), OrdinalStatus::Absent),
            (
                RawReference::Name(i32::MIN),
                OrdinalStatus::InvalidOrdinalDomain,
            ),
            (RawReference::Name(-1), OrdinalStatus::InvalidOrdinalDomain),
            (
                RawReference::Name(3),
                OrdinalStatus::InvalidReferenceRepresentation,
            ),
            (
                RawReference::Index(0),
                OrdinalStatus::InvalidReferenceRepresentation,
            ),
            (
                RawReference::Index(u32::MAX),
                OrdinalStatus::InvalidReferenceRepresentation,
            ),
            (RawReference::Index(3), OrdinalStatus::OutOfRange),
            (
                RawReference::Index(1),
                OrdinalStatus::InRangeRegion {
                    slot: 5,
                    body: BodyStatus::UnknownBody,
                },
            ),
            (
                RawReference::Index(2),
                OrdinalStatus::InRangeRegion {
                    slot: 11,
                    body: BodyStatus::Typed,
                },
            ),
        ] {
            let source: FragmentRef<Region> = match reference {
                RawReference::Index(value) => FragmentRef::Index(value, PhantomData),
                RawReference::Name(value) => {
                    FragmentRef::Name(StringReference::new(value), PhantomData)
                }
            };
            assert_eq!(
                classify_ordinal(&source, 2, Some(&regions), &mut budget).unwrap(),
                OrdinalObservation {
                    reference,
                    status: expected
                }
            );
        }
        assert_eq!(budget.usage.reference_visits, 9);
        assert_eq!(budget.usage.work_units, 9);
        assert_eq!(budget.usage.document_visits, 0);
    }
    #[test]
    fn mutated_declared_counts_and_pools_are_observations_not_equality_gates() {
        let mut region = region(0x181);
        region.num_region_vertex = 7;
        region.num_proximal_regions = 9;
        region.num_render_vertices = 8;
        region.num_walls = 2;
        region.num_obstacles = 3;
        region.num_cutting_obstacles = 4;
        region.num_vis_node = 5;
        region.num_vis_list = 6;
        let pools = region_pools(&region);
        assert_eq!(
            pools,
            RegionPools {
                declared: [7, 9, 8, 2, 3, 4, 5, 6],
                lengths: [0; 7]
            }
        );
        assert_eq!(
            legacy_coverage(pools),
            LegacyCoverage::LegacyOrInlinePresent
        );
        assert_eq!(region_layout(&region).1, None);
        for index in [0, 2, 3, 4, 5] {
            let mut pools = RegionPools {
                declared: [0; 8],
                lengths: [0; 7],
            };
            pools.declared[index] = 1;
            assert_eq!(
                legacy_coverage(pools),
                LegacyCoverage::LegacyOrInlinePresent
            );
        }
        for index in [0, 2, 3, 4] {
            let mut pools = RegionPools {
                declared: [0; 8],
                lengths: [0; 7],
            };
            pools.lengths[index] = 1;
            assert_eq!(
                legacy_coverage(pools),
                LegacyCoverage::LegacyOrInlinePresent
            );
        }
        assert_eq!(
            legacy_coverage(RegionPools {
                declared: [0, 1, 0, 0, 0, 0, 1, 1],
                lengths: [0, 1, 0, 0, 0, 1, 1]
            }),
            LegacyCoverage::NoLegacyOrInlineObserved
        );
        let mut tree = WorldTree::parse(&[0; 8]).unwrap().1;
        tree.world_node_count = 30;
        assert_eq!(
            tree_observation(2, Some(8), Some(&tree)),
            TreeObservation {
                original_tree_count: 2,
                selected_slot: Some(8),
                status: TreeStatus::Typed,
                declared_node_count: Some(30),
                actual_node_count: Some(0),
                node_count_agreement: Some(CountAgreement::Different)
            }
        );
    }
    #[test]
    fn arithmetic_and_managed_reservation_failures_are_tagged() {
        let mut budget = budget();
        budget.usage.work_units = u64::MAX;
        assert_eq!(
            budget.charge(
                WldAssociationBudget::WorkUnits,
                1,
                WldAssociationStage::InspectTree
            ),
            Err(WldAssociationError::ArithmeticOverflow {
                counter: WldAssociationBudget::WorkUnits,
                stage: WldAssociationStage::InspectTree
            })
        );
        assert_eq!(
            increment(
                u64::MAX,
                WldAssociationBudget::ResolvedAssociations,
                WldAssociationStage::ResolveModern
            ),
            Err(WldAssociationError::ArithmeticOverflow {
                counter: WldAssociationBudget::ResolvedAssociations,
                stage: WldAssociationStage::ResolveModern
            })
        );
        assert!(matches!(
            reserve::<u64>(u64::MAX, WldAssociationBudget::DocumentSlots),
            Err(WldAssociationError::ReservationFailed { .. })
                | Err(WldAssociationError::ArithmeticOverflow { .. })
        ));
    }
}
