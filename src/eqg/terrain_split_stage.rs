//! Conditional, value-only split preparation for four q32 views.
//!
//! This composes leaf seeding, internal refinement and prepared-marker updates.
//! It does not select a source/bake profile, establish cache reachability, run
//! dependency propagation, emit a surface or enable a bake/query route. The
//! arithmetic model is conditional on 53-bit rounding toward zero, not proof
//! that this mode is active at every native terrain entry.

use super::{
    terrain_arithmetic::{deviation_exceeds_53rz, NonFiniteOperand},
    terrain_dependencies::Q32SplitStates,
    terrain_hierarchy::Q32Hierarchy,
    terrain_leaf_states::{seed_if_unprepared, Q32QuadFlags},
    terrain_neighbors::Q32Neighbors,
};

/// Caller-supplied working heights indexed by `[y][x]`, before world scaling.
pub type Q32HeightGrid = [[f32; 33]; 33];

/// Exact override sentinel selecting the global tolerance. This is not a
/// recommended/default tolerance or a source-admission policy.
pub const USE_GLOBAL_TOLERANCE: f32 = f32::from_bits(0x4f80_0000);

/// One present context with disjoint writable state and prepared marker.
/// `flags: None` explicitly means no flag view, not malformed-data fallback.
pub struct Q32SplitView<'a> {
    pub heights: &'a Q32HeightGrid,
    pub flags: Option<&'a Q32QuadFlags>,
    pub prepared: &'a mut u8,
    pub states: &'a mut Q32SplitStates,
    pub local_tolerance: f32,
    pub override_tolerance: f32,
}

/// The control that was actually needed when selecting an effective tolerance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToleranceField {
    Global,
    Local,
    Override,
}

/// Explicit finite-domain refusal. States and markers are unchanged on error.
/// This is an API policy, not emulation of native nonfinite/exception behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SplitPreparationError {
    #[error("nonfinite {field:?} tolerance in context {context}")]
    Tolerance {
        context: usize,
        field: ToleranceField,
    },
    #[error("nonfinite height in context {context}, tree {tree}, node {node}: {operand}")]
    Height {
        context: usize,
        tree: usize,
        node: usize,
        operand: NonFiniteOperand,
    },
}

fn finite(value: f32, context: usize, field: ToleranceField) -> Result<f32, SplitPreparationError> {
    if value.to_bits() & 0x7f80_0000 == 0x7f80_0000 {
        return Err(SplitPreparationError::Tolerance { context, field });
    }
    Ok(value)
}

fn select_tolerance(
    context: usize,
    global: f32,
    local: f32,
    override_value: f32,
) -> Result<f32, SplitPreparationError> {
    let override_value = finite(override_value, context, ToleranceField::Override)?;
    if override_value.to_bits() == USE_GLOBAL_TOLERANCE.to_bits() {
        return finite(global, context, ToleranceField::Global);
    }
    let local = finite(local, context, ToleranceField::Local)?;
    // Order finite encodings without depending on host denormal/rounding modes.
    // Equal signed zeros must select the local value, preserving its sign bit.
    let l = local.to_bits();
    let o = override_value.to_bits();
    if (l | o) & 0x7fff_ffff == 0 || local.total_cmp(&override_value).is_le() {
        Ok(local)
    } else {
        Ok(override_value)
    }
}

