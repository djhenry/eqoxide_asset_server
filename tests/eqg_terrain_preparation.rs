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
        prepare_self_53rz(&hierarchy, &topology, self.global, &mut own, &mut neighbors)
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
    assert_eq!(got.states, want.states);
    assert_eq!(got.prepared, want.prepared);
    assert_eq!(got.flags, want.flags);
    for c in 0..4 {
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
    let mut f = Fixture::new(1);
    f.prepared = [128, 255, 0, 0];
    f.states[0] = [[0; 2048]; 2];
    f.states[1] = [[0; 2048]; 2];
    f.states[0][0][1] = 1;
    f.states[0][1][1] = 1;
    f.heights[0] = corner_grid();
    f.heights[0][16][16] = f32::NAN;
    let before = f.clone();
    assert_eq!(
        f.run().unwrap_err(),
        PreparationError::Midpoint(MidpointError {
            tree: 0,
            node: 4,
            operand: NonFiniteOperand::SecondEndpoint,
        })
    );
    assert_unchanged(&f, &before);
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
