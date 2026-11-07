use cfdcraft_shapes::{
    Block, DomainSpec, Layout, MeshRequest, ProblemKind, ShapeError, ShapeSpec, Side, edges,
    layout, problems, reference_totals,
};

const LX: f64 = 2.2;
const LY: f64 = 0.41;
const LZ: f64 = 0.1;
const D: f64 = 0.1;
const CX: f64 = 0.2;
const CY: f64 = 0.2;

fn domain() -> DomainSpec {
    DomainSpec {
        length_x: LX,
        length_y: LY,
        length_z: LZ,
    }
}

fn shape_at(centre_x: f64, centre_y: f64) -> ShapeSpec {
    ShapeSpec::CylinderChannel {
        diameter: D,
        centre_x,
        centre_y,
    }
}

fn shape() -> ShapeSpec {
    shape_at(CX, CY)
}

fn request(level: u32) -> MeshRequest {
    MeshRequest {
        cells_x: 114,
        cells_y: 44,
        cells_z: 1,
        level,
        ratio_x: None,
        ratio_y: None,
    }
}

fn build(level: u32) -> Layout {
    layout(&domain(), &shape(), &request(level)).unwrap()
}

fn quads_of(l: &Layout, side: Side) -> Vec<[usize; 4]> {
    l.quads
        .iter()
        .filter(|q| q.side == side)
        .map(|q| q.vertices)
        .collect()
}

fn radius_of(p: [f64; 3]) -> f64 {
    ((p[0] - CX).powi(2) + (p[1] - CY).powi(2)).sqrt()
}

fn extent(l: &Layout, b: &Block, axis: usize) -> f64 {
    let end = if axis == 0 { 1 } else { 3 };
    let (p, q) = (l.vertices[b.vertices[0]], l.vertices[b.vertices[end]]);
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
}

#[test]
fn the_cylinder_layout_is_twelve_blocks_and_eight_arcs() {
    let l = build(0);
    assert_eq!(l.blocks.len(), 12);
    assert_eq!(l.arcs.len(), 8);
    assert_eq!(l.vertices.len(), 40);

    // The four inner points of each z plane sit at 45 degrees on the circle.
    let k = D / 2.0 / 2.0f64.sqrt();
    for (plane, z) in [(0, 0.0), (20, LZ)] {
        let inner = &l.vertices[plane + 16..plane + 20];
        for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            let want = [CX + sx * k, CY + sy * k, z];
            assert!(
                inner
                    .iter()
                    .any(|p| (0..3).all(|c| (p[c] - want[c]).abs() < 1e-12)),
                "missing inner point {want:?}"
            );
        }
    }

    // Four arcs per plane, each through a point on the circle between two inner points.
    for plane_z in [0.0, LZ] {
        let in_plane: Vec<_> = l.arcs.iter().filter(|a| a.through[2] == plane_z).collect();
        assert_eq!(in_plane.len(), 4);
    }
    for arc in &l.arcs {
        assert!((radius_of(arc.through) - D / 2.0).abs() < 1e-12);
        for v in [arc.from, arc.to] {
            assert!(v % 20 >= 16, "an arc ends on a lattice point");
            assert!((radius_of(l.vertices[v]) - D / 2.0).abs() < 1e-12);
        }
    }
}

#[test]
fn the_cell_count_and_totals_at_level_zero() {
    let l = build(0);
    assert_eq!(l.cell_count, 4440 + 4 * 24 * 14);
    assert_eq!(l.cell_count, 5784);
    assert_eq!(reference_totals(&shape(), &domain()), Some((114, 44)));
}

#[test]
fn the_patches_cover_the_boundary() {
    for level in 0..=4 {
        let l = build(level);
        let found = problems(&l, &domain(), &shape());
        assert!(found.is_empty(), "level {level}: {found:?}");
    }
    let l = build(0);
    let on = |side, axis: usize, at: f64| {
        let quads = quads_of(&l, side);
        assert_eq!(quads.len(), 3, "{side:?}");
        assert!(
            quads
                .iter()
                .all(|q| q.iter().all(|&v| l.vertices[v][axis] == at)),
            "{side:?}"
        );
    };
    on(Side::XMin, 0, 0.0);
    on(Side::XMax, 0, LX);
    on(Side::YMin, 1, 0.0);
    on(Side::YMax, 1, LY);
    assert_eq!(quads_of(&l, Side::Obstacle).len(), 4);
    assert!(quads_of(&l, Side::Startup).is_empty());
    assert_eq!(quads_of(&l, Side::FrontBack).len(), 24);
}

