//! Integer topology for a fixed 32-by-32-quad terrain tile.
//!
//! This constructs ordered triangle corners only. It does not admit source
//! dimensions, compute refinement or neighbor links, prepare heights, emit a
//! surface, or establish rendering/collision equivalence.

use std::collections::TryReserveError;

/// Ordered P, Q, R corners in the source tile's integer XY grid, before any
/// world-unit scaling, placement or output-basis conversion.
pub type GridTriangle = [[u16; 2]; 3];

/// Two immutable binary triangle trees for `q = 32`.
///
/// Tree 0 is rooted at `(0,32), (32,0), (0,0)`; tree 1 at
/// `(32,0), (0,32), (32,32)`. Node identifiers start at 1. For parent `k`,
/// child `2*k` has corners `(Q,R,M)` and child `2*k+1` has `(R,P,M)`,
/// where `M` is the exact integer midpoint of the parent's P/Q edge.
pub struct Q32Hierarchy {
    trees: [Vec<GridTriangle>; 2],
}

impl Q32Hierarchy {
    /// First leaf identifier in either tree. Earlier valid identifiers are
    /// internal nodes and cannot be used for leaf-quad lookup.
    pub const FIRST_LEAF: usize = 1024;
    /// Exclusive upper bound of valid node identifiers in either tree.
    pub const NODE_END: usize = 2048;

    /// Construct both trees, requesting capacity for 4094 triangles in total.
    /// Allocation failure returns an error without exposing a partial hierarchy.
    pub fn try_new() -> Result<Self, TryReserveError> {
        let mut trees = [Vec::new(), Vec::new()];
        for tree in &mut trees {
            tree.try_reserve_exact(Self::NODE_END - 1)?;
        }
        trees[0].push([[0, 32], [32, 0], [0, 0]]);
        trees[1].push([[32, 0], [0, 32], [32, 32]]);
        for tree in &mut trees {
            for parent in 1..Self::FIRST_LEAF {
                let [p, q, r] = tree[parent - 1];
                let midpoint = [(p[0] + q[0]) / 2, (p[1] + q[1]) / 2];
                tree.push([q, r, midpoint]);
                tree.push([r, p, midpoint]);
            }
        }
        Ok(Self { trees })
    }

    /// Return ordered corners for tree 0 or 1 and node identifiers 1..2048.
    /// Invalid tree/node identifiers return `None`, including zero.
    pub fn node(&self, tree: usize, node: usize) -> Option<GridTriangle> {
        self.trees.get(tree)?.get(node.checked_sub(1)?).copied()
    }

    /// Return the source-grid quad of a leaf, using the minimum X/Y of its P/Q
    /// edge. Internal nodes and all invalid identifiers return `None`.
    pub fn leaf_quad(&self, tree: usize, node: usize) -> Option<[u16; 2]> {
        if !(Self::FIRST_LEAF..Self::NODE_END).contains(&node) {
            return None;
        }
        let [p, q, _] = self.node(tree, node)?;
        Some([p[0].min(q[0]), p[1].min(q[1])])
    }
}
