use eqoxide_asset_server::eqg::{
    terrain_dependencies::Q32SplitStates,
    terrain_hierarchy::Q32Hierarchy,
    terrain_leaf_states::{seed_if_unprepared, Q32QuadFlags},
};

fn initial_states() -> Q32SplitStates {
    [[0x35; 2048], [0x91; 2048]]
}

fn cleared_states() -> Q32SplitStates {
    let mut result = [[0; 2048]; 2];
    result[0][0] = 0x35;
    result[1][0] = 0x91;
    result
}

fn assert_states(got: &Q32SplitStates, want: &Q32SplitStates) {
    for tree in 0..2 {
        for node in 0..2048 {
            assert_eq!(got[tree][node], want[tree][node], "tree={tree} node={node}");
        }
    }
}

#[test]
fn absent_flags_clear_active_states_but_preserve_reserved_slots() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let mut states = initial_states();
    seed_if_unprepared(&hierarchy, None, 0, &mut states);
    assert_states(&states, &cleared_states());
}

#[test]
fn every_nonzero_prepared_marker_preserves_all_states() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let flags: Q32QuadFlags = [[5; 32]; 32];
    for prepared in [1, 2, 0x80, 0xff] {
        for flag_view in [None, Some(&flags)] {
            let mut states = initial_states();
            seed_if_unprepared(&hierarchy, flag_view, prepared, &mut states);
            assert_states(&states, &initial_states());
        }
    }
}

#[test]
fn asymmetric_quad_reaches_first_leaf_and_its_same_tree_partner() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    for (flag, leaf, parent) in [
        (1, 0xff, 1),
        (4, 0, 1),
        (5, 0xff, 1),
        (2, 0, 0),
        (0x80, 0, 0),
        (0xfe, 0, 1),
        (0xff, 0xff, 1),
    ] {
        let mut flags: Q32QuadFlags = [[0; 32]; 32];
        flags[6][12] = flag;
        let mut states = initial_states();
        seed_if_unprepared(&hierarchy, Some(&flags), 0, &mut states);
        let mut want = cleared_states();
        want[0][1024] = leaf;
        want[0][1027] = leaf;
        want[0][512] = parent;
        want[0][513] = parent;
        assert_states(&states, &want);
    }
}

#[test]
fn uniform_masks_seed_only_leaves_and_their_immediate_parents() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    for (flag, leaf, parent) in [
        (0, 0, 0),
        (1, 0xff, 1),
        (4, 0, 1),
        (5, 0xff, 1),
        (2, 0, 0),
        (0x80, 0, 0),
        (0xfa, 0, 0),
        (0xfe, 0, 1),
        (0xff, 0xff, 1),
    ] {
        let flags: Q32QuadFlags = [[flag; 32]; 32];
        let mut states = initial_states();
        seed_if_unprepared(&hierarchy, Some(&flags), 0, &mut states);
        let mut want = cleared_states();
        for tree in &mut want {
            tree[512..1024].fill(parent);
            tree[1024..2048].fill(leaf);
        }
        assert_states(&states, &want);
    }
}
