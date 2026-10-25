//! One in-place dependency sweep over a fixed q32 neighbor template.
//!
//! This operates on caller-seeded state, not on heights, source availability or
//! prepared-cache markers. It neither establishes that a state is reachable
//! through full terrain preparation nor emits a surface or activates a bake.

use super::{terrain_hierarchy::Q32Hierarchy, terrain_neighbors::Q32Neighbors};

/// Raw states for two trees, including reserved parent slot zero. Geometric
/// node lookup still excludes zero. Nonzero source bytes trigger propagation;
/// targeted bytes are assigned one, while other bytes retain their values.
pub type Q32SplitStates = [[u8; Q32Hierarchy::NODE_END]; 2];

/// Visit internal nodes from 1023 down to 1, contexts self/east/north/northeast,
/// then tree 0 before tree 1. A nonzero source with a present W target view
/// assigns one to that target and its parent, including slot zero for a root.
///
/// Writes are immediately visible to subsequent visits in this single sweep.
/// There is no source-parent closure, repeated convergence or prepared-state
/// gate. Absent views and outside-window links produce no writes; leaf bytes
/// are not visited as sources. Fixed array lengths and exclusive borrows enforce
/// complete, disjoint views. The sweep allocates no memory.
pub fn propagate_once(topology: &Q32Neighbors, views: &mut [Option<&mut Q32SplitStates>; 4]) {
    for node in (1..Q32Hierarchy::FIRST_LEAF).rev() {
        for context in 0..4 {
            for tree in 0..2 {
                let Some(source) = views[context].as_deref() else {
                    continue;
                };
                if source[tree][node] == 0 {
                    continue;
                }
                let Some(target) = topology
                    .neighbors(context, tree, node)
                    .expect("fixed internal node is within the template")[2]
                else {
                    continue;
                };
                if let Some(states) = views[usize::from(target.context)].as_deref_mut() {
                    let tree = usize::from(target.tree);
                    let node = usize::from(target.node);
                    states[tree][node] = 1;
                    states[tree][node / 2] = 1;
                }
            }
        }
    }
}
