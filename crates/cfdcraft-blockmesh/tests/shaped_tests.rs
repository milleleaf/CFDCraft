use cfdcraft_blockmesh::{Mesh2d, build};
use cfdcraft_shapes::grading::{one_sided, uniform};
use cfdcraft_shapes::{
    ArcEdge, Block, DomainSpec, Layout, MeshRequest, Quad, ShapeSpec, Side, layout,
};
use std::collections::{HashMap, HashSet};

const CX: f64 = 0.2;
const CY: f64 = 0.2;
const D: f64 = 0.1;

fn request(cells_x: u64, cells_y: u64, level: u32) -> MeshRequest {
    MeshRequest {
        cells_x,
        cells_y,
        cells_z: 1,
        level,
        ratio_x: None,
        ratio_y: None,
    }
}

fn step(level: u32, startup_length: f64) -> Layout {
    let domain = DomainSpec {
        length_x: 2.286,
        length_y: 0.1143,
        length_z: 0.0127,
    };
    let shape = ShapeSpec::BackwardStep {
        step_height: 0.0127,
        upstream_length: 1.651,
        startup_length,
    };
    layout(&domain, &shape, &request(220, 114, level)).unwrap()
}

fn cylinder(level: u32) -> Layout {
    let domain = DomainSpec {
        length_x: 2.2,
        length_y: 0.41,
        length_z: 0.1,
    };
    let shape = ShapeSpec::CylinderChannel {
        diameter: D,
        centre_x: CX,
        centre_y: CY,
    };
    layout(&domain, &shape, &request(114, 44, level)).unwrap()
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

/// The count, the area and the orientation of the mesh against its layout.
fn check_areas(l: &Layout, mesh: &Mesh2d) {
    assert_eq!(mesh.quads.len() as u64, l.cell_count);
    let areas: Vec<f64> = mesh.quads.iter().map(|&q| signed_area(mesh, q)).collect();
    assert!(
        areas.iter().all(|&a| a > 1e-12),
        "a quad is flat or clockwise"
    );
    let total: f64 = areas.iter().sum();
    let want = l.meshed_area();
    assert!(
        (total - want).abs() <= 1e-9 * want,
        "{total} against {want}"
    );
}

/// Every edge is in two quads inside and one on the boundary, and the boundary groups hold exactly
/// the edges that are in one quad, each run with its cell on the left.
fn check_edges(mesh: &Mesh2d) {
    let mut counts: HashMap<(u32, u32), u32> = HashMap::new();
    let mut directed = HashSet::new();
    for q in &mesh.quads {
        for k in 0..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            *counts.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            directed.insert((a, b));
        }
    }
    assert!(counts.values().all(|&c| c == 1 || c == 2));
    let mut grouped = HashSet::new();
    for g in &mesh.boundary {
        for &[a, b] in &g.edges {
            assert_eq!(counts[&(a.min(b), a.max(b))], 1, "edge {a} {b} is inside");
            assert!(
                directed.contains(&(a, b)),
                "edge {a} {b} runs the wrong way"
            );
            assert!(grouped.insert((a.min(b), a.max(b))), "edge {a} {b} twice");
        }
    }
    assert_eq!(grouped.len(), counts.values().filter(|&&c| c == 1).count());
}

fn group(mesh: &Mesh2d, side: Side) -> Option<&[[u32; 2]]> {
    mesh.boundary
        .iter()
        .find(|g| g.side == side)
        .map(|g| g.edges.as_slice())
}

#[test]
fn the_step_mesh_has_the_layout_cell_count_and_area() {
    for startup in [0.0, 0.5] {
        for level in [0, 1] {
            let l = step(level, startup);
            check_areas(&l, &build(&l).unwrap());
        }
    }
}

/// The cells along the circle: the first-direction cells of the block whose inner edge each arc
/// of the lower plane lies on.
fn obstacle_cells(l: &Layout) -> usize {
    l.arcs
        .iter()
        .filter(|a| a.through[2].abs() <= 1e-12)
        .map(|a| {
            l.blocks
                .iter()
                .find(|b| {
                    let (p, q) = (b.vertices[3], b.vertices[2]);
                    (p == a.from && q == a.to) || (p == a.to && q == a.from)
                })
                .map_or(0, |b| b.cells[0] as usize)
        })
        .sum()
}

