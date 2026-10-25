//! Per-view integer state initialization before terrain error evaluation.
//!
//! This does not choose cache policy, establish source admission or complete
//! preparation reachability, evaluate errors, emit a surface or enable a route.

use super::{terrain_dependencies::Q32SplitStates, terrain_hierarchy::Q32Hierarchy};

/// Quad bytes indexed by `[y][x]`. Only masks 0x01 and 0x04 participate in this
/// stage; unrelated bits retain their possible meanings in other stages.
pub type Q32QuadFlags = [[u8; 32]; 32];

/// Skip all changes for any nonzero supplied prepared marker. Otherwise clear
/// both trees at indices 1..2048, preserving reserved slot zero, then seed leaves
/// from quad flags. Mask 0x01 sets leaf state 0xff and its immediate parent to 1;
/// mask 0x04 sets only the parent to 1. Mask 0x01 wins when both bits are set.
///
/// `None` explicitly represents an absent flag view: clearing still occurs. It
/// is not fallback for malformed or missing authored metadata. The caller omits
/// this call for an absent whole state view and owns stage scheduling. This
/// function does not mark a view prepared, mutate flags, propagate to further
/// ancestors or neighbors, or allocate memory.
pub fn seed_if_unprepared(
    hierarchy: &Q32Hierarchy,
    flags: Option<&Q32QuadFlags>,
    prepared: u8,
    states: &mut Q32SplitStates,
) {
    if prepared != 0 {
        return;
    }
    for tree in states.iter_mut() {
        tree[1..].fill(0);
    }
    let Some(flags) = flags else {
        return;
    };
    for leaf in Q32Hierarchy::FIRST_LEAF..Q32Hierarchy::NODE_END {
        for (tree, tree_states) in states.iter_mut().enumerate() {
            let [x, y] = hierarchy
                .leaf_quad(tree, leaf)
                .expect("fixed leaf is within the hierarchy");
            let flag = flags[usize::from(y)][usize::from(x)];
            if flag & 0x01 != 0 {
                tree_states[leaf] = 0xff;
            }
            if flag & 0x05 != 0 {
                tree_states[leaf / 2] = 1;
            }
        }
    }
}
