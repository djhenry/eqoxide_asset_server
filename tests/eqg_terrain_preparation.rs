use eqoxide_asset_server::eqg::{
    terrain_arithmetic::NonFiniteOperand,
    terrain_dependencies::Q32SplitStates,
    terrain_emission::{emit_53rz, MidpointError, Q32Emission},
    terrain_hierarchy::Q32Hierarchy,
    terrain_leaf_states::Q32QuadFlags,
    terrain_neighbors::Q32Neighbors,
    terrain_preparation::{
        prepare_self_53rz, rebuild_diagonal_flags, PreparationError, Q32WorkingView,
    },
    terrain_split_stage::{
        Q32HeightGrid, SplitPreparationError, ToleranceField, USE_GLOBAL_TOLERANCE,
    },
};

#[derive(Clone)]
struct Fixture {
    neighbors: u8,
    heights: [Q32HeightGrid; 4],
    flags: [Q32QuadFlags; 4],
    states: [Q32SplitStates; 4],
    prepared: [u8; 4],
    locals: [f32; 4],
    overrides: [f32; 4],
    global: f32,
}
impl Fixture {
    fn new(neighbors: u8) -> Self {
        Self {
            neighbors,
            heights: [[[0.0; 33]; 33]; 4],
            flags: [[[0x80; 32]; 32]; 4],
            states: std::array::from_fn(|c| [[0x35 + c as u8; 2048], [0x91 + c as u8; 2048]]),
            prepared: [0; 4],
            locals: [1.0; 4],
            overrides: [USE_GLOBAL_TOLERANCE; 4],
            global: 1.0,
        }
    }
    fn run(&mut self) -> Result<Q32Emission, PreparationError> {
        let hierarchy = Q32Hierarchy::try_new().unwrap();
        let topology = Q32Neighbors::try_new().unwrap();
        let mut heights = self.heights.iter_mut();
        let mut flags = self.flags.iter_mut();
        let mut states = self.states.iter_mut();
        let mut prepared = self.prepared.iter_mut();
        let views: [Q32WorkingView; 4] = std::array::from_fn(|c| Q32WorkingView {
            heights: heights.next().unwrap(),
            flags: flags.next().unwrap(),
            states: states.next().unwrap(),
            prepared: prepared.next().unwrap(),
            local_tolerance: self.locals[c],
            override_tolerance: self.overrides[c],
        });
        let [mut own, e, n, ne] = views;
        let mut iter = [e, n, ne].into_iter();
        let mut neighbors = std::array::from_fn(|c| {
            let view = iter.next().unwrap();
            (self.neighbors & (1 << c) != 0).then_some(view)
        });
        let result =
            prepare_self_53rz(&hierarchy, &topology, self.global, &mut own, &mut neighbors);
        // Observe the caller-owned value fields on both success and refusal.
        self.locals[0] = own.local_tolerance;
        self.overrides[0] = own.override_tolerance;
        self.neighbors = 0;
        for (context, view) in neighbors.iter().enumerate() {
            if let Some(view) = view {
                self.neighbors |= 1 << context;
                self.locals[context + 1] = view.local_tolerance;
                self.overrides[context + 1] = view.override_tolerance;
            }
        }
        result
    }
}
fn assert_heights(got: &Q32HeightGrid, want: &Q32HeightGrid) {
    for (y, (g, w)) in got.iter().zip(want).enumerate() {
        for (x, (g, w)) in g.iter().zip(w).enumerate() {
            assert_eq!(g.to_bits(), w.to_bits(), "height({x},{y})");
        }
    }
}
fn assert_unchanged(got: &Fixture, want: &Fixture) {
    assert_eq!(got.neighbors, want.neighbors);
    assert_eq!(got.states, want.states);
    assert_eq!(got.prepared, want.prepared);
    assert_eq!(got.flags, want.flags);
    assert_eq!(got.global.to_bits(), want.global.to_bits());
    for c in 0..4 {
        assert_eq!(got.locals[c].to_bits(), want.locals[c].to_bits());
        assert_eq!(got.overrides[c].to_bits(), want.overrides[c].to_bits());
        assert_heights(&got.heights[c], &want.heights[c]);
    }
}
fn corner_grid() -> Q32HeightGrid {
    let mut h = [[100.0; 33]; 33];
    h[32][0] = 2.0;
    h[0][32] = 6.0;
    h[0][0] = 10.0;
    h[32][32] = 14.0;
    h
}