#[test]
fn the_area_is_the_channel_minus_the_disc() {
    let l = build(0);
    let r = D / 2.0;
    assert!((l.area - (LX * LY - std::f64::consts::PI * r * r)).abs() < 1e-12);
    let found = problems(&l, &domain(), &shape());
    assert!(
        found.iter().all(|p| p.kind != ProblemKind::Area),
        "{found:?}"
    );
}

#[test]
fn refinement_nests_and_multiplies() {
    let (l0, l2) = (build(0), build(2));
    assert_eq!(l0.blocks.len(), l2.blocks.len());
    assert_eq!(l2.cell_count, 16 * l0.cell_count);
    for (b0, b2) in l0.blocks.iter().zip(&l2.blocks) {
        assert_eq!(b2.cells, [4 * b0.cells[0], 4 * b0.cells[1], b0.cells[2]]);
        for axis in 0..2 {
            let length = extent(&l0, b0, axis);
            let coarse = edges(&b0.grading[axis], b0.cells[axis], length);
            let fine = edges(&b2.grading[axis], b2.cells[axis], length);
            for (i, c) in coarse.iter().enumerate() {
                assert!(
                    (c - fine[4 * i]).abs() <= 1e-12 * length,
                    "axis {axis} edge {i}"
                );
            }
        }
    }
}

#[test]
fn the_circle_needs_room() {
    for (cx, cy) in [
        (0.05, CY),
        (0.09, CY),
        (CX, 0.05),
        (CX, 0.09),
        (CX, 0.35),
        (LX - 0.05, CY),
        (LX - 0.09, CY),
    ] {
        assert!(
            matches!(
                layout(&domain(), &shape_at(cx, cy), &request(0)),
                Err(ShapeError::BadSize { .. })
            ),
            "({cx}, {cy}) should be refused"
        );
    }
    assert!(layout(&domain(), &shape_at(0.2, 0.2), &request(0)).is_ok());
}

#[test]
fn the_ring_blocks_face_the_disc() {
    let l = build(0);
    let found = problems(&l, &domain(), &shape());
    assert!(
        found.iter().all(|p| p.kind != ProblemKind::Jacobian),
        "{found:?}"
    );
    let obstacle = quads_of(&l, Side::Obstacle);
    assert_eq!(obstacle.len(), 4);
    for quad in &obstacle {
        for &v in quad {
            assert!((radius_of(l.vertices[v]) - D / 2.0).abs() < 1e-12);
        }
        let low = quad.iter().filter(|&&v| l.vertices[v][2] == 0.0).count();
        assert_eq!(low, 2);
    }
    // The four ring blocks come last and each has its two inner corners on the circle.
    for b in &l.blocks[8..] {
        for corner in [2, 3, 6, 7] {
            assert!((radius_of(l.vertices[b.vertices[corner]]) - D / 2.0).abs() < 1e-12);
        }
    }
}

/// The compute host's mesh check of the coarse cylinder channel (2.2 m by 0.41 m, depth 0.01 m,
/// 96 faces on the circle) reported a total volume of 0.00894151624492 m3. The exact outline gives
/// 0.0089414601836 m3: the mesh joins the points on the circle by straight chords.
#[test]
fn the_meshed_area_is_what_the_mesh_check_measured() {
    let l = build(0);
    let measured = 0.00894151624492 / 0.01;
    assert!(
        (l.meshed_area() - measured).abs() < 1e-12,
        "{} against {measured}",
        l.meshed_area()
    );
    assert!(l.meshed_area() > l.area);
}

#[test]
fn the_chords_leave_a_quarter_as_much_when_the_cells_double() {
    let r = D / 2.0;
    let loss = |n: f64| {
        4.0 * r * r / 2.0
            * (std::f64::consts::FRAC_PI_2 - n * (std::f64::consts::FRAC_PI_2 / n).sin())
    };
    let l = build(1);
    assert!((l.meshed_area() - l.area - loss(48.0)).abs() < 1e-14);
}
