use eqoxide_asset_server::eqg::terrain_hierarchy::Q32Hierarchy;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn ordered_roots_and_children_preserve_source_grid_corners() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let expected = [
        [
            [[0, 32], [32, 0], [0, 0]],
            [[32, 0], [0, 0], [16, 16]],
            [[0, 0], [0, 32], [16, 16]],
            [[0, 0], [16, 16], [16, 0]],
            [[16, 16], [32, 0], [16, 0]],
            [[0, 32], [16, 16], [0, 16]],
            [[16, 16], [0, 0], [0, 16]],
        ],
        [
            [[32, 0], [0, 32], [32, 32]],
            [[0, 32], [32, 32], [16, 16]],
            [[32, 32], [32, 0], [16, 16]],
            [[32, 32], [16, 16], [16, 32]],
            [[16, 16], [0, 32], [16, 32]],
            [[32, 0], [16, 16], [32, 16]],
            [[16, 16], [32, 32], [32, 16]],
        ],
    ];
    for (tree, nodes) in expected.iter().enumerate() {
        for (index, corners) in nodes.iter().enumerate() {
            assert_eq!(hierarchy.node(tree, index + 1), Some(*corners));
        }
    }
}

#[test]
fn lookup_rejects_invalid_trees_nodes_and_nonleaf_quad_requests() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    for tree in [0, 1] {
        for index in [0, 2048, usize::MAX] {
            assert_eq!(hierarchy.node(tree, index), None);
            assert_eq!(hierarchy.leaf_quad(tree, index), None);
        }
        for index in [1, 2, 1023] {
            assert!(hierarchy.node(tree, index).is_some());
            assert_eq!(hierarchy.leaf_quad(tree, index), None);
        }
        for index in [1024, 2047] {
            assert!(hierarchy.node(tree, index).is_some());
            assert!(hierarchy.leaf_quad(tree, index).is_some());
        }
        assert!((1..2048).all(|index| hierarchy.node(tree, index).is_some()));
    }
    for tree in [2, usize::MAX] {
        for index in [0, 1, 1024, 2047, usize::MAX] {
            assert_eq!(hierarchy.node(tree, index), None);
            assert_eq!(hierarchy.leaf_quad(tree, index), None);
        }
    }
}

#[test]
fn leaves_partition_every_quad_into_two_clockwise_unit_triangles() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let mut cells = BTreeMap::<[u16; 2], Vec<BTreeSet<[u16; 2]>>>::new();
    for tree in [0, 1] {
        for index in 1024..2048 {
            let corners = hierarchy.node(tree, index).unwrap();
            let min_x = corners.iter().map(|p| p[0]).min().unwrap();
            let min_y = corners.iter().map(|p| p[1]).min().unwrap();
            let max_x = corners.iter().map(|p| p[0]).max().unwrap();
            let max_y = corners.iter().map(|p| p[1]).max().unwrap();
            assert!(max_x <= 32 && max_y <= 32);
            assert_eq!([max_x - min_x, max_y - min_y], [1, 1]);
            assert_eq!(hierarchy.leaf_quad(tree, index), Some([min_x, min_y]));
            let [p, q, r] = corners.map(|p| p.map(i32::from));
            let area = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
            assert_eq!(area, -1);
            let points = BTreeSet::from(corners);
            assert_eq!(points.len(), 3);
            cells.entry([min_x, min_y]).or_default().push(points);
        }
    }
    assert_eq!(cells.len(), 1024);
    for x in 0..32 {
        for y in 0..32 {
            let pair = &cells[&[x, y]];
            assert_eq!(pair.len(), 2);
            let common: Vec<_> = pair[0].intersection(&pair[1]).collect();
            assert_eq!(common.len(), 2);
            assert_eq!(common[0][0].abs_diff(common[1][0]), 1);
            assert_eq!(common[0][1].abs_diff(common[1][1]), 1);
        }
    }
}

#[test]
fn every_depth_preserves_clockwise_area_and_exact_parent_midpoints() {
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    // Fixed-grid doubled areas halve at every bisection, through half-quad leaves.
    let areas_by_depth = [-1024, -512, -256, -128, -64, -32, -16, -8, -4, -2, -1];
    for tree in [0, 1] {
        for node in 1_usize..2048 {
            let corners = hierarchy.node(tree, node).unwrap();
            assert!(corners.iter().flatten().all(|&coordinate| coordinate <= 32));
            let [p, q, r] = corners.map(|point| point.map(i32::from));
            let area = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
            let depth = node.ilog2() as usize;
            assert_eq!(area, areas_by_depth[depth], "tree {tree}, node {node}");
            if node < 1024 {
                // Even coordinate sums make halving exact, not truncation.
                let sum = [p[0] + q[0], p[1] + q[1]];
                assert_eq!(sum.map(|coordinate| coordinate % 2), [0, 0]);
                let midpoint = sum.map(|coordinate| (coordinate / 2) as u16);
                let [p, q, r] = corners;
                assert_eq!(hierarchy.node(tree, node * 2), Some([q, r, midpoint]));
                assert_eq!(hierarchy.node(tree, node * 2 + 1), Some([r, p, midpoint]));
            }
        }
    }
}