/// Prepare optional self/east/north/northeast views in that context order.
/// All views finish clearing/leaf seeding before any error evaluation. Each
/// unprepared context selects its tolerance once, then visits nodes 1023..1,
/// tree 0 before tree 1. A nonzero current state skips the whole node; otherwise
/// a nonzero child forces a split, or strict conditional error > tolerance does.
/// A new split writes its own state and a present W target to 1, even when the
/// target is prepared. It does not write the target's parent or revisit nodes.
/// Success records this phase's output, not a closed mesh or certification of
/// prepared-state consistency. The separate dependency sweep is a later
/// composition step, not a certification of arbitrary caller states or a
/// complete surface.
///
/// Override equal to `USE_GLOBAL_TOLERANCE` selects global and ignores local;
/// otherwise local <= override selects local (including zero ties), else
/// override. Unused controls and unvisited height samples need not be finite.
/// Every unprepared source selects tolerance even when flags force every node.
/// Finite negative tolerance does not bypass checks on samples actually used.
///
/// A bounded scratch copy (16 KiB of split-state payload plus metadata) makes
/// refusals atomic without heap allocation. On success, every present prepared
/// marker becomes 1, including initially nonzero markers; absent views receive
/// no writes. Errors follow context/node order, checking controls before nodes
/// and numeric operands through `deviation_exceeds_53rz`'s validation order.
/// This deterministic lazy first-error policy can report an earlier height
/// refusal before a later context's invalid tolerance.
/// Flags, working heights and reserved state slots are never changed.
pub fn prepare_splits_53rz(
    hierarchy: &Q32Hierarchy,
    topology: &Q32Neighbors,
    global_tolerance: f32,
    views: &mut [Option<Q32SplitView<'_>>; 4],
) -> Result<(), SplitPreparationError> {
    let mut working: [Option<Q32SplitStates>; 4] =
        std::array::from_fn(|c| views[c].as_ref().map(|view| *view.states));
    for (context, view) in views.iter().enumerate() {
        if let Some(view) = view {
            seed_if_unprepared(
                hierarchy,
                view.flags,
                *view.prepared,
                working[context]
                    .as_mut()
                    .expect("present source has scratch"),
            );
        }
    }
    for (context, view) in views.iter().enumerate() {
        let Some(view) = view else { continue };
        if *view.prepared != 0 {
            continue;
        }
        let tolerance = select_tolerance(
            context,
            global_tolerance,
            view.local_tolerance,
            view.override_tolerance,
        )?;
        for node in (1..Q32Hierarchy::FIRST_LEAF).rev() {
            for tree in 0..2 {
                let states = &working[context]
                    .as_ref()
                    .expect("present source has scratch")[tree];
                if states[node] != 0 {
                    continue;
                }
                let split = if states[node * 2 + 1] != 0 || states[node * 2] != 0 {
                    true
                } else {
                    let [p, q, _] = hierarchy.node(tree, node).expect("fixed internal node");
                    let [_, _, m] = hierarchy.node(tree, node * 2 + 1).expect("fixed odd child");
                    let height = |[x, y]: [u16; 2]| view.heights[usize::from(y)][usize::from(x)];
                    deviation_exceeds_53rz(height(p), height(q), height(m), tolerance).map_err(
                        |operand| SplitPreparationError::Height {
                            context,
                            tree,
                            node,
                            operand,
                        },
                    )?
                };
                working[context]
                    .as_mut()
                    .expect("present source has scratch")[tree][node] = u8::from(split);
                if split {
                    if let Some(target) =
                        topology.neighbors(context, tree, node).expect("fixed node")[2]
                    {
                        if let Some(states) = working[usize::from(target.context)].as_mut() {
                            states[usize::from(target.tree)][usize::from(target.node)] = 1;
                        }
                    }
                }
            }
        }
    }
    for (context, view) in views.iter_mut().enumerate() {
        if let Some(view) = view {
            *view.states = working[context].take().expect("present source has scratch");
            *view.prepared = 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::select_tolerance;

    #[test]
    fn selector_preserves_local_zero_ties_and_orders_finite_extremes() {
        for (local, override_value) in [
            (0, 0),
            (0, 0x8000_0000),
            (0x8000_0000, 0),
            (0x8000_0000, 0x8000_0000),
        ] {
            assert_eq!(
                select_tolerance(
                    0,
                    f32::NAN,
                    f32::from_bits(local),
                    f32::from_bits(override_value)
                )
                .unwrap()
                .to_bits(),
                local
            );
        }
        let ordered = [
            0xff7f_ffff,
            0xbf80_0000,
            0x8080_0000,
            0x807f_ffff,
            0x8000_0001,
            0,
            1,
            0x007f_ffff,
            0x0080_0000,
            0x3f7f_ffff,
            0x3f80_0000,
            0x3f80_0001,
            0x7f7f_ffff,
        ];
        for (l, &local) in ordered.iter().enumerate() {
            for (o, &override_value) in ordered.iter().enumerate() {
                let selected = select_tolerance(
                    0,
                    f32::NAN,
                    f32::from_bits(local),
                    f32::from_bits(override_value),
                )
                .unwrap();
                assert_eq!(selected.to_bits(), ordered[l.min(o)]);
            }
        }
    }
}
