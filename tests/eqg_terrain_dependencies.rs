use eqoxide_asset_server::eqg::{
    terrain_dependencies::{propagate_once, Q32SplitStates},
    terrain_neighbors::Q32Neighbors,
};

fn run_once(topology: &Q32Neighbors, states: &mut [Q32SplitStates; 4], present: [bool; 4]) {
    let [a, b, c, d] = states;
    propagate_once(
        topology,
        &mut [
            present[0].then_some(a),
            present[1].then_some(b),
            present[2].then_some(c),
            present[3].then_some(d),
        ],
    );
}

fn assert_states(got: &[Q32SplitStates; 4], want: &[Q32SplitStates; 4]) {
    for context in 0..4 {
        for tree in 0..2 {
            for node in 0..2048 {
                assert_eq!(
                    got[context][tree][node], want[context][tree][node],
                    "context={context} tree={tree} node={node}"
                );
            }
        }
    }
}

#[test]
fn roots_write_reserved_parent_slots_in_tree_order() {
    let topology = Q32Neighbors::try_new().unwrap();
    let mut states = [[[0; 2048]; 2]; 4];
    states[0][0][1] = 7;
    run_once(&topology, &mut states, [true, false, false, false]);
    let mut want = [[[0; 2048]; 2]; 4];
    want[0][0][0] = 1;
    want[0][0][1] = 1;
    want[0][1][0] = 1;
    want[0][1][1] = 1;
    assert_states(&states, &want);

    let mut states = [[[0; 2048]; 2]; 4];
    states[0][1][1] = 7;
    states[0][1][0] = 99;
    run_once(&topology, &mut states, [true, false, false, false]);
    let mut want = [[[0; 2048]; 2]; 4];
    want[0][0][0] = 1;
    want[0][0][1] = 1;
    want[0][1][0] = 99;
    want[0][1][1] = 7;
    assert_states(&states, &want);
}

#[test]
fn present_target_receives_writes_and_absent_target_does_not() {
    let topology = Q32Neighbors::try_new().unwrap();
    let mut states = [[[0; 2048]; 2]; 4];
    states[0][1][2] = 9;
    run_once(&topology, &mut states, [true, false, true, false]);
    let mut want = [[[0; 2048]; 2]; 4];
    for (context, tree, nodes) in [
        (0, 0, &[0, 1][..]),
        (0, 1, &[1, 2][..]),
        (2, 0, &[0, 1, 2][..]),
        (2, 1, &[0, 1][..]),
    ] {
        for &node in nodes {
            want[context][tree][node] = 1;
        }
    }
    assert_states(&states, &want);

    let mut states = [[[0; 2048]; 2]; 4];
    states[0][1][2] = 9;
    states[2][0][1] = 37;
    let want = states;
    run_once(&topology, &mut states, [true, false, false, false]);
    assert_states(&states, &want);
}

#[test]
fn one_context_ordered_sweep_does_not_iterate_to_closure() {
    let topology = Q32Neighbors::try_new().unwrap();
    let mut states = [[[0; 2048]; 2]; 4];
    states[2][0][2] = 9;
    run_once(&topology, &mut states, [true, false, true, false]);
    let mut want = [[[0; 2048]; 2]; 4];
    want[0][0][0] = 1;
    want[0][0][1] = 1;
    want[0][1][1] = 1;
    want[0][1][2] = 1;
    want[2][0][2] = 9;
    assert_states(&states, &want);

    run_once(&topology, &mut states, [true, false, true, false]);
    want[0][1][0] = 1;
    want[2][0][0] = 1;
    want[2][0][1] = 1;
    want[2][0][2] = 1;
    want[2][1][0] = 1;
    want[2][1][1] = 1;
    assert_states(&states, &want);
}

#[test]
fn leaves_reserved_bytes_and_absent_views_are_not_sources() {
    let topology = Q32Neighbors::try_new().unwrap();
    let mut states = [[[0; 2048]; 2]; 4];
    states[0][0][0] = 88;
    states[0][1][0] = 99;
    states[0][0][1024] = 7;
    states[0][1][2047] = 11;
    states[1][0][1] = 13;
    states[2][1][1] = 17;
    states[3][0][1023] = 19;
    let want = states;
    run_once(&topology, &mut states, [true, false, false, false]);
    assert_states(&states, &want);
    run_once(&topology, &mut states, [false; 4]);
    assert_states(&states, &want);
}
