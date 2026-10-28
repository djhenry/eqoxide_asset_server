use eqoxide_asset_server::eqg::{
    terrain_dependencies::{propagate_once, Q32SplitStates},
    terrain_hierarchy::Q32Hierarchy,
    terrain_neighbors::Q32Neighbors,
};
use std::{cmp::Reverse, collections::BTreeMap};

fn run_once(topology: &Q32Neighbors, states: &mut [Q32SplitStates; 4], present: [bool; 4]) {
    let identities = states.each_ref().map(std::ptr::from_ref);
    let [a, b, c, d] = states;
    let mut views = [
        present[0].then_some(a),
        present[1].then_some(b),
        present[2].then_some(c),
        present[3].then_some(d),
    ];
    propagate_once(topology, &mut views);
    for (context, view) in views.iter().enumerate() {
        assert_eq!(view.is_some(), present[context], "context={context}");
        if let Some(view) = view.as_deref() {
            assert!(std::ptr::eq(view, identities[context]), "context={context}");
        }
    }
}

fn assert_states(got: &[Q32SplitStates; 4], want: &[Q32SplitStates; 4]) {
    for (context, (got, want)) in got.iter().zip(want).enumerate() {
        for (tree, (got, want)) in got.iter().zip(want).enumerate() {
            for (node, (&got, &want)) in got.iter().zip(want).enumerate() {
                assert_eq!(got, want, "context={context} tree={tree} node={node}");
            }
        }
    }
}

#[test]
fn roots_write_reserved_parent_slots_in_tree_order() {
    let topology = Q32Neighbors::try_new().unwrap();
    let mut states = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
    states[0][0][1] = 7;
    run_once(&topology, &mut states, [true, false, false, false]);
    let mut want = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
    want[0][0][0] = 1;
    want[0][0][1] = 1;
    want[0][1][0] = 1;
    want[0][1][1] = 1;
    assert_states(&states, &want);

    let mut states = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
    states[0][1][1] = 7;
    states[0][1][0] = 99;
    run_once(&topology, &mut states, [true, false, false, false]);
    let mut want = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
    want[0][0][0] = 1;
    want[0][0][1] = 1;
    want[0][1][0] = 99;
    want[0][1][1] = 7;
    assert_states(&states, &want);
}

#[test]
fn present_target_receives_writes_and_absent_target_does_not() {
    let topology = Q32Neighbors::try_new().unwrap();
    let mut states = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
    states[0][1][2] = 9;
    run_once(&topology, &mut states, [true, false, true, false]);
    let mut want = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
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

    let mut states = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
    states[0][1][2] = 9;
    states[2][0][1] = 37;
    let want = states;
    run_once(&topology, &mut states, [true, false, false, false]);
    assert_states(&states, &want);
}

#[test]
fn one_context_ordered_sweep_does_not_iterate_to_closure() {
    let topology = Q32Neighbors::try_new().unwrap();
    let mut states = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
    states[2][0][2] = 9;
    run_once(&topology, &mut states, [true, false, true, false]);
    let mut want = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
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
    let mut states = [[[0; Q32Hierarchy::NODE_END]; 2]; 4];
    states[0][0][0] = 88;
    states[0][1][0] = 99;
    states[0][0][Q32Hierarchy::FIRST_LEAF] = 7;
    states[0][1][Q32Hierarchy::NODE_END - 1] = 11;
    states[1][0][1] = 13;
    states[2][1][1] = 17;
    states[3][0][Q32Hierarchy::FIRST_LEAF - 1] = 19;
    let want = states;
    run_once(&topology, &mut states, [true, false, false, false]);
    assert_states(&states, &want);
    run_once(&topology, &mut states, [false; 4]);
    assert_states(&states, &want);
}

// Geometric node identity: original window context, tree, node identifier.
type Node = (usize, usize, usize);
type Edge = (u32, [[i32; 2]; 2]);
type Event = (Node, Option<Node>);

struct GeometricSweep {
    targets: BTreeMap<Node, Option<Node>>,
    events: Vec<Event>,
}

