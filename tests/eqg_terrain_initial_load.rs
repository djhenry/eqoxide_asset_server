use eqoxide_asset_server::eqg::{
    terrain_hierarchy::Q32Hierarchy,
    terrain_initial_load::{
        prepare_initial_load_53rz, InitialLoadError, Q32LoadLimits, Q32TileInput,
    },
    terrain_neighbors::Q32Neighbors,
    terrain_preparation::{prepare_self_53rz, Q32WorkingView},
};

fn tile(coordinate: [i32; 2], height: f32) -> Q32TileInput {
    Q32TileInput {
        coordinate,
        heights: [[height; 33]; 33],
        flags: [[0; 32]; 32],
    }
}
fn limits() -> Q32LoadLimits {
    Q32LoadLimits::default()
}

#[test]
fn empty_is_only_an_empty_supplied_set() {
    assert!(prepare_initial_load_53rz(&[], &limits())
        .unwrap()
        .is_empty());
}

#[test]
fn signed_order_sparse_membership_and_full_pair_identity() {
    let input = vec![tile([1, -1000], 3.0), tile([-1, 5], 7.0), tile([0, 0], 2.0)];
    let output = prepare_initial_load_53rz(&input, &limits()).unwrap();
    assert_eq!(
        output.iter().map(|t| t.coordinate()).collect::<Vec<_>>(),
        [[-1, 5], [0, 0], [1, -1000]]
    );
    for (t, expected) in output.iter().zip([7.0, 2.0, 3.0]) {
        assert_eq!(t.prepared(), 1);
        assert_eq!(t.heights()[0][0], expected);
        assert_eq!(t.triangles(), [[1056, 32, 0], [32, 1056, 1088]]);
    }
    let reordered = vec![input[2].clone(), input[0].clone(), input[1].clone()];
    let second = prepare_initial_load_53rz(&reordered, &limits()).unwrap();
    for (a, b) in output.iter().zip(&second) {
        assert_eq!(a.coordinate(), b.coordinate());
        assert_eq!(
            a.heights().map(|r| r.map(f32::to_bits)),
            b.heights().map(|r| r.map(f32::to_bits))
        );
        assert_eq!(a.flags(), b.flags());
        assert_eq!(a.states(), b.states());
        assert_eq!(a.triangles(), b.triangles());
    }
}

#[test]
fn all_seams_finish_before_any_update_and_north_wins_corner() {
    let mut a = tile([0, 0], 0.0);
    let mut n = tile([0, 1], 20.0);
    let mut e = tile([1, 0], 10.0);
    let mut ne = tile([1, 1], 30.0);
    for t in [&mut a, &mut n, &mut e, &mut ne] {
        t.flags = [[4; 32]; 32];
    }
    n.heights[0][32] = 123.0;
    e.heights[32][0] = 456.0;
    ne.heights[0][0] = 789.0;
    let input = vec![ne, a, e, n];
    let output = prepare_initial_load_53rz(&input, &limits()).unwrap();
    let first = &output[0];
    assert_eq!(first.coordinate(), [0, 0]);
    assert_eq!(first.heights()[32][32], 123.0);
    assert_eq!(first.heights()[12][32], 10.0);
    assert_eq!(first.heights()[32][12], 20.0);
    assert_eq!(output[1].heights()[0][32], 789.0);
    assert_eq!(first.triangles().len(), 2048);
    assert_eq!(input[3].heights[0][32], 123.0);
    assert_eq!(input[1].heights[32][32], 0.0);
}

