use eqoxide_asset_server::eqg::{
    terrain_arithmetic::NonFiniteOperand,
    terrain_dependencies::Q32SplitStates,
    terrain_hierarchy::Q32Hierarchy,
    terrain_leaf_states::Q32QuadFlags,
    terrain_neighbors::Q32Neighbors,
    terrain_split_stage::{
        prepare_splits_53rz, Q32HeightGrid, Q32SplitView, SplitPreparationError, ToleranceField,
        USE_GLOBAL_TOLERANCE,
    },
};

struct Fixture {
    present: u8,
    heights: [Q32HeightGrid; 4],
    flags: [Option<Q32QuadFlags>; 4],
    states: [Q32SplitStates; 4],
    prepared: [u8; 4],
    local: [f32; 4],
    overrides: [f32; 4],
    global: f32,
}
impl Fixture {
    fn new(present: u8) -> Self {
        Self {
            present,
            heights: [[[0.0; 33]; 33]; 4],
            flags: [None; 4],
            states: std::array::from_fn(|c| [[0x35 + c as u8; 2048], [0x91 + c as u8; 2048]]),
            prepared: [0; 4],
            local: [1.0; 4],
            overrides: [USE_GLOBAL_TOLERANCE; 4],
            global: 1.0,
        }
    }
    fn run(&mut self) -> Result<(), SplitPreparationError> {
        let hierarchy = Q32Hierarchy::try_new().unwrap();
        let topology = Q32Neighbors::try_new().unwrap();
        let mut states = self.states.iter_mut();
        let mut prepared = self.prepared.iter_mut();
        let mut views = std::array::from_fn(|c| {
            let states = states.next().unwrap();
            let prepared = prepared.next().unwrap();
            (self.present & (1 << c) != 0).then(|| Q32SplitView {
                heights: &self.heights[c],
                flags: self.flags[c].as_ref(),
                states,
                prepared,
                local_tolerance: self.local[c],
                override_tolerance: self.overrides[c],
            })
        });
        prepare_splits_53rz(&hierarchy, &topology, self.global, &mut views)
    }
    fn peak(&mut self, height: f32) {
        let hierarchy = Q32Hierarchy::try_new().unwrap();
        let [_, _, [x, y]] = hierarchy.node(0, 2047).unwrap();
        self.heights[0][usize::from(y)][usize::from(x)] = height;
    }
}

#[test]
fn flat_preparation_clears_active_states_preserves_reserved_and_absent() {
    for mask in 0..16 {
        let mut f = Fixture::new(mask);
        let before = f.states;
        f.run().unwrap();
        for (c, old) in before.iter().enumerate() {
            if mask & (1 << c) == 0 {
                assert_eq!(f.states[c], *old);
                assert_eq!(f.prepared[c], 0);
            } else {
                assert_eq!(f.prepared[c], 1);
                for (tree, old_tree) in old.iter().enumerate() {
                    assert_eq!(f.states[c][tree][0], old_tree[0]);
                    assert!(f.states[c][tree][1..].iter().all(|&s| s == 0));
                }
            }
        }
    }
}

#[test]
fn cached_sources_ignore_unused_nonfinite_values_but_normalize_markers() {
    let mut f = Fixture::new(15);
    f.prepared = [1, 2, 128, 255];
    f.heights = [[[f32::NAN; 33]; 33]; 4];
    f.local = [f32::NAN; 4];
    f.overrides = [f32::NAN; 4];
    f.global = f32::NAN;
    f.flags = [Some([[5; 32]; 32]); 4];
    let before = f.states;
    f.run().unwrap();
    assert_eq!(f.states, before);
    assert_eq!(f.prepared, [1; 4]);
}

#[test]
fn forced_subtrees_do_not_read_heights() {
    for (flag, leaf) in [(4, 0), (1, 255), (5, 255)] {
        let mut f = Fixture::new(1);
        f.flags[0] = Some([[flag; 32]; 32]);
        f.heights[0] = [[f32::NAN; 33]; 33];
        f.run().unwrap();
        for tree in &f.states[0] {
            assert!(tree[1..1024].iter().all(|&s| s == 1));
            assert!(tree[1024..].iter().all(|&s| s == leaf));
        }
    }
}