#[test]
fn the_cylinder_channel_mesh_follows_the_arcs() {
    let l = cylinder(0);
    let mesh = build(&l).unwrap();
    check_areas(&l, &mesh);
    let edges = group(&mesh, Side::Obstacle).unwrap();
    assert_eq!(edges.len(), obstacle_cells(&l));
    assert!(!edges.is_empty());
    let r = D / 2.0;
    for &[a, b] in edges {
        for id in [a, b] {
            let n = mesh.nodes[id as usize];
            let dist = (n[0] - CX).hypot(n[1] - CY);
            assert!(
                (dist - r).abs() <= 1e-9 * r,
                "node {id} is {dist} from the axis"
            );
        }
    }
}

#[test]
fn interior_edges_are_shared_twice_on_the_shapes_too() {
    for level in [0, 1] {
        check_edges(&build(&step(level, 0.0)).unwrap());
        check_edges(&build(&step(level, 0.5)).unwrap());
        check_edges(&build(&cylinder(level)).unwrap());
    }
}

#[test]
fn the_obstacle_and_startup_groups_exist_where_the_layout_has_them() {
    let sides = |m: &Mesh2d| m.boundary.iter().map(|g| g.side).collect::<Vec<_>>();
    let with = build(&step(0, 0.5)).unwrap();
    assert!(group(&with, Side::Startup).is_some_and(|e| !e.is_empty()));
    assert!(group(&with, Side::Obstacle).is_none());
    let without = build(&step(0, 0.0)).unwrap();
    assert!(group(&without, Side::Startup).is_none());
    let cyl = build(&cylinder(0)).unwrap();
    assert!(group(&cyl, Side::Obstacle).is_some_and(|e| !e.is_empty()));
    assert!(group(&cyl, Side::Startup).is_none());
    let domain = DomainSpec {
        length_x: 1.0,
        length_y: 1.0,
        length_z: 0.25,
    };
    let l = layout(&domain, &ShapeSpec::Box, &request(4, 4, 0)).unwrap();
    let plain = build(&l).unwrap();
    assert_eq!(
        sides(&plain),
        [Side::XMin, Side::XMax, Side::YMin, Side::YMax]
    );
    assert_eq!(
        sides(&with),
        [
            Side::XMin,
            Side::XMax,
            Side::YMin,
            Side::YMax,
            Side::Startup
        ]
    );
}

#[test]
fn a_curved_edge_is_placed_by_arc_length_grading() {
    // A unit square one cell thick whose top edge is an arc of the circle about (0.5, 0.5), graded
    // along x so each cell is 1.25 times the one before.
    let rim = 0.5 + 0.5_f64.sqrt();
    let mut vertices = Vec::new();
    for z in [0.0, 0.25] {
        for c in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            vertices.push([c[0], c[1], z]);
        }
    }
    let quad = |side, vertices| Quad { side, vertices };
    let l = Layout {
        vertices,
        blocks: vec![Block {
            vertices: [0, 1, 2, 3, 4, 5, 6, 7],
            cells: [4, 3, 1],
            grading: [one_sided(4, 1.25, true), uniform(), uniform()],
        }],
        arcs: vec![ArcEdge {
            from: 3,
            to: 2,
            through: [0.5, rim, 0.0],
        }],
        quads: vec![
            quad(Side::XMin, [0, 4, 7, 3]),
            quad(Side::XMax, [1, 2, 6, 5]),
            quad(Side::YMin, [0, 1, 5, 4]),
            quad(Side::YMax, [3, 7, 6, 2]),
        ],
        cell_count: 12,
        area: 1.0,
    };
    let mesh = build(&l).unwrap();
    // The top group runs from the right corner to the left one.
    let top = group(&mesh, Side::YMax).unwrap();
    let mut ids: Vec<u32> = top.iter().map(|e| e[0]).collect();
    ids.push(top[top.len() - 1][1]);
    ids.reverse();
    assert_eq!(ids.len(), 5);
    let angles: Vec<f64> = ids
        .iter()
        .map(|&id| {
            let n = mesh.nodes[id as usize];
            (n[1] - 0.5).atan2(n[0] - 0.5)
        })
        .collect();
    let steps: Vec<f64> = angles.windows(2).map(|w| w[0] - w[1]).collect();
    for w in steps.windows(2) {
        let ratio = w[1] / w[0];
        assert!((ratio / 1.25 - 1.0).abs() <= 1e-6, "ratio {ratio}");
    }
    // The nodes are on the circle, and the cells above the arc are the ones it bounds.
    for &id in &ids {
        let n = mesh.nodes[id as usize];
        let dist = (n[0] - 0.5).hypot(n[1] - 0.5);
        assert!((dist - 0.5_f64.sqrt()).abs() <= 1e-9, "{dist}");
    }
    assert!(mesh.quads.iter().all(|&q| signed_area(&mesh, q) > 1e-12));
}
