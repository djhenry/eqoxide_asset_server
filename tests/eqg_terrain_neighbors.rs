use eqoxide_asset_server::eqg::terrain_hierarchy::Q32Hierarchy;
use eqoxide_asset_server::eqg::terrain_neighbors::{NeighborNode, Q32Neighbors};
use std::collections::BTreeMap;

fn key(context: u8, tree: u8, node: u16) -> NeighborNode {
    NeighborNode {
        context,
        tree,
        node,
    }
}

#[test]
fn roots_and_first_vs_deeper_children_keep_their_distinct_links() {
    let graph = Q32Neighbors::try_new().unwrap();
    let roots = [
        [None, None, Some(key(0, 1, 1))],
        [Some(key(1, 0, 1)), Some(key(2, 0, 1)), Some(key(0, 0, 1))],
        [Some(key(0, 1, 1)), None, Some(key(1, 1, 1))],
        [None, Some(key(3, 0, 1)), Some(key(1, 0, 1))],
        [None, Some(key(0, 1, 1)), Some(key(2, 1, 1))],
        [Some(key(3, 0, 1)), None, Some(key(2, 0, 1))],
        [Some(key(2, 1, 1)), Some(key(1, 1, 1)), Some(key(3, 1, 1))],
        [None, None, Some(key(3, 0, 1))],
    ];
    for (index, links) in roots.into_iter().enumerate() {
        assert_eq!(graph.neighbors(index / 2, index % 2, 1), Some(links));
    }
    assert_eq!(
        graph.neighbors(0, 1, 2),
        Some([Some(key(0, 0, 3)), Some(key(0, 1, 3)), Some(key(2, 0, 2))])
    );
    assert_eq!(graph.neighbors(0, 1, 4).unwrap()[2], Some(key(0, 1, 7)));
    assert_eq!(graph.neighbors(0, 1, 5).unwrap()[2], Some(key(0, 0, 6)));
}

#[test]
fn invalid_lookup_differs_from_a_valid_window_boundary() {
    let graph = Q32Neighbors::try_new().unwrap();
    assert_eq!(graph.neighbors(0, 0, 1).unwrap()[0], None);
    for context in [4, usize::MAX] {
        assert_eq!(graph.neighbors(context, 0, 1), None);
    }
    for tree in [2, usize::MAX] {
        assert_eq!(graph.neighbors(0, tree, 1), None);
    }
    for node in [0, 2048, usize::MAX] {
        assert_eq!(graph.neighbors(0, 0, node), None);
    }
    for context in 0..4 {
        for tree in 0..2 {
            assert!(graph.neighbors(context, tree, 2047).is_some());
        }
    }
}

#[test]
fn every_template_link_matches_a_whole_geometric_edge_at_its_depth() {
    let graph = Q32Neighbors::try_new().unwrap();
    let hierarchy = Q32Hierarchy::try_new().unwrap();
    let origins = [[0, 0], [32, 0], [0, 32], [32, 32]];
    type Edge = (u32, [[i32; 2]; 2]);
    let mut edges = BTreeMap::<Edge, Vec<(NeighborNode, usize)>>::new();
    for (context, origin) in origins.into_iter().enumerate() {
        for tree in 0..2 {
            for node in 1_usize..Q32Hierarchy::NODE_END {
                let depth = usize::BITS - 1 - node.leading_zeros();
                let [p, q, r] = hierarchy
                    .node(tree, node)
                    .unwrap()
                    .map(|p| [i32::from(p[0]) + origin[0], i32::from(p[1]) + origin[1]]);
                for (edge, mut points) in [[r, p], [q, r], [p, q]].into_iter().enumerate() {
                    points.sort();
                    edges
                        .entry((depth, points))
                        .or_default()
                        .push((key(context as u8, tree as u8, node as u16), edge));
                }
            }
        }
    }
    let mut slots = 0;
    let mut boundaries = 0;
    for incident in edges.values() {
        assert!((1..=2).contains(&incident.len()));
        for &(node, edge) in incident {
            let other = incident.iter().find(|(candidate, _)| *candidate != node);
            let expected = other.map(|(candidate, _)| *candidate);
            assert_eq!(
                graph
                    .neighbors(node.context.into(), node.tree.into(), node.node.into())
                    .unwrap()[edge],
                expected,
                "{node:?} edge {edge}"
            );
            slots += 1;
            boundaries += usize::from(expected.is_none());
        }
    }
    // Four contexts, two 2047-node trees and three edge slots per node.
    assert_eq!(slots, 4 * 2 * 2047 * 3);
    // The window perimeter has eight tile edges. At depth d, each has
    // 2^(floor(d/2)) segments; sum the counts from depth 0 through depth 10.
    let expected_boundaries = 8 * (0..=10).map(|depth| 1_usize << (depth / 2)).sum::<usize>();
    assert_eq!(boundaries, expected_boundaries);
}

#[test]
fn wide_invalid_indices_do_not_alias_valid_template_records() {
    let graph = Q32Neighbors::try_new().unwrap();
    assert!(graph.neighbors(0, 0, 1).is_some());
    assert!(graph.neighbors(3, 1, 2047).is_some());
    // These invalid usize inputs become valid indices if narrowed first.
    for [context, tree, node] in [
        [256, 0, 1],
        [257, 1, 1024],
        [259, 1, 2047],
        [0, 256, 1],
        [3, 257, 2047],
        [0, 0, 65537],
        [3, 1, 66560],
        [3, 1, 67583],
    ] {
        assert_eq!(
            graph.neighbors(context, tree, node),
            None,
            "context {context}, tree {tree}, node {node}"
        );
    }
}
