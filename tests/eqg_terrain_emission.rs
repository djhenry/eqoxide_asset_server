use eqoxide_asset_server::eqg::{
    terrain_arithmetic::NonFiniteOperand,
    terrain_dependencies::Q32SplitStates,
    terrain_emission::{emit_53rz, MidpointError, Q32Emission},
    terrain_hierarchy::Q32Hierarchy,
    terrain_split_stage::Q32HeightGrid,
};

fn corner_grid() -> Q32HeightGrid {
    let mut h = [[100.0; 33]; 33];
    h[32][0] = 2.0;
    h[0][32] = 6.0;
    h[0][0] = 10.0;
    h[32][32] = 14.0;
    h
}
fn assert_height_bits(got: &Q32HeightGrid, want: &Q32HeightGrid) {
    for y in 0..33 {
        for x in 0..33 {
            assert_eq!(got[y][x].to_bits(), want[y][x].to_bits(), "height({x},{y})");
        }
    }
}

#[test]
fn unsplit_roots_emit_in_order_and_reconcile_the_complete_working_grid() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let states = [[0; 2048]; 2];
    let mut heights = corner_grid();
    let output = emit_53rz(&hierarchy, &states, &mut heights).unwrap();
    assert_eq!(output.triangles(), [[1056, 32, 0], [32, 1056, 1088]]);
    let expected = std::array::from_fn(|y| {
        std::array::from_fn(|x| {
            if x + y <= 32 {
                (80 - x as i32 - 2 * y as i32) as f32 / 8.0
            } else {
                (3 * x as i32 + 2 * y as i32 - 48) as f32 / 8.0
            }
        })
    });
    assert_height_bits(&heights, &expected);
}

#[test]
fn fully_split_nodes_never_read_heights_and_only_zero_children_emit() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let mut states: Q32SplitStates = [[1; 2048]; 2];
    let heights = [[f32::from_bits(0x7fc1_2345); 33]; 33];
    for tree in &mut states {
        tree[1024..].fill(0);
    }
    let mut actual = heights;
    let output = emit_53rz(&hierarchy, &states, &mut actual).unwrap();
    assert_eq!(output.triangles().len(), 2048);
    let mut cells = [[0; 32]; 32];
    for &[p, q, r] in output.triangles() {
        let [px, py] = [i32::from(p % 33), i32::from(p / 33)];
        let [qx, qy] = [i32::from(q % 33), i32::from(q / 33)];
        let [rx, ry] = [i32::from(r % 33), i32::from(r / 33)];
        assert_eq!((qx - px) * (ry - py) - (qy - py) * (rx - px), -1);
        cells[py.min(qy).min(ry) as usize][px.min(qx).min(rx) as usize] += 1;
    }
    assert!(cells.iter().flatten().all(|&count| count == 2));
    assert_height_bits(&actual, &heights);
    for raw_leaf in [1, 2, 128, 255] {
        for tree in &mut states {
            tree[1024..].fill(raw_leaf);
        }
        let output = emit_53rz(&hierarchy, &states, &mut actual).unwrap();
        assert!(output.triangles().is_empty());
        assert_height_bits(&actual, &heights);
    }
}

#[test]
fn asymmetric_splits_pin_node_tree_child_and_corner_order() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let mut states = [[0; 2048]; 2];
    states[0][1] = 1;
    states[1][1] = 1;
    states[0][2] = 1;
    let output = emit_53rz(&hierarchy, &states, &mut corner_grid()).unwrap();
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
}

#[test]
fn tree_interleaving_observes_the_other_roots_prior_midpoint_store() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let mut states = [[0; 2048]; 2];
    states[0][1] = 1;
    let mut heights = corner_grid();
    let output = emit_53rz(&hierarchy, &states, &mut heights).unwrap();
    assert_eq!(
        output.triangles(),
        [[0, 1056, 544], [32, 0, 544], [32, 1056, 1088]]
    );
    assert_eq!(heights[8][8].to_bits(), 7.0_f32.to_bits());
}

#[test]
fn midpoint_store_keeps_extended_sum_and_rounds_destination_toward_zero() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let mut states = [[1; 2048]; 2];
    states[0][1] = 0;
    for (p, q, midpoint) in [
        (0x7f7f_ffff, 0x7f7f_ffff, 0x7f7f_ffff),
        (0x3f80_0001, 0x3f80_0002, 0x3f80_0001),
        (0xbf80_0001, 0xbf80_0002, 0xbf80_0001),
        (0x3f80_0000, 0x8000_0001, 0x3eff_ffff),
        (0x8000_0000, 0x8000_0000, 0x8000_0000),
        (0x8000_0001, 0, 0x8000_0000),
    ] {
        let mut heights = [[f32::NAN; 33]; 33];
        heights[32][0] = f32::from_bits(p);
        heights[0][32] = f32::from_bits(q);
        let mut expected = heights;
        expected[16][16] = f32::from_bits(midpoint);
        let output = emit_53rz(&hierarchy, &states, &mut heights).unwrap();
        assert_eq!(output.triangles(), [[1056, 32, 0]]);
        assert_height_bits(&heights, &expected);
    }
}

#[test]
fn late_used_endpoint_refusal_rolls_back_all_prior_height_stores() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let mut states = [[0; 2048]; 2];
    states[0][1] = 1;
    states[1][1] = 1;
    let mut heights = corner_grid();
    heights[16][16] = f32::NAN;
    let before = heights;
    assert_eq!(
        emit_53rz(&hierarchy, &states, &mut heights).unwrap_err(),
        MidpointError {
            tree: 0,
            node: 4,
            operand: NonFiniteOperand::SecondEndpoint,
        }
    );
    assert_height_bits(&heights, &before);
    let mut heights = before;
    emit_53rz(&hierarchy, &[[0; 2048]; 2], &mut heights).unwrap();
    assert_eq!(heights[16][16], 4.0);
}

#[test]
fn nonclosed_raw_states_can_emit_more_than_a_valid_mesh_capacity() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let mut states = [[0; 2048]; 2];
    for tree in &mut states {
        for (start, end) in [(2, 4), (8, 16), (32, 64), (128, 256), (512, 1024)] {
            tree[start..end].fill(1);
        }
    }
    let output = emit_53rz(&hierarchy, &states, &mut [[0.0; 33]; 33]).unwrap();
    assert_eq!(output.triangles().len(), 2730);
    assert_eq!(Q32Emission::MAX_TRIANGLES, 4094);
}

#[test]
fn unselected_descendants_still_update_the_working_grid() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let mut states = [[1; 2048]; 2];
    states[0][1] = 0;
    states[0][2] = 0;
    states[0][4] = 0;
    states[1][1] = 0;
    let mut heights = corner_grid();
    let output = emit_53rz(&hierarchy, &states, &mut heights).unwrap();
    assert_eq!(output.triangles(), [[1056, 32, 0], [32, 1056, 1088]]);
    assert_eq!(heights[8][8], 7.0);
}
