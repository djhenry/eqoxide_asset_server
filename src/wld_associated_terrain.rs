//! Bounded explicit owner binding for inspected WLD Region associations.
//!
//! This strict profile rejects incomplete association coverage and shared targets. It is an
//! explicit caller binding boundary, not a full-world selector or ownership certificate.
use crate::{wld_association::*, wld_collision::*};
use libeq_wld::parser::WldDoc;

/// One caller owner for a resolved Region, in original Region order.
/// `region_slot` is a zero-based document slot, not a mesh slot or Region ordinal.
#[derive(Debug, Clone, Copy)]
pub struct WldRegionOwnerBinding<'a> {
    pub region_slot: usize,
    pub owner_id: &'a str,
}
/// Borrowed post-parse input. Owner text and opaque source context are not copied here.
pub struct WldAssociatedTerrainInput<'a> {
    pub document: &'a WldDoc,
    pub source_context: &'a str,
    pub bindings: &'a [WldRegionOwnerBinding<'a>],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WldBindingBudget {
    Bindings,
    WorkUnits,
    PayloadBytes,
    ResourceEntries,
}
/// Caller-lowerable binding-only logical limits; zero caps are valid.
///
/// For R inspected Regions and S successfully matched bindings, usage is S bindings,
/// `2 + 2*R + S` work units, `88 + 24*S` logical bytes and `2 + S` resources.
/// The fixed wrapper/usage costs 64 bytes/1 resource; the borrowed selection vector costs
/// 24 bytes/1 resource plus 24 bytes/1 resource per reserved entry. These are engineering
/// units, not Rust sizes or RSS. Owner text is borrowed; its copies remain charged by
/// the existing normalizer. Inspector/normalizer storage and work remain separately charged.
#[derive(Debug, Clone, Copy)]
pub struct WldBindingLimits {
    pub bindings: u64,
    pub work_units: u64,
    pub payload_bytes: u64,
    pub resource_entries: u64,
}
impl WldBindingLimits {
    pub const HARD: Self = Self {
        bindings: 100000,
        work_units: 300002,
        payload_bytes: 2400088,
        resource_entries: 100002,
    };
}
impl Default for WldBindingLimits {
    fn default() -> Self {
        Self::HARD
    }
}
/// Exact cumulative binding charges, including the full selection vector capacity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WldBindingUsage {
    pub bindings: u64,
    pub work_units: u64,
    pub payload_bytes: u64,
    pub resource_entries: u64,
}
/// Each nested component validates its limits before its own phase. Binding limits validate
/// first; this does not promise global validation of all nested limits before inspection.
#[derive(Debug, Clone, Copy, Default)]
pub struct WldAssociatedTerrainLimits {
    pub inspection: WldAssociationLimits,
    pub binding: WldBindingLimits,
    pub normalization: WldCollisionLimits,
}
/// Both existing results retain their own usage and scope; binding usage is separate.
#[derive(Debug)]
pub struct WldAssociatedTerrain {
    pub association_report: WldAssociationReport,
    pub normalized: NormalizedWldTerrain,
    pub binding_usage: WldBindingUsage,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WldAssociatedTerrainError {
    #[error("association inspection refused: {0}")]
    Inspection(#[from] WldAssociationError),
    #[error("selected normalization refused: {0}")]
    Normalization(#[from] WldCollisionError),
    #[error("invalid binding limit: {counter:?}")]
    InvalidBindingLimits { counter: WldBindingBudget },
    #[error("binding budget exhausted: {counter:?}, attempted {attempted}, limit {limit}")]
    BindingBudgetExceeded {
        counter: WldBindingBudget,
        attempted: u64,
        limit: u64,
    },
    #[error("binding arithmetic overflow: {counter:?}")]
    BindingArithmeticOverflow { counter: WldBindingBudget },
    #[error("binding selection reservation failed")]
    BindingReservationFailed,
    #[error("header profile refused: {gate:?}, {agreement:?}")]
    HeaderProfileRefused {
        gate: StoredRegionCountGate,
        agreement: CountAgreement,
    },
    #[error("Region profile refused at document slot {slot}")]
    RegionProfileRefused {
        slot: u64,
        body: BodyStatus,
        layout: LayoutStatus,
        legacy_coverage: LegacyCoverage,
        association: AssociationStatus,
    },
    #[error("shared modern targets cannot be represented by the selected frontend")]
    AliasedTargets {
        resolved_associations: u64,
        distinct_targets: u64,
    },
    #[error("no resolved required association")]
    EmptyAssociationSelection,
    #[error("missing bindings: expected {expected}, actual {actual}")]
    MissingBindings { expected: u64, actual: u64 },
    #[error("extra bindings: expected {expected}, actual {actual}")]
    ExtraBindings { expected: u64, actual: u64 },
    #[error("wrong Region slot at binding {binding_index}: expected {expected_slot}, actual {actual_slot}")]
    WrongRegionSlot {
        binding_index: usize,
        expected_slot: u64,
        actual_slot: usize,
    },
}
struct Budget {
    limits: WldBindingLimits,
    usage: WldBindingUsage,
}
impl Budget {
    fn charge(
        &mut self,
        counter: WldBindingBudget,
        amount: u64,
    ) -> Result<(), WldAssociatedTerrainError> {
        let (used, limit) = match counter {
            WldBindingBudget::Bindings => (&mut self.usage.bindings, self.limits.bindings),
            WldBindingBudget::WorkUnits => (&mut self.usage.work_units, self.limits.work_units),
            WldBindingBudget::PayloadBytes => {
                (&mut self.usage.payload_bytes, self.limits.payload_bytes)
            }
            WldBindingBudget::ResourceEntries => (
                &mut self.usage.resource_entries,
                self.limits.resource_entries,
            ),
        };
        let attempted = used
            .checked_add(amount)
            .ok_or(WldAssociatedTerrainError::BindingArithmeticOverflow { counter })?;
        if attempted > limit {
            return Err(WldAssociatedTerrainError::BindingBudgetExceeded {
                counter,
                attempted,
                limit,
            });
        }
        *used = attempted;
        Ok(())
    }
}
/// Inspect and bind one immutable parsed document, then delegate selected geometry preparation.
///
/// Binding slots are zero-based Region document slots in resolved Region order. Owner tokens
/// and source context are passed verbatim to the normalizer; neither ownership nor source
/// identity is verified. Tree diagnostics never filter selection. Errors expose no partial result.
/// Binding limits validate before inspection; nested limits validate before their respective
/// phases. Parsing and upstream allocation/panic caveats remain outside these limits.
pub fn normalize_associated_wld_terrain(
    input: WldAssociatedTerrainInput<'_>,
    limits: &WldAssociatedTerrainLimits,
) -> Result<WldAssociatedTerrain, WldAssociatedTerrainError> {
    use WldBindingBudget as B;
    for (counter, limit, hard) in [
        (
            B::Bindings,
            limits.binding.bindings,
            WldBindingLimits::HARD.bindings,
        ),
        (
            B::WorkUnits,
            limits.binding.work_units,
            WldBindingLimits::HARD.work_units,
        ),
        (
            B::PayloadBytes,
            limits.binding.payload_bytes,
            WldBindingLimits::HARD.payload_bytes,
        ),
        (
            B::ResourceEntries,
            limits.binding.resource_entries,
            WldBindingLimits::HARD.resource_entries,
        ),
    ] {
        if limit > hard {
            return Err(WldAssociatedTerrainError::InvalidBindingLimits { counter });
        }
    }
    let overflow = |counter| WldAssociatedTerrainError::BindingArithmeticOverflow { counter };
    let supplied = u64::try_from(input.bindings.len()).map_err(|_| overflow(B::Bindings))?;
    let mut budget = Budget {
        limits: limits.binding,
        usage: WldBindingUsage::default(),
    };
    // Only O(1) slice length/scalar work precedes the fixed binding root charges.
    budget.charge(B::Bindings, supplied)?;
    budget.charge(B::PayloadBytes, 64)?;
    budget.charge(B::ResourceEntries, 1)?;
    let association_report =
        inspect_wld_terrain_associations(input.document, limits.inspection, None)?;
    budget.charge(B::WorkUnits, 1)?;
    let header = association_report.header;
    if header.stored_region_count_gate != StoredRegionCountGate::Nonzero
        || header.region_count_agreement != CountAgreement::Equal
    {
        return Err(WldAssociatedTerrainError::HeaderProfileRefused {
            gate: header.stored_region_count_gate,
            agreement: header.region_count_agreement,
        });
    }
    for region in &association_report.regions {
        budget.charge(B::WorkUnits, 1)?;
        if region.body != BodyStatus::Typed
            || !matches!(
                region.layout,
                LayoutStatus::AcceptedByteVisibility | LayoutStatus::AcceptedWordVisibility
            )
            || region.legacy_coverage != LegacyCoverage::NoLegacyOrInlineObserved
            || !matches!(
                region.association,
                AssociationStatus::ResolvedModern { .. } | AssociationStatus::NoModernField
            )
        {
            return Err(WldAssociatedTerrainError::RegionProfileRefused {
                slot: region.slot,
                body: region.body,
                layout: region.layout,
                legacy_coverage: region.legacy_coverage,
                association: region.association,
            });
        }
    }
    budget.charge(B::WorkUnits, 1)?;
    let required = association_report.resolved_association_count;
    if required != association_report.distinct_resolved_modern_target_count {
        return Err(WldAssociatedTerrainError::AliasedTargets {
            resolved_associations: required,
            distinct_targets: association_report.distinct_resolved_modern_target_count,
        });
    }
    if required == 0 {
        return Err(WldAssociatedTerrainError::EmptyAssociationSelection);
    }
    if supplied < required {
        return Err(WldAssociatedTerrainError::MissingBindings {
            expected: required,
            actual: supplied,
        });
    }
    if supplied > required {
        return Err(WldAssociatedTerrainError::ExtraBindings {
            expected: required,
            actual: supplied,
        });
    }
    let payload = required
        .checked_mul(24)
        .and_then(|n| n.checked_add(24))
        .ok_or_else(|| overflow(B::PayloadBytes))?;
    let resources = required
        .checked_add(1)
        .ok_or_else(|| overflow(B::ResourceEntries))?;
    budget.charge(B::PayloadBytes, payload)?;
    budget.charge(B::ResourceEntries, resources)?;
    // Precharge the entire borrowed vector before its sole fallible reservation.
    let mut selections = Vec::new();
    selections
        .try_reserve_exact(input.bindings.len())
        .map_err(|_| WldAssociatedTerrainError::BindingReservationFailed)?;
    let mut binding_index = 0;
    for region in &association_report.regions {
        budget.charge(B::WorkUnits, 1)?;
        if let AssociationStatus::ResolvedModern { slot } = region.association {
            budget.charge(B::WorkUnits, 1)?;
            let binding = &input.bindings[binding_index];
            if u64::try_from(binding.region_slot).map_err(|_| overflow(B::Bindings))? != region.slot
            {
                return Err(WldAssociatedTerrainError::WrongRegionSlot {
                    binding_index,
                    expected_slot: region.slot,
                    actual_slot: binding.region_slot,
                });
            }
            let fragment_slot = usize::try_from(slot).map_err(|_| overflow(B::Bindings))?;
            selections.push(WldTerrainSelection {
                fragment_slot,
                owner_id: binding.owner_id,
            });
            binding_index += 1;
        }
    }
    let normalized = normalize_selected_wld_terrain(
        WldTerrainInput {
            document: input.document,
            source_context: input.source_context,
            selections: &selections,
        },
        &limits.normalization,
    )?;
    Ok(WldAssociatedTerrain {
        association_report,
        normalized,
        binding_usage: budget.usage,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cumulative_binding_charge_overflow_is_tagged_without_wrapping() {
        let mut b = Budget {
            limits: WldBindingLimits::HARD,
            usage: WldBindingUsage {
                work_units: u64::MAX,
                ..Default::default()
            },
        };
        assert_eq!(
            b.charge(WldBindingBudget::WorkUnits, 1),
            Err(WldAssociatedTerrainError::BindingArithmeticOverflow {
                counter: WldBindingBudget::WorkUnits
            })
        );
        assert_eq!(b.usage.work_units, u64::MAX);
    }
}