#[test]
fn flat_update_handles_all_neighbor_presence_patterns_without_neighbor_height_or_flag_writes() {
    for mask in 0..8 {
        let mut f = Fixture::new(mask);
        let before = f.clone();
        let output = f.run().unwrap();
        assert_eq!(output.triangles(), [[1056, 32, 0], [32, 1056, 1088]]);
        for c in 0..4 {
            let present = c == 0 || mask & (1 << (c - 1)) != 0;
            if present {
                assert_eq!(f.prepared[c], 1);
                for tree in 0..2 {
                    assert_eq!(f.states[c][tree][0], before.states[c][tree][0]);
                    assert!(f.states[c][tree][1..].iter().all(|&s| s == 0));
                }
            } else {
                assert_eq!(f.prepared[c], 0);
                assert_eq!(f.states[c], before.states[c]);
            }
            assert_heights(&f.heights[c], &before.heights[c]);
            if c != 0 {
                assert_eq!(f.flags[c], before.flags[c]);
            }
        }
        for y in 0..32 {
            for x in 0..32 {
                assert_eq!(f.flags[0][y][x], if x + y == 31 { 0x80 } else { 0 });
            }
        }
    }
}

#[test]
fn opaque_emission_rebuild_clears_stale_bits_even_when_empty_and_preserves_low_bits() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let empty = emit_53rz(&hierarchy, &[[1; 2048]; 2], &mut [[f32::NAN; 33]; 33]).unwrap();
    let mut flags = [[0xff; 32]; 32];
    rebuild_diagonal_flags(&empty, &mut flags);
    assert_eq!(flags, [[0x7f; 32]; 32]);
    let roots = emit_53rz(&hierarchy, &[[0; 2048]; 2], &mut [[0.0; 33]; 33]).unwrap();
    flags = [[0x7e; 32]; 32];
    rebuild_diagonal_flags(&roots, &mut flags);
    for (y, row) in flags.iter().enumerate() {
        for (x, &flag) in row.iter().enumerate() {
            assert_eq!(flag, if x + y == 31 { 0xfe } else { 0x7e });
        }
    }
    let before = flags;
    rebuild_diagonal_flags(&roots, &mut flags);
    assert_eq!(flags, before);
}

#[test]
fn forced_hole_preserves_low_flags_while_removing_its_current_diagonal() {
    let mut f = Fixture::new(0);
    f.flags[0] = [[0x84; 32]; 32];
    f.flags[0][12][7] = 0x85;
    f.heights[0] = [[f32::from_bits(0x7fc1_2345); 33]; 33];
    let before = f.clone();
    let output = f.run().unwrap();
    assert_eq!(output.triangles().len(), 2046);
    assert_eq!(f.prepared[0], 1);
    let mut marked = 0;
    for y in 0..32 {
        for x in 0..32 {
            let hole = (x, y) == (7, 12);
            let want = if hole {
                5
            } else if (x + y) % 2 == 1 {
                0x84
            } else {
                4
            };
            assert_eq!(f.flags[0][y][x], want);
            marked += usize::from(want & 0x80 != 0);
        }
    }
    assert_eq!(marked, 511);
    assert_heights(&f.heights[0], &before.heights[0]);
    assert_eq!(
        f.states[0]
            .iter()
            .flat_map(|t| t[1024..].iter())
            .filter(|&&s| s == 255)
            .count(),
        2
    );
    for tree in &f.states[0] {
        assert!(tree[1..1024].iter().all(|&s| s == 1));
        assert_eq!(tree[0], 1);
    }
}

#[test]
fn live_neighbor_dependency_reaches_cached_self_before_emission() {
    let mut f = Fixture::new(2);
    f.prepared[0] = 255;
    f.states[0] = [[0; 2048]; 2];
    f.flags[2][6][12] = 4;
    let before = f.clone();
    let output = f.run().unwrap();
    assert_eq!(f.prepared[0], 1);
    assert_eq!(f.prepared[2], 1);
    assert_eq!(f.states[0][1][2], 1);
    assert_eq!(f.states[0][1][1], 1);
    assert_eq!(f.states[0][0][0], 1);
    assert!(!output.triangles().is_empty());
    assert_eq!(f.flags[2], before.flags[2]);
    assert_heights(&f.heights[2], &before.heights[2]);
}