impl GeometricSweep {
    fn new(hierarchy: &Q32Hierarchy) -> Self {
        let origins = [[0, 0], [32, 0], [0, 32], [32, 32]];
        let mut incident = BTreeMap::<Edge, Vec<Node>>::new();
        let mut source_edges = BTreeMap::<Node, Edge>::new();
        for (context, origin) in origins.into_iter().enumerate() {
            for tree in 0..2 {
                for node in 1_usize..Q32Hierarchy::FIRST_LEAF {
                    let depth = usize::BITS - 1 - node.leading_zeros();
                    let [p, q, r] = hierarchy
                        .node(tree, node)
                        .unwrap()
                        .map(|[x, y]| [i32::from(x) + origin[0], i32::from(y) + origin[1]]);
                    let identity = (context, tree, node);
                    // Index geometric edge incidences without assuming which
                    // neighbor role the matching edge occupies.
                    for mut points in [[p, q], [q, r], [r, p]] {
                        points.sort();
                        incident.entry((depth, points)).or_default().push(identity);
                    }
                    let mut points = [p, q];
                    points.sort();
                    source_edges.insert(identity, (depth, points));
                }
            }
        }
        let targets = source_edges
            .into_iter()
            .map(|(source, edge)| {
                let owners = &incident[&edge];
                assert!((1..=2).contains(&owners.len()), "{source:?}");
                assert_eq!(owners.iter().filter(|&&node| node == source).count(), 1);
                let target = owners.iter().copied().find(|&node| node != source);
                (source, target)
            })
            .collect::<BTreeMap<_, _>>();
        let mut events = targets
            .iter()
            .map(|(&source, &target)| (source, target))
            .collect::<Vec<_>>();
        events.sort_by_key(|&((context, tree, node), _)| (Reverse(node), context, tree));
        Self { targets, events }
    }

    fn apply(&self, expected: &mut [Q32SplitStates; 4], present: [bool; 4]) {
        for &((context, tree, node), target) in &self.events {
            if !present[context] || expected[context][tree][node] == 0 {
                continue;
            }
            if let Some((target_context, target_tree, target_node)) = target {
                if present[target_context] {
                    expected[target_context][target_tree][target_node] = 1;
                    expected[target_context][target_tree][target_node / 2] = 1;
                }
            }
        }
    }
}

#[test]
fn deep_and_cross_context_sweeps_match_geometric_events_for_every_presence_mask() {
    let topology = Q32Neighbors::try_new().unwrap();
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let oracle = GeometricSweep::new(&hierarchy);
    assert_eq!(oracle.targets[&(0, 1, 3)], Some((1, 0, 3)));
    assert_eq!(oracle.targets[&(1, 1, 2)], Some((3, 0, 2)));
    for context in 0..4 {
        for tree in 0..2 {
            assert_eq!(
                oracle.targets[&(context, tree, 513)],
                Some((context, tree, 526))
            );
            assert_eq!(
                oracle.targets[&(context, tree, 1023)],
                Some((context, tree, 1020))
            );
        }
    }

    let selected = [1, 2, 3, 7, 16, 31, 32, 255, 256, 511, 512, 513, 1023];
    let mut cases = 0;
    for context in 0..4 {
        for tree in 0..2 {
            for node in selected {
                let source = (context, tree, node);
                for mask in 0_u8..16 {
                    let present = std::array::from_fn(|context| mask & (1 << context) != 0);
                    let mut actual: [Q32SplitStates; 4] = std::array::from_fn(|context| {
                        std::array::from_fn(|tree| {
                            std::array::from_fn(|node| {
                                if node == 0 {
                                    0x40 + (context * 2 + tree) as u8
                                } else if node >= Q32Hierarchy::FIRST_LEAF {
                                    0x80 + (context * 2 + tree) as u8
                                } else {
                                    0
                                }
                            })
                        })
                    });
                    actual[context][tree][node] = 9;
                    if let Some((context, tree, node)) = oracle.targets[&source] {
                        actual[context][tree][node] = 2;
                        actual[context][tree][node / 2] = 37;
                    }
                    // A present preseeded target can itself propagate even when
                    // the selected source's context is absent from this mask.
                    let mut expected = actual;
                    oracle.apply(&mut expected, present);
                    run_once(&topology, &mut actual, present);
                    assert_eq!(actual, expected, "source={source:?} mask={mask:04b}");
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 1664);
}
