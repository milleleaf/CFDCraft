//! Soundness checks on a layout, run before any mesher sees it.

use crate::{DomainSpec, Layout, ShapeSpec};
use std::collections::BTreeMap;

/// Relative tolerance of the area and volume checks.
const REL_TOL: f64 = 1e-9;

/// The three neighbours of each hexahedron corner, in the order the triple product is taken.
const HEX_NEIGHBOURS: [[usize; 3]; 8] = [
    [1, 3, 4],
    [2, 0, 5],
    [3, 1, 6],
    [0, 2, 7],
    [7, 5, 0],
    [4, 6, 1],
    [5, 7, 2],
    [6, 4, 3],
];

/// The six faces of a hexahedron as corner numbers: x-min, x-max, y-min, y-max, z-min, z-max.
const HEX_FACES: [[usize; 4]; 6] = [
    [0, 4, 7, 3],
    [1, 2, 6, 5],
    [0, 1, 5, 4],
    [3, 7, 6, 2],
    [0, 3, 2, 1],
    [4, 5, 6, 7],
];

/// What kind of defect a [`Problem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    /// A block has a corner whose edge vectors are not right-handed (inverted or flat).
    Jacobian,
    /// A block face is not shared once or on exactly one patch, or shared faces do not match.
    Coverage,
    /// The blocks do not add up to the domain area or volume.
    Area,
    /// The block cell counts do not add up to the layout's cell count.
    Count,
}

/// One defect found in a layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The kind of defect.
    pub kind: ProblemKind,
    /// The block it concerns, when it concerns one.
    pub block: Option<usize>,
    /// A sentence saying what is wrong.
    pub detail: String,
}

fn problem(kind: ProblemKind, block: Option<usize>, detail: String) -> Problem {
    Problem {
        kind,
        block,
        detail,
    }
}

/// The area the domain must have, from its size and its shape alone.
pub(crate) fn domain_area(domain: &DomainSpec, shape: &ShapeSpec) -> f64 {
    let full = domain.length_x * domain.length_y;
    match *shape {
        ShapeSpec::Box => full,
        ShapeSpec::BackwardStep {
            step_height,
            upstream_length,
            ..
        } => full - upstream_length * step_height,
        ShapeSpec::CylinderChannel { diameter, .. } => {
            let r = diameter / 2.0;
            full - std::f64::consts::PI * r * r
        }
    }
}

