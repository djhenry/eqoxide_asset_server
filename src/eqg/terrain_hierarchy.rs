//! Integer topology for a fixed 32-by-32-quad terrain tile.
//!
//! This constructs ordered triangle corners only. It does not admit source
//! dimensions, compute refinement or neighbor links, prepare heights, emit a
//! surface, or establish rendering/collision equivalence.

use std::collections::TryReserveError;

/// Ordered P, Q, R corners in the source tile's integer XY grid, before any
/// world-unit scaling, placement or output-basis conversion. With X drawn right
/// and Y drawn up, P/Q/R have clockwise winding (negative signed doubled area);
/// output-basis conversion may change that winding.
pub type GridTriangle = [[u16; 2]; 3];

/// Two immutable binary triangle trees for `q = 32`.
///
/// Tree 0 is rooted at `(0,32), (32,0), (0,0)`; tree 1 at
/// `(32,0), (0,32), (32,32)`. Node identifiers start at 1. For parent `k`,
/// child `2*k` has corners `(Q,R,M)` and child `2*k+1` has `(R,P,M)`,
/// where `M` is the exact integer midpoint of the parent's P/Q edge.
/// Each tree stores 2047 nodes, including 1024 leaf triangles. Across both
/// trees, 2048 leaf triangles partition 1024 source-grid quads; the 4094 stored
/// triangle records include every refinement level, not one emitted surface.
///
/// This is a fixed-grid template, not a validator for an input tile's dimensions.
/// Callers must admit source dimensions separately and can retain/share this
/// immutable value rather than reconstructing it for every tile.
pub struct Q32Hierarchy {
    trees: [Vec<GridTriangle>; 2],
}

impl Q32Hierarchy {
    /// Source-grid quads along each edge of this fixed template. This is not a
    /// configurable dimension; other source sizes require separate admission.
    pub const QUADS_PER_EDGE: u16 = 32;
    /// First leaf identifier in either tree. Earlier valid identifiers are
    /// internal nodes and cannot be used for leaf-quad lookup.
    pub const FIRST_LEAF: usize = Self::QUADS_PER_EDGE as usize * Self::QUADS_PER_EDGE as usize;
    /// Exclusive upper bound of valid node identifiers in either tree.
    pub const NODE_END: usize = Self::FIRST_LEAF * 2;

    /// Construct both trees, requesting capacity for 4094 triangle records.
    /// The requested element payload is 49,128 bytes (4094 records of 12 bytes);
    /// vector headers, allocator overhead and excess capacity are not included.
    /// Allocation failure returns an error without exposing a partial hierarchy.
    pub fn try_new() -> Result<Self, TryReserveError> {
        let mut trees = [Vec::new(), Vec::new()];
        for tree in &mut trees {
            tree.try_reserve_exact(Self::NODE_END - 1)?;
        }
        let edge = Self::QUADS_PER_EDGE;
        trees[0].push([[0, edge], [edge, 0], [0, 0]]);
        trees[1].push([[edge, 0], [0, edge], [edge, edge]]);
        for tree in &mut trees {
            for parent in 1..Self::FIRST_LEAF {
                let [p, q, r] = tree[parent - 1];
                // For every internal edge of this fixed grid, both sums are
                // even and at most 64: halving is exact and cannot overflow u16.
                debug_assert_eq!((p[0] + q[0]) % 2, 0);
                debug_assert_eq!((p[1] + q[1]) % 2, 0);
                let midpoint = [(p[0] + q[0]) / 2, (p[1] + q[1]) / 2];
                tree.push([q, r, midpoint]);
                tree.push([r, p, midpoint]);
            }
        }
        Ok(Self { trees })
    }

    /// Return ordered corners for tree 0 or 1 and node identifiers from 1
    /// through 2047 (`NODE_END` is exclusive).
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