#[test]
fn shared_preparation_reuses_neighbor_state_instead_of_resetting_each_self() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let topology = Q32Neighbors::try_new().unwrap();
    let mut input = vec![tile([0, 0], 0.0), tile([1, 0], 0.0)];
    input[0].heights[16][16] = 16.0;
    input[1].heights[9][7] = 5.0;
    input[1].flags[5][4] = 1;
    let output = prepare_initial_load_53rz(&input, &limits()).unwrap();
    let mut a = input[0].clone();
    let mut e = input[1].clone();
    for y in 0..33 {
        a.heights[y][32] = e.heights[y][0];
    }
    let mut sa = [[0; 2048]; 2];
    let mut se = sa;
    let mut pa = 0;
    let mut pe = 0;
    let mut view = Q32WorkingView {
        heights: &mut a.heights,
        flags: &mut a.flags,
        states: &mut sa,
        prepared: &mut pa,
        local_tolerance: 10.0,
        override_tolerance: 4294967296.0,
    };
    let ea = prepare_self_53rz(
        &hierarchy,
        &topology,
        1.0,
        &mut view,
        &mut [
            Some(Q32WorkingView {
                heights: &mut e.heights,
                flags: &mut e.flags,
                states: &mut se,
                prepared: &mut pe,
                local_tolerance: 10.0,
                override_tolerance: 4294967296.0,
            }),
            None,
            None,
        ],
    )
    .unwrap();
    assert_eq!(pe, 1);
    let ee = prepare_self_53rz(
        &hierarchy,
        &topology,
        1.0,
        &mut Q32WorkingView {
            heights: &mut e.heights,
            flags: &mut e.flags,
            states: &mut se,
            prepared: &mut pe,
            local_tolerance: 10.0,
            override_tolerance: 4294967296.0,
        },
        &mut [None, None, None],
    )
    .unwrap();
    assert_eq!(output[0].states(), &sa);
    assert_eq!(output[1].states(), &se);
    assert_eq!(output[0].triangles(), ea.triangles());
    assert_eq!(output[1].triangles(), ee.triangles());
    assert_eq!(output[0].flags(), &a.flags);
    assert_eq!(output[1].flags(), &e.flags);
    assert_eq!(output[0].heights(), &a.heights);
    assert_eq!(output[1].heights(), &e.heights);
}

#[test]
fn duplicate_bounds_and_tile_budget_are_explicit_refusals() {
    assert!(matches!(
        prepare_initial_load_53rz(&[tile([0, 0], 0.0), tile([0, 0], 1.0)], &limits()),
        Err(InitialLoadError::Duplicate { coordinate: [0, 0] })
    ));
    assert!(matches!(
        prepare_initial_load_53rz(&[tile([200001, 0], 0.0)], &limits()),
        Err(InitialLoadError::Coordinate {
            coordinate: [200001, 0]
        })
    ));
    assert!(matches!(
        prepare_initial_load_53rz(
            &[tile([0, 0], 0.0)],
            &Q32LoadLimits {
                max_tiles: 0,
                ..limits()
            }
        ),
        Err(InitialLoadError::TileLimit {
            requested: 1,
            limit: 0
        })
    ));
    let output = prepare_initial_load_53rz(&[tile([-200000, 200000], 0.0)], &limits()).unwrap();
    assert_eq!(output[0].coordinate(), [-200000, 200000]);
}

#[test]
fn late_refusal_keeps_all_input_bits_and_reports_actual_update() {
    let mut input = vec![tile([0, 0], 2.0), tile([9, 9], 3.0)];
    input[1].heights[0][32] = f32::from_bits(0x7fc01234);
    let before = input
        .iter()
        .map(|t| (t.heights.map(|r| r.map(f32::to_bits)), t.flags))
        .collect::<Vec<_>>();
    assert!(matches!(
        prepare_initial_load_53rz(&input, &limits()),
        Err(InitialLoadError::Preparation {
            coordinate: [9, 9],
            ..
        })
    ));
    let after = input
        .iter()
        .map(|t| (t.heights.map(|r| r.map(f32::to_bits)), t.flags))
        .collect::<Vec<_>>();
    assert_eq!(before, after);
    input[1].heights[0][32] = 3.0;
    assert!(matches!(
        prepare_initial_load_53rz(
            &input,
            &Q32LoadLimits {
                max_triangles: 3,
                ..limits()
            }
        ),
        Err(InitialLoadError::TriangleLimit {
            coordinate: [9, 9],
            requested: 4,
            limit: 3
        })
    ));
    assert_eq!(
        prepare_initial_load_53rz(
            &input,
            &Q32LoadLimits {
                max_triangles: 4,
                ..limits()
            }
        )
        .unwrap()
        .len(),
        2
    );
}

#[test]
fn actual_empty_emission_fits_zero_triangle_budget() {
    let mut input = tile([0, 0], 4.0);
    input.flags = [[0xff; 32]; 32];
    let output = prepare_initial_load_53rz(
        &[input],
        &Q32LoadLimits {
            max_triangles: 0,
            ..limits()
        },
    )
    .unwrap();
    assert!(output[0].triangles().is_empty());
    assert_eq!(output[0].flags(), &[[0x7f; 32]; 32]);
}
