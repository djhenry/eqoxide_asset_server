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
    for prepared in 1..=u8::MAX {
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

fn centroid_quad(hierarchy: &Q32Hierarchy, tree: usize, leaf: usize) -> [usize; 2] {
    let corners = hierarchy.node(tree, leaf).unwrap();
    std::array::from_fn(|axis| {
        let coordinates = corners.map(|point| usize::from(point[axis]));
        assert_eq!(
            coordinates.iter().max().unwrap() - coordinates.iter().min().unwrap(),
            1
        );
        let sum: usize = coordinates.into_iter().sum();
        assert_ne!(sum % 3, 0);
        sum / 3
    })
}

fn quad_members(hierarchy: &Q32Hierarchy) -> Vec<Vec<(usize, usize)>> {
    // Every leaf is a unit triangle, so its strictly interior centroid locates
    // its cell without the production leaf-quad lookup or its P/Q-edge rule.
    let mut members = vec![Vec::new(); 32 * 32];
    for tree in 0..2 {
        for leaf in Q32Hierarchy::FIRST_LEAF..Q32Hierarchy::NODE_END {
            let [x, y] = centroid_quad(hierarchy, tree, leaf);
            members[y * 32 + x].push((tree, leaf));
        }
    }
    assert!(members.iter().all(|nodes| nodes.len() == 2));
    assert_eq!(members[6 * 32 + 12], [(0, 1024), (0, 1027)]);
    assert_eq!(members[25 * 32 + 19], [(1, 1024), (1, 1027)]);
    members
}

#[test]
fn isolated_quads_cover_all_locations_and_both_hierarchy_trees() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let members = quad_members(&hierarchy);
    for y in 0..32 {
        for x in 0..32 {
            for (flag, leaf_state, parent_state) in
                [(1, 0xff, 1), (4, 0, 1), (5, 0xff, 1), (0xfa, 0, 0)]
            {
                let mut flags: Q32QuadFlags = [[0; 32]; 32];
                flags[y][x] = flag;
                let mut want = cleared_states();
                for &(tree, leaf) in &members[y * 32 + x] {
                    want[tree][leaf] = leaf_state;
                    want[tree][leaf / 2] = parent_state;
                }
                let mut states = initial_states();
                seed_if_unprepared(&hierarchy, Some(&flags), 0, &mut states);
                assert_states(&states, &want);
            }
        }
    }
}

#[test]
fn asymmetric_fields_preserve_mixed_sibling_requests_and_ignore_other_flag_bits() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let members = quad_members(&hierarchy);
    // Literal outcomes for the eight low-three-bit combinations. Parent
    // requests are a union over geometric cell membership, independent of visit order.
    let outcomes = [
        (0, 0),
        (0xff, 1),
        (0, 0),
        (0xff, 1),
        (0, 1),
        (0xff, 1),
        (0, 1),
        (0xff, 1),
    ];
    for field in 0..10 {
        let flags: Q32QuadFlags = std::array::from_fn(|y| {
            std::array::from_fn(|x| {
                if field < 6 {
                    let pair = [(1, 4), (1, 0), (4, 0)][field / 2];
                    if (x + y + field % 2) % 2 == 0 {
                        pair.0
                    } else {
                        pair.1
                    }
                } else {
                    ((y * 32 + x) * 73 + (field - 6) * 61) as u8
                }
            })
        });
        if field < 6 {
            for tree in 0..2 {
                for parent in Q32Hierarchy::FIRST_LEAF / 2..Q32Hierarchy::FIRST_LEAF {
                    let [x0, y0] = centroid_quad(&hierarchy, tree, parent * 2);
                    let [x1, y1] = centroid_quad(&hierarchy, tree, parent * 2 + 1);
                    assert_ne!(flags[y0][x0], flags[y1][x1]);
                }
            }
        } else {
            let mut seen = [false; 256];
            for &flag in flags.iter().flatten() {
                seen[usize::from(flag)] = true;
            }
            assert!(seen.into_iter().all(|present| present));
        }
        let mut want = cleared_states();
        for (quad, nodes) in members.iter().enumerate() {
            let (leaf_state, parent_state) = outcomes[usize::from(flags[quad / 32][quad % 32] % 8)];
            for &(tree, leaf) in nodes {
                want[tree][leaf] = leaf_state;
                want[tree][leaf / 2] |= parent_state;
            }
        }
        let mut states = initial_states();
        seed_if_unprepared(&hierarchy, Some(&flags), 0, &mut states);
        assert_states(&states, &want);
    }
}
