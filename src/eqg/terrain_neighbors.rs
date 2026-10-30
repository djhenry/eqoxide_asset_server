//! Fixed neighbor topology for four q32 tile contexts.
//!
//! The window is self, east, north, northeast in source-grid XY space. This
//! template contains no source-tile availability, prepared state or split state.
//! An edge outside this window does not establish that an authored tile is absent.

use super::terrain_hierarchy::Q32Hierarchy;
use std::collections::TryReserveError;

/// Identity of a node in the fixed template. Returned identities always use
/// contexts 0 through 3, trees 0 or 1 and node identifiers at least 1 and less
/// than `Q32Hierarchy::NODE_END`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeighborNode {
    pub context: u8,
    pub tree: u8,
    pub node: u16,
}

const NODES: usize = Q32Hierarchy::NODE_END - 1;
const RECORDS: usize = 4 * 2 * NODES;

const fn root(context: u8, tree: u8) -> Option<NeighborNode> {
    Some(NeighborNode {
        context,
        tree,
        node: 1,
    })
}

const ROOTS: [[Option<NeighborNode>; 3]; 8] = [
    [None, None, root(0, 1)],
    [root(1, 0), root(2, 0), root(0, 0)],
    [root(0, 1), None, root(1, 1)],
    [None, root(3, 0), root(1, 0)],
    [None, root(0, 1), root(2, 1)],
    [root(3, 0), None, root(2, 0)],
    [root(2, 1), root(1, 1), root(3, 1)],
    [None, None, root(3, 0)],
];

fn slot(context: usize, tree: usize, node: usize) -> usize {
    (context * 2 + tree) * NODES + node - 1
}

fn child(link: Option<NeighborNode>, odd: bool) -> Option<NeighborNode> {
    link.map(|parent| NeighborNode {
        node: parent.node * 2 + u16::from(odd),
        ..parent
    })
}

/// Immutable neighbor records for both trees in each of four fixed source-grid
/// contexts: self, east, north and northeast.
/// This is a topology prerequisite, not a prepared terrain surface or query mesh.
pub struct Q32Neighbors {
    links: Vec<[Option<NeighborNode>; 3]>,
}

impl Q32Neighbors {
    /// Allocate the bounded template with one fallible reservation for
    /// `RECORDS = 4 * 2 * (Q32Hierarchy::NODE_END - 1)` node records.
    /// No partial template is exposed on allocation failure.
    pub fn try_new() -> Result<Self, TryReserveError> {
        let mut links = Vec::new();
        links.try_reserve_exact(RECORDS)?;
        links.resize(RECORDS, [None; 3]);
        for context in 0_u8..4 {
            for tree in 0_u8..2 {
                links[slot(context.into(), tree.into(), 1)] =
                    ROOTS[usize::from(context) * 2 + usize::from(tree)];
            }
        }
        for context in 0_u8..4 {
            for parent in 1..Q32Hierarchy::FIRST_LEAF as u16 {
                for tree in 0_u8..2 {
                    let [u, v, w] = links[slot(context.into(), tree.into(), parent.into())];
                    let even = NeighborNode {
                        context,
                        tree,
                        node: parent * 2,
                    };
                    let odd = NeighborNode {
                        node: even.node + 1,
                        ..even
                    };
                    let is_root = parent == 1;
                    // Root side-edge orientations pair even.W with V's even
                    // child and odd.W with U's odd child; deeper parents use
                    // V's odd and U's even child. Across W, even.U and odd.V
                    // always pair with W's odd and even child respectively.
                    links[slot(context.into(), tree.into(), even.node.into())] =
                        [child(w, true), Some(odd), child(v, !is_root)];
                    links[slot(context.into(), tree.into(), odd.node.into())] =
                        [Some(even), child(w, false), child(u, is_root)];
                }
            }
        }
        Ok(Self { links })
    }

    /// Return U, V, W neighbors, corresponding to unordered edges R-P, Q-R, P-Q.
    /// Contexts 0,1,2,3 mean self,east,north,northeast; trees are 0 or 1 and node
    /// identifiers are at least 1 and less than `Q32Hierarchy::NODE_END`.
    /// Invalid inputs return outer `None`. Inner `None` means a valid edge exits
    /// this fixed window, not a missing source tile.
    pub fn neighbors(
        &self,
        context: usize,
        tree: usize,
        node: usize,
    ) -> Option<[Option<NeighborNode>; 3]> {
        if context >= 4 || tree >= 2 || !(1..Q32Hierarchy::NODE_END).contains(&node) {
            return None;
        }
        Some(self.links[slot(context, tree, node)])
    }
}