#[test]
fn raw_tolerance_selection_and_strict_error_ties() {
    for (global, local, override_value, peak, split) in [
        (1.0, 0.0, USE_GLOBAL_TOLERANCE, 1.0, false),
        (0.0, 3.0, 2.0, 1.0, false),
        (0.0, 3.0, 7.0, 4.0, true),
        (1.0, 1.0, USE_GLOBAL_TOLERANCE, 1.0, false),
        (0.0, 1.0, USE_GLOBAL_TOLERANCE, 1.0, true),
    ] {
        let mut f = Fixture::new(1);
        f.global = global;
        f.local[0] = local;
        f.overrides[0] = override_value;
        f.peak(peak);
        f.run().unwrap();
        assert_eq!(f.states[0][0][1023], u8::from(split));
    }
}

#[test]
fn cached_target_is_writable_without_target_parent_propagation() {
    let mut f = Fixture::new(5);
    let mut flags = [[0; 32]; 32];
    flags[25][19] = 4;
    f.flags[0] = Some(flags);
    f.states[2] = [[0; 2048]; 2];
    f.prepared[2] = 128;
    f.run().unwrap();
    assert_eq!(f.states[2][0][2], 1);
    assert_eq!([f.states[2][0][1], f.states[2][1][1]], [0, 0]);
    assert_eq!(f.prepared[2], 1);
}

#[test]
fn all_views_are_seeded_before_any_error_context() {
    let mut f = Fixture::new(5);
    let mut flags = [[0; 32]; 32];
    flags[25][19] = 4;
    f.flags[0] = Some(flags);
    f.run().unwrap();
    assert_eq!([f.states[2][0][1], f.states[2][1][1]], [1, 1]);
}

#[test]
fn later_context_writes_do_not_revisit_earlier_roots() {
    let mut f = Fixture::new(5);
    let mut flags = [[0; 32]; 32];
    flags[6][12] = 4;
    f.flags[2] = Some(flags);
    f.run().unwrap();
    assert_eq!(f.states[0][1][2], 1);
    assert_eq!([f.states[0][0][1], f.states[0][1][1]], [0, 0]);
}

#[test]
fn late_refusals_commit_neither_states_nor_prepared_markers() {
    for height_error in [true, false] {
        let mut f = Fixture::new(15);
        f.flags[0] = Some([[4; 32]; 32]);
        f.prepared[3] = 255;
        if height_error {
            f.heights[2] = [[f32::NAN; 33]; 33];
        } else {
            f.overrides[2] = f32::NAN;
        }
        let before = f.states;
        let markers = f.prepared;
        let want = if height_error {
            SplitPreparationError::Height {
                context: 2,
                tree: 0,
                node: 1023,
                operand: NonFiniteOperand::FirstEndpoint,
            }
        } else {
            SplitPreparationError::Tolerance {
                context: 2,
                field: ToleranceField::Override,
            }
        };
        assert_eq!(f.run(), Err(want));
        assert_eq!(f.states, before);
        assert_eq!(f.prepared, markers);
    }
}

#[test]
fn tolerance_is_selected_before_forcing_and_only_used_controls_are_checked() {
    let mut f = Fixture::new(1);
    f.flags[0] = Some([[4; 32]; 32]);
    f.global = f32::NAN;
    let before = f.states;
    assert_eq!(
        f.run(),
        Err(SplitPreparationError::Tolerance {
            context: 0,
            field: ToleranceField::Global
        })
    );
    assert_eq!(f.states, before);
    assert_eq!(f.prepared[0], 0);
    f.overrides[0] = 2.0;
    f.run().unwrap();
    let mut f = Fixture::new(1);
    f.local[0] = f32::NAN;
    f.run().unwrap();
    let mut f = Fixture::new(0);
    f.global = f32::NAN;
    f.run().unwrap();
}

#[test]
fn negative_tolerance_does_not_bypass_used_height_validation() {
    let mut f = Fixture::new(1);
    f.global = -1.0;
    f.peak(f32::NAN);
    assert_eq!(
        f.run(),
        Err(SplitPreparationError::Height {
            context: 0,
            tree: 0,
            node: 1023,
            operand: NonFiniteOperand::MidpointSample
        })
    );
    assert_eq!(f.prepared[0], 0);
}
