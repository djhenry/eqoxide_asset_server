//! One conditional per-self preparation transaction for supplied q32 views.
//!
//! This composes split preparation, dependency propagation, emission and current
//! diagonal reconstruction. It does not select project membership, schedule
//! seams, establish cache history or choose a bake profile. The 53-bit/toward-zero
//! arithmetic remains conditional; no complete surface or query route is enabled.

use super::{
    terrain_dependencies::{propagate_once, Q32SplitStates},
    terrain_emission::{emit_53rz, MidpointError, Q32Emission},
    terrain_hierarchy::Q32Hierarchy,
    terrain_leaf_states::Q32QuadFlags,
    terrain_neighbors::Q32Neighbors,
    terrain_split_stage::{
        prepare_splits_53rz, Q32HeightGrid, Q32SplitView, SplitPreparationError,
    },
};

/// A present working view with disjoint mutable arrays and raw control inputs.
/// Concrete grids are required; missing metadata is not replaced by defaults.
pub struct Q32WorkingView<'a> {
    pub heights: &'a mut Q32HeightGrid,
    pub flags: &'a mut Q32QuadFlags,
    pub states: &'a mut Q32SplitStates,
    pub prepared: &'a mut u8,
    pub local_tolerance: f32,
    pub override_tolerance: f32,
}

/// A used-input refusal from either numerical phase. No caller field changes
/// on error. This is an atomic helper policy, not native exception emulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PreparationError {
    #[error(transparent)]
    Split(#[from] SplitPreparationError),
    #[error(transparent)]
    Midpoint(#[from] MidpointError),
}

/// Preserve each flag's low seven bits and rebuild only 0x80 from emitted
/// negative diagonal edges. Clear stale bits even for an empty stream. This is
/// an edge-cell union, with no interior fill, hole test or later-edge clearing.
///
/// The opaque emission from the fixed hierarchy guarantees full-grid ids and
/// axis/45-degree edges. Those bounds make the integer walk safe; an arbitrary
/// triangle slice would need separate validation. No heights or FP mode enter
/// this phase. This does not certify render/collision surface equivalence.
pub fn rebuild_diagonal_flags(emission: &Q32Emission, flags: &mut Q32QuadFlags) {
    for row in flags.iter_mut() {
        for flag in row {
            *flag &= 0x7f;
        }
    }
    for triangle in emission.triangles() {
        for edge in 0..3 {
            let a = triangle[edge];
            let b = triangle[(edge + 1) % 3];
            let (ax, ay) = (a % 33, a / 33);
            let (bx, by) = (b % 33, b / 33);
            let (right_x, right_y, left_x) = if ax < bx && ay > by {
                (bx, by, ax)
            } else if bx < ax && by > ay {
                (ax, ay, bx)
            } else {
                continue;
            };
            for step in 0..right_x - left_x {
                flags[usize::from(right_y + step)][usize::from(right_x - 1 - step)] |= 0x80;
            }
        }
    }
}

/// Update a concrete self view and optional east/north/northeast contexts.
/// First prepare all present split views using their pre-emission heights and
/// flags, then run one dependency sweep, emit self and rebuild self diagonals.
/// Dependency, emission and flag rebuilding still run for a prepared self.
/// The prepared marker caches split preparation only, not emitted output.
/// Neighbor state changes are retained for the caller's later update of that
/// view as self; this call does not emit or rebuild neighbor heights/flags.
///
/// Bounded scratch state leaves every caller field unchanged on a returned
/// `PreparationError`. Success commits all present states/markers and self
/// heights/flags. Neighbor heights, neighbor flags and control values remain
/// unchanged. The existing phases'
/// lazy finite-used-input rules apply; no extra whole-grid validation is added.
/// Scratch and output payloads are bounded and require no heap allocation;
/// total compiler/ABI stack usage is not specified.
///
/// Caller-supplied cached states may be unreachable or overlap geometrically.
/// This one-update model does not admit source data, certify native FP control
/// persistence or define a complete project load/cache/seam policy.
pub fn prepare_self_53rz(
    hierarchy: &Q32Hierarchy,
    topology: &Q32Neighbors,
    global: f32,
    self_view: &mut Q32WorkingView<'_>,
    neighbors: &mut [Option<Q32WorkingView<'_>>; 3],
) -> Result<Q32Emission, PreparationError> {
    let mut states: [Option<Q32SplitStates>; 4] = std::array::from_fn(|context| {
        if context == 0 {
            Some(*self_view.states)
        } else {
            neighbors[context - 1].as_ref().map(|view| *view.states)
        }
    });
    let mut prepared: [u8; 4] = std::array::from_fn(|context| {
        if context == 0 {
            *self_view.prepared
        } else {
            neighbors[context - 1]
                .as_ref()
                .map_or(0, |view| *view.prepared)
        }
    });
    {
        let mut state_slots = states.iter_mut();
        let mut marker_slots = prepared.iter_mut();
        let mut views = std::array::from_fn(|context| {
            // Advance both slots before skipping an absent context.
            let state_slot = state_slots.next().expect("fixed state slot");
            let prepared = marker_slots.next().expect("fixed marker slot");
            let states = state_slot.as_mut()?;
            let source = if context == 0 {
                &*self_view
            } else {
                neighbors[context - 1]
                    .as_ref()
                    .expect("present scratch source")
            };
            Some(Q32SplitView {
                heights: &*source.heights,
                flags: Some(&*source.flags),
                states,
                prepared,
                local_tolerance: source.local_tolerance,
                override_tolerance: source.override_tolerance,
            })
        });
        prepare_splits_53rz(hierarchy, topology, global, &mut views)?;
    }
    {
        let mut views = states.each_mut().map(Option::as_mut);
        propagate_once(topology, &mut views);
    }
    let mut heights = *self_view.heights;
    let output = emit_53rz(
        hierarchy,
        states[0].as_ref().expect("self is present"),
        &mut heights,
    )?;
    let mut flags = *self_view.flags;
    rebuild_diagonal_flags(&output, &mut flags);

    *self_view.states = states[0].take().expect("self is present");
    *self_view.prepared = prepared[0];
    *self_view.heights = heights;
    *self_view.flags = flags;
    for (index, view) in neighbors.iter_mut().enumerate() {
        if let Some(view) = view {
            *view.states = states[index + 1].take().expect("present scratch source");
            *view.prepared = prepared[index + 1];
        }
    }
    Ok(output)
}
