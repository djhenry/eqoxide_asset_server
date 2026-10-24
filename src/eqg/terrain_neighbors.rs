//! Fixed neighbor topology for four q32 tile contexts.
//!
//! The window is self, east, north, northeast in source-grid XY space. This
//! template contains no source-tile availability, prepared state or split state.
//! An edge outside this window does not establish that an authored tile is absent.

use super::terrain_hierarchy::Q32Hierarchy;
use std::collections::TryReserveError;

/// Identity of a node in the fixed template. Returned identities always use
/// contexts 0..4, trees 0..2 and node identifiers 1..2048.
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

fn slot(context: u8, tree: u8, node: u16) -> usize {
    (usize::from(context) * 2 + usize::from(tree)) * NODES + usize::from(node) - 1
}

fn child(link: Option<NeighborNode>, odd: bool) -> Option<NeighborNode> {
    link.map(|parent| NeighborNode {
        node: parent.node * 2 + u16::from(odd),
        ..parent
    })
}

/// Immutable neighbor records for both trees in each of four canonical contexts.
/// This is a topology prerequisite, not a prepared terrain surface or query mesh.
pub struct Q32Neighbors {
    links: Vec<[Option<NeighborNode>; 3]>,
}

impl Q32Neighbors {
    /// Allocate the bounded template with one fallible reservation for 16376
    /// node records. No partial template is exposed on allocation failure.
    pub fn try_new() -> Result<Self, TryReserveError> {
        let mut links = Vec::new();
        links.try_reserve_exact(RECORDS)?;
        links.resize(RECORDS, [None; 3]);
        for context in 0_u8..4 {
            for tree in 0_u8..2 {
                links[slot(context, tree, 1)] = ROOTS[usize::from(context) * 2 + usize::from(tree)];
            }
        }
        for context in 0_u8..4 {
            for parent in 1..Q32Hierarchy::FIRST_LEAF as u16 {
                for tree in 0_u8..2 {
                    let [u, v, w] = links[slot(context, tree, parent)];
                    let even = NeighborNode {
                        context,
                        tree,
                        node: parent * 2,
                    };
                    let odd = NeighborNode {
                        node: even.node + 1,
                        ..even
                    };
                    links[slot(context, tree, even.node)] =
                        [child(w, true), Some(odd), child(v, parent != 1)];
                    links[slot(context, tree, odd.node)] =
                        [Some(even), child(w, false), child(u, parent == 1)];
                }
            }
        }
        Ok(Self { links })
    }

    /// Return U, V, W neighbors, corresponding to unordered edges R-P, Q-R, P-Q.
    /// Contexts 0,1,2,3 mean self,east,north,northeast; trees are 0 or 1 and node
    /// identifiers are 1..2048. Invalid inputs return outer `None`. Inner `None`
    /// means a valid edge exits this fixed window, not a missing source tile.
    pub fn neighbors(
        &self,
        context: usize,
        tree: usize,
        node: usize,
    ) -> Option<[Option<NeighborNode>; 3]> {
        if context >= 4 || tree >= 2 || !(1..Q32Hierarchy::NODE_END).contains(&node) {
            return None;
        }
        Some(self.links[(context * 2 + tree) * NODES + node - 1])
    }
}