#[test]
fn late_split_refusal_publishes_none_of_the_preceding_context_work() {
    let mut f = Fixture::new(2);
    f.flags[0] = [[4; 32]; 32];
    f.heights[0] = [[f32::NAN; 33]; 33];
    f.overrides[2] = f32::NAN;
    let before = f.clone();
    assert_eq!(
        f.run().unwrap_err(),
        PreparationError::Split(SplitPreparationError::Tolerance {
            context: 2,
            field: ToleranceField::Override,
        })
    );
    assert_unchanged(&f, &before);
}

#[test]
fn late_emission_refusal_rolls_back_cache_normalization_dependency_and_height_stores() {
    for mask in 0..8 {
        let mut f = Fixture::new(mask);
        f.prepared = [128, 255, 128, 255];
        f.states = [[[0; 2048]; 2]; 4];
        f.states[0][0][1] = 1;
        f.states[0][1][1] = 1;
        f.heights[0] = corner_grid();
        f.heights[0][16][16] = f32::from_bits(0x7fc8_1313);
        f.global = f32::NAN;
        f.locals = [f32::NAN; 4];
        f.overrides = [f32::NAN; 4];
        let before = f.clone();
        assert_eq!(
            f.run().unwrap_err(),
            PreparationError::Midpoint(MidpointError {
                tree: 0,
                node: 4,
                operand: NonFiniteOperand::SecondEndpoint,
            }),
            "mask={mask}"
        );
        assert_unchanged(&f, &before);
    }
}

#[test]
fn prepared_self_still_emits_and_rebuilds_flags_without_selecting_unused_controls() {
    let mut f = Fixture::new(0);
    f.prepared[0] = 128;
    f.states[0] = [[0; 2048]; 2];
    f.heights[0] = corner_grid();
    f.global = f32::NAN;
    f.locals[0] = f32::NAN;
    f.overrides[0] = f32::NAN;
    let output = f.run().unwrap();
    assert_eq!(output.triangles(), [[1056, 32, 0], [32, 1056, 1088]]);
    assert_eq!(f.prepared[0], 1);
    assert_eq!(f.heights[0][16][16], 4.0);
    for y in 0..32 {
        for x in 0..32 {
            assert_eq!(f.flags[0][y][x], if x + y == 31 { 0x80 } else { 0 });
        }
    }
}

#[test]
fn cached_neighbor_state_is_committed_before_its_later_self_emission() {
    let mut f = Fixture::new(2);
    f.states = [[[0; 2048]; 2]; 4];
    f.states[0][1][2] = 9;
    f.prepared = [255, 0, 255, 0];
    f.global = f32::NAN;
    f.locals = [f32::NAN; 4];
    f.overrides = [f32::NAN; 4];
    f.heights[2] = corner_grid();
    f.flags[2] = [[0xa5; 32]; 32];
    let before = f.clone();

    f.run().unwrap();
    let mut want_states = [[[0; 2048]; 2]; 4];
    // This north W link and the root cross-links normalize raw 9 to 1.
    for (context, tree, nodes) in [
        (0, 0, &[0, 1][..]),
        (0, 1, &[1, 2][..]),
        (2, 0, &[0, 1, 2][..]),
        (2, 1, &[0, 1][..]),
    ] {
        for &node in nodes {
            want_states[context][tree][node] = 1;
        }
    }
    assert_eq!(f.states, want_states);
    assert_eq!(f.prepared, [1, 0, 1, 0]);
    for c in 1..4 {
        assert_heights(&f.heights[c], &before.heights[c]);
        assert_eq!(f.flags[c], before.flags[c]);
    }

    // The synthetic cached north view now becomes self. Its marker skips only
    // split preparation, while its current states drive fresh output and stores.
    f.heights.swap(0, 2);
    f.flags.swap(0, 2);
    f.states.swap(0, 2);
    f.prepared.swap(0, 2);
    f.locals.swap(0, 2);
    f.overrides.swap(0, 2);
    f.neighbors = 0;
    let before_self = f.clone();
    let output = f.run().unwrap();
    assert_eq!(
        output.triangles(),
        [
            [0, 1056, 544],
            [1088, 32, 544],
            [1056, 1088, 544],
            [544, 32, 16],
            [0, 544, 16],
        ]
    );
    // The zero tree-1 node 2 averages bottom corners 2 and 14. Its tree-0
    // counterpart is nonzero, so the top midpoint is deliberately not stored.
    assert_eq!(before_self.heights[0][32][16], 100.0);
    assert_eq!(f.heights[0][32][16], 8.0);
    assert_eq!(f.heights[0][0][16], 100.0);
    assert_eq!(f.heights[0][16][16], 100.0);
    assert_eq!(f.states, before_self.states);
    assert_eq!(f.prepared, before_self.prepared);
    for (y, row) in f.flags[0].iter().enumerate() {
        for (x, &flag) in row.iter().enumerate() {
            assert_eq!(flag, if x + y == 31 { 0xa5 } else { 0x25 });
        }
    }
    for c in 1..4 {
        assert_heights(&f.heights[c], &before_self.heights[c]);
        assert_eq!(f.flags[c], before_self.flags[c]);
    }
}

