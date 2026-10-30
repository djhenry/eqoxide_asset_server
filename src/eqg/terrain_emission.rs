//! Conditional q32 index emission and working-height reconciliation.
//!
//! This is one phase for supplied raw states and current working heights. It
//! neither establishes a complete prepared surface nor selects a source/bake
//! profile. Arithmetic is conditional on 53-bit rounding toward zero; no active
//! native control mode, cache history or render/collision equivalence is proved.

use super::{
    terrain_arithmetic::{midpoint_53rz, NonFiniteOperand},
    terrain_dependencies::Q32SplitStates,
    terrain_hierarchy::{GridTriangle, Q32Hierarchy},
    terrain_split_stage::Q32HeightGrid,
};

/// Ordered full-grid triangle indices. Arbitrary raw states can produce
/// overlapping coarse/fine triangles; this type does not certify a valid mesh.
/// Storage has a fixed bound and uses no heap allocation.
pub struct Q32Emission {
    triangles: [[u16; 3]; Self::MAX_TRIANGLES],
    len: usize,
}

impl std::fmt::Debug for Q32Emission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Q32Emission")
            .field("triangles", &self.triangles())
            .finish()
    }
}

impl Q32Emission {
    /// Each of the two trees' 2047 usable node identities can be emitted at most
    /// once: roots directly, other nodes only through their unique parent.
    /// This conservative storage bound assumes neither ancestor consistency nor
    /// a 2048-face limit; it is not the tight maximum of the current traversal.
    pub const MAX_TRIANGLES: usize = 2 * (Q32Hierarchy::NODE_END - 1);

    /// Indices are `33*y+x`, in emission order with stored P/Q/R corner order.
    /// No world scaling, output-basis conversion or compact vertex pool is used.
    pub fn triangles(&self) -> &[[u16; 3]] {
        &self.triangles[..self.len]
    }

    fn push(&mut self, corners: GridTriangle) {
        self.triangles[self.len] = corners.map(|[x, y]| 33 * y + x);
        self.len += 1;
    }
}

/// A used midpoint endpoint was nonfinite. Caller heights remain unchanged.
/// This finite-domain refusal is API policy, not native exception emulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("nonfinite midpoint endpoint in tree {tree}, node {node}: {operand}")]
pub struct MidpointError {
    pub tree: usize,
    pub node: usize,
    pub operand: NonFiniteOperand,
}

/// Emit one present self view; the caller omits this call for an absent view.
/// This does not model reuse of native output counters for absent input.
///
/// Visit internal nodes 1 through 1023 ascending, tree 0 before tree 1. A zero root
/// emits itself. Every zero internal node immediately stores its current P/Q
/// mean at its odd child's R, even beneath an already emitted coarser ancestor.
/// Nonzero nodes instead emit their zero-state children, odd then even, with
/// no height reads or stores. A leaf is emitted only when its state is zero and
/// its parent is nonzero; any nonzero leaf byte suppresses that leaf. There is
/// no recursive traversal or ancestor prune.
///
/// Midpoints use `midpoint_53rz`, including destination binary32 rounding toward
/// zero. Stores affect subsequent reads in the same working grid. Only used
/// endpoints require finite values: an unread or overwritten nonfinite cell
/// need not cause refusal. Success does not certify all output heights finite.
///
/// Scratch heights and owned bounded index storage make refusal atomic: on any
/// error no caller height changes or partial output are published. Success
/// commits heights and returns the indices. States are never modified; no
/// dependency sweep, flag regeneration, source admission or route is enabled.
pub fn emit_53rz(
    hierarchy: &Q32Hierarchy,
    states: &Q32SplitStates,
    heights: &mut Q32HeightGrid,
) -> Result<Q32Emission, MidpointError> {
    let mut working = *heights;
    let mut output = Q32Emission {
        triangles: [[0; 3]; Q32Emission::MAX_TRIANGLES],
        len: 0,
    };
    // The hierarchy constructor creates both complete immutable trees. All
    // node/child indices supplied by these fixed loops are construction invariants.
    for node in 1..Q32Hierarchy::FIRST_LEAF {
        for (tree, tree_states) in states.iter().enumerate() {
            if tree_states[node] == 0 {
                let corners = hierarchy.node(tree, node).expect("fixed internal node");
                if node == 1 {
                    output.push(corners);
                }
                let [p, q, _] = corners;
                let [_, _, [mx, my]] = hierarchy.node(tree, node * 2 + 1).expect("fixed odd child");
                let height = |[x, y]: [u16; 2]| working[usize::from(y)][usize::from(x)];
                let midpoint =
                    midpoint_53rz(height(p), height(q)).map_err(|operand| MidpointError {
                        tree,
                        node,
                        operand,
                    })?;
                working[usize::from(my)][usize::from(mx)] = midpoint;
            } else {
                for child in [node * 2 + 1, node * 2] {
                    if tree_states[child] == 0 {
                        output.push(hierarchy.node(tree, child).expect("fixed child"));
                    }
                }
            }
        }
    }
    *heights = working;
    Ok(output)
}