/// Everything wrong with a layout; empty means sound.
///
/// The layout is a 2D shape extruded along z: the first four corners of every block lie on the
/// z = 0 plane and arcs on that plane bulge away from the block they bound.
pub fn problems(layout: &Layout, domain: &DomainSpec, shape: &ShapeSpec) -> Vec<Problem> {
    let mut out = Vec::new();
    let n = layout.vertices.len();
    for (i, b) in layout.blocks.iter().enumerate() {
        if b.vertices.iter().any(|&v| v >= n) {
            out.push(problem(
                ProblemKind::Coverage,
                Some(i),
                "the block names a vertex that does not exist".to_string(),
            ));
        }
    }
    for (k, q) in layout.quads.iter().enumerate() {
        if q.vertices.iter().any(|&v| v >= n) {
            out.push(problem(
                ProblemKind::Coverage,
                None,
                format!("patch face {k} names a vertex that does not exist"),
            ));
        }
    }
    for (k, a) in layout.arcs.iter().enumerate() {
        if a.from >= n || a.to >= n {
            out.push(problem(
                ProblemKind::Coverage,
                None,
                format!("arc {k} names a vertex that does not exist"),
            ));
        }
    }
    if !out.is_empty() {
        return out;
    }
    jacobian(layout, &mut out);
    coverage(layout, &mut out);
    area(layout, domain, shape, &mut out);
    let total = layout
        .blocks
        .iter()
        .fold(0u64, |sum, b| sum.saturating_add(b.cells.iter().product()));
    if total != layout.cell_count {
        out.push(problem(
            ProblemKind::Count,
            None,
            format!(
                "the blocks hold {total} cells but the layout says {}",
                layout.cell_count
            ),
        ));
    }
    out
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn jacobian(layout: &Layout, out: &mut Vec<Problem>) {
    for (i, b) in layout.blocks.iter().enumerate() {
        for (corner, nb) in HEX_NEIGHBOURS.iter().enumerate() {
            let p = layout.vertices[b.vertices[corner]];
            let edge = |k: usize| sub(layout.vertices[b.vertices[nb[k]]], p);
            let triple = dot(edge(0), cross(edge(1), edge(2)));
            if triple.is_nan() || triple <= 0.0 {
                out.push(problem(
                    ProblemKind::Jacobian,
                    Some(i),
                    format!(
                        "corner {corner} of the block is not right-handed (triple product {triple})"
                    ),
                ));
                break;
            }
        }
    }
}

/// The two in-face cell counts of a block face, smaller first.
fn face_dims(cells: &[u64; 3], face: usize) -> [u64; 2] {
    let (p, q) = match face {
        0 | 1 => (cells[1], cells[2]),
        2 | 3 => (cells[0], cells[2]),
        _ => (cells[0], cells[1]),
    };
    if p <= q { [p, q] } else { [q, p] }
}

fn coverage(layout: &Layout, out: &mut Vec<Problem>) {
    let mut faces: BTreeMap<[usize; 4], Vec<(usize, [u64; 2])>> = BTreeMap::new();
    for (i, b) in layout.blocks.iter().enumerate() {
        for (f, local) in HEX_FACES.iter().enumerate() {
            let mut key = local.map(|l| b.vertices[l]);
            key.sort_unstable();
            faces
                .entry(key)
                .or_default()
                .push((i, face_dims(&b.cells, f)));
        }
    }
    let mut patched: BTreeMap<[usize; 4], usize> = BTreeMap::new();
    for q in &layout.quads {
        let mut key = q.vertices;
        key.sort_unstable();
        *patched.entry(key).or_default() += 1;
    }
    for (key, owners) in &faces {
        let on_patches = patched.get(key).copied().unwrap_or(0);
        match owners.as_slice() {
            [(block, _)] => {
                if on_patches != 1 {
                    out.push(problem(
                        ProblemKind::Coverage,
                        Some(*block),
                        format!("the boundary face {key:?} is on {on_patches} patches, not one"),
                    ));
                }
            }
            [(block, first), (_, second)] => {
                if on_patches != 0 {
                    out.push(problem(
                        ProblemKind::Coverage,
                        Some(*block),
                        format!("the interior face {key:?} is on a patch"),
                    ));
                }
                if first != second {
                    out.push(problem(
                        ProblemKind::Coverage,
                        Some(*block),
                        format!("the cell counts differ across the shared face {key:?}"),
                    ));
                }
            }
            _ => out.push(problem(
                ProblemKind::Coverage,
                None,
                format!("the face {key:?} is shared by more than two blocks"),
            )),
        }
    }
    for key in patched.keys() {
        if !faces.contains_key(key) {
            out.push(problem(
                ProblemKind::Coverage,
                None,
                format!("the patch face {key:?} is not a face of any block"),
            ));
        }
    }
}

/// Signed area of a quadrilateral from its corners, counter-clockwise positive.
fn shoelace(p: [[f64; 3]; 4]) -> f64 {
    let mut twice = 0.0;
    for i in 0..4 {
        let (a, b) = (p[i], p[(i + 1) % 4]);
        twice += a[0] * b[1] - b[0] * a[1];
    }
    twice / 2.0
}

fn length(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = sub(a, b);
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}

/// The radius and the angle of the circular arc from `a` to `b` through `through`, in the z = 0
/// plane; `None` for three points on a line.
fn arc_radius_angle(a: [f64; 3], b: [f64; 3], through: [f64; 3]) -> Option<(f64, f64)> {
    let chord = length(a, b);
    let side_a = length(a, through);
    let side_b = length(through, b);
    let twice_area =
        ((through[0] - a[0]) * (b[1] - a[1]) - (through[1] - a[1]) * (b[0] - a[0])).abs();
    if twice_area <= f64::MIN_POSITIVE {
        return None;
    }
    let radius = chord * side_a * side_b / (2.0 * twice_area);
    let theta = 2.0 * (chord / (2.0 * radius)).min(1.0).asin();
    Some((radius, theta))
}

/// Area between a chord and the circular arc through `through`, in the z = 0 plane.
fn circular_segment(a: [f64; 3], b: [f64; 3], through: [f64; 3]) -> f64 {
    arc_radius_angle(a, b, through).map_or(0.0, |(radius, theta)| {
        radius * radius / 2.0 * (theta - theta.sin())
    })
}

/// The cells along the bottom-face edge of a block that joins vertices `a` and `b`.
fn cells_along(layout: &Layout, a: usize, b: usize) -> Option<u64> {
    layout.blocks.iter().find_map(|block| {
        let v = block.vertices;
        [(0, 1, 0), (1, 2, 1), (3, 2, 0), (0, 3, 1)]
            .into_iter()
            .find_map(|(i, j, direction)| {
                let joins = (v[i] == a && v[j] == b) || (v[i] == b && v[j] == a);
                joins.then_some(block.cells[direction])
            })
    })
}

/// The area of the mesh itself: the layout's area plus, for each circular edge, what the straight
/// chords between its mesh points leave over the arc. `Layout::area` is the exact outline.
pub(crate) fn meshed_area(layout: &Layout) -> f64 {
    let extra: f64 = layout
        .arcs
        .iter()
        .filter(|arc| arc.through[2].abs() <= 1e-12)
        .filter_map(|arc| {
            let (a, b) = (layout.vertices[arc.from], layout.vertices[arc.to]);
            let (radius, theta) = arc_radius_angle(a, b, arc.through)?;
            let n = cells_along(layout, arc.from, arc.to)? as f64;
            Some(radius * radius / 2.0 * (theta - n * (theta / n).sin()))
        })
        .sum();
    layout.area + extra
}

fn area(layout: &Layout, domain: &DomainSpec, shape: &ShapeSpec, out: &mut Vec<Problem>) {
    let expected = domain_area(domain, shape);
    let tol = REL_TOL * expected.abs();
    let (mut polygons, mut volume) = (0.0, 0.0);
    for b in &layout.blocks {
        let p = |k: usize| layout.vertices[b.vertices[k]];
        let a = shoelace([p(0), p(1), p(2), p(3)]);
        polygons += a;
        volume += a * (p(4)[2] - p(0)[2]);
    }
    let flat = 1e-12 * domain.length_z.abs();
    let segments: f64 = layout
        .arcs
        .iter()
        .filter(|arc| arc.through[2].abs() <= flat)
        .map(|arc| {
            circular_segment(
                layout.vertices[arc.from],
                layout.vertices[arc.to],
                arc.through,
            )
        })
        .sum();
    let blocks = polygons - segments;
    if (blocks - expected).abs() > tol {
        out.push(problem(
            ProblemKind::Area,
            None,
            format!("the blocks cover an area of {blocks} but the domain has {expected}"),
        ));
    }
    if (layout.area - expected).abs() > tol {
        out.push(problem(
            ProblemKind::Area,
            None,
            format!(
                "the layout says its area is {} but the domain has {expected}",
                layout.area
            ),
        ));
    }
    let blocks_volume = volume - segments * domain.length_z;
    let expected_volume = layout.area * domain.length_z;
    if (blocks_volume - expected_volume).abs() > tol * domain.length_z.abs() {
        out.push(problem(
            ProblemKind::Area,
            None,
            format!("the blocks fill a volume of {blocks_volume} but area times depth is {expected_volume}"),
        ));
    }
}
