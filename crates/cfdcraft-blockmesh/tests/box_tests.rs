use cfdcraft_blockmesh::{BlockMeshError, MAX_CELLS, Mesh2d, build};
use cfdcraft_shapes::{DomainSpec, Layout, MeshRequest, ShapeSpec, Side, layout};
use std::collections::{HashMap, HashSet};

fn box_layout(cells_x: u64, cells_y: u64, ratio: Option<f64>) -> Layout {
    let domain = DomainSpec {
        length_x: 1.0,
        length_y: 1.0,
        length_z: 0.25,
    };
    let request = MeshRequest {
        cells_x,
        cells_y,
        cells_z: 1,
        level: 0,
        ratio_x: ratio,
        ratio_y: ratio,
    };
    layout(&domain, &ShapeSpec::Box, &request).unwrap()
}

fn signed_area(mesh: &Mesh2d, q: [u32; 4]) -> f64 {
    let p = q.map(|id| mesh.nodes[id as usize]);
    let mut twice = 0.0;
    for k in 0..4 {
        let (a, b) = (p[k], p[(k + 1) % 4]);
        twice += a[0] * b[1] - b[0] * a[1];
    }
    twice / 2.0
}

/// How many quads each undirected edge belongs to.
fn edge_counts(mesh: &Mesh2d) -> HashMap<(u32, u32), u32> {
    let mut counts = HashMap::new();
    for q in &mesh.quads {
        for k in 0..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            *counts.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    counts
}

#[test]
fn a_4x4_box_has_25_nodes_and_16_quads() {
    let mesh = build(&box_layout(4, 4, None)).unwrap();
    assert_eq!(mesh.nodes.len(), 25);
    assert_eq!(mesh.quads.len(), 16);
    for j in 0..=4 {
        for i in 0..=4 {
            let want = [f64::from(i) / 4.0, f64::from(j) / 4.0];
            let found = mesh
                .nodes
                .iter()
                .any(|n| (n[0] - want[0]).abs() <= 1e-12 && (n[1] - want[1]).abs() <= 1e-12);
            assert!(found, "no node at {want:?}");
        }
    }
    assert!((mesh.thickness - 0.25).abs() <= 1e-12);
}

#[test]
fn every_quad_is_counter_clockwise_with_positive_area() {
    let mesh = build(&box_layout(4, 4, Some(1.5))).unwrap();
    let areas: Vec<f64> = mesh.quads.iter().map(|&q| signed_area(&mesh, q)).collect();
    assert_eq!(areas.len(), 16);
    assert!(areas.iter().all(|&a| a > 1e-12), "{areas:?}");
    // The cells next to the walls are the smallest: 0.2 by 0.2 against 0.25 by 0.25 uniform.
    let smallest = areas.iter().copied().fold(f64::INFINITY, f64::min);
    assert!((smallest - 0.04).abs() <= 1e-9, "{smallest}");
}

#[test]
fn a_clockwise_block_is_reordered() {
    let mut l = box_layout(4, 3, None);
    for v in &mut l.vertices {
        v[0] = 1.0 - v[0];
    }
    let mesh = build(&l).unwrap();
    assert_eq!(mesh.quads.len(), 12);
    assert!(mesh.quads.iter().all(|&q| signed_area(&mesh, q) > 1e-12));
    let edges: usize = mesh.boundary.iter().map(|g| g.edges.len()).sum();
    assert_eq!(edges, 14);
}

#[test]
fn edges_are_shared_twice_inside_and_once_on_the_boundary() {
    let (nx, ny) = (4u64, 3u64);
    let mesh = build(&box_layout(nx, ny, None)).unwrap();
    let counts = edge_counts(&mesh);
    assert!(counts.values().all(|&c| c == 1 || c == 2));
    let boundary = counts.values().filter(|&&c| c == 1).count() as u64;
    assert_eq!(boundary, 2 * (nx + ny));
}

#[test]
fn the_boundary_groups_follow_the_layout_sides() {
    let mesh = build(&box_layout(4, 3, None)).unwrap();
    let sides: Vec<Side> = mesh.boundary.iter().map(|g| g.side).collect();
    assert_eq!(sides, [Side::XMin, Side::XMax, Side::YMin, Side::YMax]);
    let lengths: Vec<usize> = mesh.boundary.iter().map(|g| g.edges.len()).collect();
    assert_eq!(lengths, [3, 3, 4, 4]);

    // Every boundary edge is in one group, and each is an edge of one quad run in the quad's own
    // direction, so the cell is on its left.
    let counts = edge_counts(&mesh);
    let directed: HashSet<(u32, u32)> = mesh
        .quads
        .iter()
        .flat_map(|q| (0..4).map(move |k| (q[k], q[(k + 1) % 4])))
        .collect();
    let mut seen = HashSet::new();
    for g in &mesh.boundary {
        for &[a, b] in &g.edges {
            assert_eq!(counts[&(a.min(b), a.max(b))], 1);
            assert!(
                directed.contains(&(a, b)),
                "edge {a} {b} runs the wrong way"
            );
            assert!(
                seen.insert((a.min(b), a.max(b))),
                "edge {a} {b} is in two groups"
            );
        }
    }
    assert_eq!(seen.len(), counts.values().filter(|&&c| c == 1).count());
}

#[test]
fn the_area_matches_the_layout() {
    for ratio in [None, Some(1.5)] {
        let l = box_layout(6, 4, ratio);
        let mesh = build(&l).unwrap();
        let total: f64 = mesh.quads.iter().map(|&q| signed_area(&mesh, q)).sum();
        let want = l.meshed_area();
        assert!(
            (total - want).abs() <= 1e-9 * want,
            "{total} against {want}"
        );
        assert_eq!(mesh.quads.len() as u64, l.cell_count);
    }
}

#[test]
fn a_layout_over_the_cell_limit_is_refused_before_any_allocation() {
    let l = Layout {
        vertices: Vec::new(),
        blocks: Vec::new(),
        arcs: Vec::new(),
        quads: Vec::new(),
        cell_count: 50_000_001,
        area: 0.0,
    };
    assert_eq!(MAX_CELLS, 50_000_000);
    assert_eq!(
        build(&l),
        Err(BlockMeshError::TooManyCells {
            cells: 50_000_001,
            limit: 50_000_000,
        })
    );
}