#[test]
fn cached_neighbors_ignore_unused_nonfinite_controls_in_every_presence_pattern() {
    for mask in 0..8 {
        let mut f = Fixture::new(mask);
        f.states = [[[0; 2048]; 2]; 4];
        f.prepared = [128, 1, 255, 128];
        f.global = f32::from_bits(0x7fc1_0101);
        for c in 0..4 {
            f.locals[c] = f32::from_bits(0x7fc2_0100 + c as u32);
            f.overrides[c] = f32::from_bits(0x7fc3_0100 + c as u32);
            f.heights[c] = [[f32::from_bits(0x7fc4_0100 + c as u32); 33]; 33];
            f.states[c][0][0] = 0x31 + c as u8;
            f.states[c][1][0] = 0x71 + c as u8;
        }
        f.heights[0] = corner_grid();
        let before = f.clone();
        let output = f.run().unwrap();
        assert_eq!(output.triangles(), [[1056, 32, 0], [32, 1056, 1088]]);
        assert_eq!(f.heights[0][16][16], 4.0);
        assert_eq!(f.states, before.states);
        for c in 0..4 {
            let present = c == 0 || mask & (1 << (c - 1)) != 0;
            assert_eq!(f.prepared[c], if present { 1 } else { before.prepared[c] });
            assert_eq!(f.locals[c].to_bits(), before.locals[c].to_bits());
            assert_eq!(f.overrides[c].to_bits(), before.overrides[c].to_bits());
            if c != 0 {
                assert_heights(&f.heights[c], &before.heights[c]);
                assert_eq!(f.flags[c], before.flags[c]);
            }
        }
        assert_eq!(f.global.to_bits(), before.global.to_bits());
    }
}

#[test]
fn used_neighbor_control_refusals_roll_back_every_presence_pattern() {
    for mask in 0..8 {
        for context in 1..4 {
            if mask & (1 << (context - 1)) == 0 {
                continue;
            }
            for field in [ToleranceField::Local, ToleranceField::Override] {
                let mut f = Fixture::new(mask);
                for c in 0..4 {
                    f.flags[c] = [[4; 32]; 32];
                    f.heights[c] = [[f32::from_bits(0x7fc5_0100 + c as u32); 33]; 33];
                    f.locals[c] = f32::from_bits(0x7fc6_0100 + c as u32);
                }
                if field == ToleranceField::Local {
                    f.overrides[context] = 0.5;
                } else {
                    f.overrides[context] = f32::from_bits(0x7fc7_0100 + context as u32);
                }
                let before = f.clone();
                assert_eq!(
                    f.run().unwrap_err(),
                    PreparationError::Split(SplitPreparationError::Tolerance { context, field }),
                    "mask={mask} context={context} field={field:?}"
                );
                assert_unchanged(&f, &before);
            }
        }
    }
}

#[test]
fn closed_hierarchy_edges_bound_the_diagonal_rebuild_walk() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    for tree in 0..2 {
        for node in 1..Q32Hierarchy::NODE_END {
            let triangle = hierarchy.node(tree, node).unwrap();
            for [x, y] in triangle {
                assert!(x <= 32 && y <= 32, "tree={tree} node={node}");
            }
            for edge in 0..3 {
                let [ax, ay] = triangle[edge];
                let [bx, by] = triangle[(edge + 1) % 3];
                let dx = ax.abs_diff(bx);
                let dy = ay.abs_diff(by);
                assert!(dx != 0 || dy != 0, "tree={tree} node={node}");
                assert!(dx == 0 || dy == 0 || dx == dy, "tree={tree} node={node}");
                // A negative diagonal's lower-Y endpoint plus its horizontal
                // extent cannot leave the 32-by-32 quad grid.
                if (ax < bx && ay > by) || (bx < ax && by > ay) {
                    assert!(ay.min(by) + dx <= 32, "tree={tree} node={node}");
                    assert!(ax.max(bx) >= dx, "tree={tree} node={node}");
                }
            }
        }
    }
}
