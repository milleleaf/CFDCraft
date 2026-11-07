use cfdcraft_shapes::{
    Block, DomainSpec, Layout, MeshRequest, ShapeError, ShapeSpec, Side, edges, layout, problems,
    reference_totals,
};

const H: f64 = 0.0127;
const UPSTREAM: f64 = 1.651;

fn domain() -> DomainSpec {
    DomainSpec {
        length_x: 2.286,
        length_y: 0.1143,
        length_z: 0.0127,
    }
}

fn shape(startup_length: f64) -> ShapeSpec {
    ShapeSpec::BackwardStep {
        step_height: H,
        upstream_length: UPSTREAM,
        startup_length,
    }
}

fn request(level: u32) -> MeshRequest {
    MeshRequest {
        cells_x: 220,
        cells_y: 114,
        cells_z: 1,
        level,
        ratio_x: None,
        ratio_y: None,
    }
}

fn build(shape: &ShapeSpec, level: u32) -> Layout {
    layout(&domain(), shape, &request(level)).unwrap()
}

fn quads_of(l: &Layout, side: Side) -> Vec<[usize; 4]> {
    l.quads
        .iter()
        .filter(|q| q.side == side)
        .map(|q| q.vertices)
        .collect()
}

fn sorted(mut v: [usize; 4]) -> [usize; 4] {
    v.sort_unstable();
    v
}

/// The distance between two corners of a block along its local x (axis 0) or y (axis 1).
fn extent(l: &Layout, b: &Block, axis: usize) -> f64 {
    let end = if axis == 0 { 1 } else { 3 };
    let (p, q) = (l.vertices[b.vertices[0]], l.vertices[b.vertices[end]]);
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
}

#[test]
fn the_step_has_four_blocks_and_about_the_tutorial_cell_count() {
    let s = shape(0.254);
    let l = build(&s, 0);
    assert_eq!(l.blocks.len(), 4);
    assert_eq!(l.cell_count, 2 * 66 + 88 * 66 + 130 * 48 + 130 * 66);
    assert_eq!(l.cell_count, 20_760);
    assert!((l.cell_count as f64 - 20_540.0).abs() / 20_540.0 < 0.011);
    assert_eq!(reference_totals(&s, &domain()), Some((220, 114)));
    assert!(l.arcs.is_empty());
}

#[test]
fn no_startup_means_three_blocks() {
    let s = shape(0.0);
    let l = build(&s, 0);
    assert_eq!(l.blocks.len(), 3);
    assert!(quads_of(&l, Side::Startup).is_empty());
    assert_eq!(l.cell_count, 88 * 66 + 130 * 48 + 130 * 66);
    assert_eq!(reference_totals(&s, &domain()), Some((218, 114)));
}

#[test]
fn the_patches_cover_the_boundary() {
    for (startup, first_block_faces) in [(0.254, true), (0.0, false)] {
        let s = shape(startup);
        for level in 0..=4 {
            let l = build(&s, level);
            let found = problems(&l, &domain(), &s);
            assert!(
                found.is_empty(),
                "startup {startup} level {level}: {found:?}"
            );
        }
        let l = build(&s, 0);
        let count = |side| quads_of(&l, side).len();
        let blocks = l.blocks.len();
        assert_eq!(count(Side::XMin), 1);
        assert_eq!(count(Side::XMax), 2);
        assert_eq!(count(Side::YMin), 3);
        assert_eq!(count(Side::YMax), 2);
        assert_eq!(count(Side::Obstacle), 0);
        assert_eq!(count(Side::FrontBack), 2 * blocks);

        // The inlet face is above the step; the step face is a floor patch at the step.
        let inlet = quads_of(&l, Side::XMin)[0];
        assert!(
            inlet
                .iter()
                .all(|&v| l.vertices[v][0] == 0.0 && l.vertices[v][1] >= H)
        );
        let floor = quads_of(&l, Side::YMin);
        let step_faces = floor
            .iter()
            .filter(|q| {
                q.iter()
                    .all(|&v| l.vertices[v][0] == UPSTREAM && l.vertices[v][1] <= H)
            })
            .count();
        assert_eq!(step_faces, 1);

        if first_block_faces {
            // The start-up quads are exactly the first block's y-faces.
            let b = &l.blocks[0];
            let face = |local: [usize; 4]| sorted(local.map(|i| b.vertices[i]));
            let mut want = vec![face([0, 1, 5, 4]), face([3, 7, 6, 2])];
            let mut got: Vec<[usize; 4]> = quads_of(&l, Side::Startup)
                .into_iter()
                .map(sorted)
                .collect();
            want.sort_unstable();
            got.sort_unstable();
            assert_eq!(got, want);
        } else {
            assert_eq!(count(Side::Startup), 0);
        }
    }
}

#[test]
fn the_area_is_the_l_shape() {
    for startup in [0.254, 0.0] {
        let l = build(&shape(startup), 0);
        let expected = 2.286 * 0.1143 - UPSTREAM * H;
        assert!((l.area - expected).abs() < 1e-12);
    }
}

#[test]
fn refinement_multiplies_every_block_by_two_per_direction() {
    for startup in [0.254, 0.0] {
        let s = shape(startup);
        let (l0, l2) = (build(&s, 0), build(&s, 2));
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
}

#[test]
fn wall_cells_are_in_the_wall_function_range_at_level_zero() {
    let l = build(&shape(0.254), 0);
    let lower = &l.blocks[2];
    let upper = &l.blocks[3];
    let y_edges = |b: &Block| edges(&b.grading[1], b.cells[1], extent(&l, b, 1));
    let floor = y_edges(lower);
    let top = y_edges(upper);
    let floor_cell = floor[1] - floor[0];
    let top_cell = top[top.len() - 1] - top[top.len() - 2];
    for (name, cell) in [("floor", floor_cell), ("top wall", top_cell)] {
        assert!(
            (0.01 * H..=0.04 * H).contains(&cell),
            "{name}: first cell is {} step heights",
            cell / H
        );
    }
}

#[test]
fn bad_sizes_are_refused() {
    let refused = |s: ShapeSpec| {
        matches!(
            layout(&domain(), &s, &request(0)),
            Err(ShapeError::BadSize { .. })
        )
    };
    assert!(refused(ShapeSpec::BackwardStep {
        step_height: 0.0,
        upstream_length: UPSTREAM,
        startup_length: 0.254,
    }));
    assert!(refused(shape(2.0)));
    assert!(refused(ShapeSpec::BackwardStep {
        step_height: H,
        upstream_length: 2.286,
        startup_length: 0.0,
    }));
    let mut graded = request(0);
    graded.ratio_x = Some(1.1);
    assert!(matches!(
        layout(&domain(), &shape(0.254), &graded),
        Err(ShapeError::Unsupported { .. })
    ));
}

#[test]
fn a_layout_without_circles_meshes_exactly_its_outline() {
    let l = build(&shape(0.254), 0);
    assert_eq!(l.meshed_area(), l.area);
}
