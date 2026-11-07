use cfdcraft_shapes::{
    DomainSpec, Layout, MeshRequest, ProblemKind, ShapeError, ShapeSpec, Side, edges, grading,
    layout, problems,
};

fn domain() -> DomainSpec {
    DomainSpec {
        length_x: 0.1,
        length_y: 0.1,
        length_z: 0.05,
    }
}

fn request(cells_x: u64, cells_y: u64) -> MeshRequest {
    MeshRequest {
        cells_x,
        cells_y,
        cells_z: 1,
        level: 0,
        ratio_x: None,
        ratio_y: None,
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

fn build(mesh: &MeshRequest) -> Layout {
    layout(&domain(), &ShapeSpec::Box, mesh).unwrap()
}

#[test]
fn the_box_layout_is_todays_mesh() {
    let l = build(&request(128, 128));
    assert_eq!(
        l.vertices,
        vec![
            [0.0, 0.0, 0.0],
            [0.1, 0.0, 0.0],
            [0.1, 0.1, 0.0],
            [0.0, 0.1, 0.0],
            [0.0, 0.0, 0.05],
            [0.1, 0.0, 0.05],
            [0.1, 0.1, 0.05],
            [0.0, 0.1, 0.05],
        ]
    );
    assert_eq!(l.blocks.len(), 1);
    let b = &l.blocks[0];
    assert_eq!(b.vertices, [0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(b.cells, [128, 128, 1]);
    for g in &b.grading {
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].length_fraction, 1.0);
        assert_eq!(g[0].cell_fraction, 1.0);
        assert_eq!(g[0].expansion, 1.0);
    }
    let quads: Vec<(Side, [usize; 4])> = l.quads.iter().map(|q| (q.side, q.vertices)).collect();
    assert_eq!(
        quads,
        vec![
            (Side::XMin, [0, 4, 7, 3]),
            (Side::XMax, [1, 2, 6, 5]),
            (Side::YMin, [0, 1, 5, 4]),
            (Side::YMax, [3, 7, 6, 2]),
            (Side::FrontBack, [0, 3, 2, 1]),
            (Side::FrontBack, [4, 5, 6, 7]),
        ]
    );
    assert!(l.arcs.is_empty());
    assert_eq!(l.cell_count, 16384);
    assert!(close(l.area, 0.01, 1e-12));
}

#[test]
fn a_level_multiplies_the_cells() {
    // 32 base cells at level 2 are requested as the effective 128.
    let mut refined = request(128, 128);
    refined.level = 2;
    assert_eq!(build(&refined).blocks, build(&request(128, 128)).blocks);
}

#[test]
fn grading_is_two_sided_and_symmetric() {
    let mut mesh = request(40, 4);
    mesh.ratio_x = Some(1.05);
    let l = build(&mesh);
    let g = &l.blocks[0].grading[0];
    assert_eq!(g.len(), 2);
    let e = 1.05f64.powi(19);
    assert_eq!((g[0].length_fraction, g[0].cell_fraction), (0.5, 0.5));
    assert_eq!((g[1].length_fraction, g[1].cell_fraction), (0.5, 0.5));
    assert!(close(g[0].expansion, e, 1e-12));
    assert!(close(g[1].expansion, 1.0 / e, 1e-12));
    // The other axis stays uniform.
    assert_eq!(l.blocks[0].grading[1].len(), 1);

    let xs = edges(g, 40, 0.1);
    assert_eq!(xs.len(), 41);
    let sizes: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    for w in sizes[..20].windows(2) {
        assert!(close(w[1] / w[0], 1.05, 1e-9));
    }
    for w in sizes[20..].windows(2) {
        assert!(close(w[1] / w[0], 1.0 / 1.05, 1e-9));
    }
    let largest = sizes.iter().copied().fold(0.0, f64::max);
    assert!(close(sizes[19], largest, 1e-12));
    assert!(close(sizes[20], largest, 1e-12));

    let mut odd = request(41, 4);
    odd.ratio_x = Some(1.05);
    assert!(matches!(
        layout(&domain(), &ShapeSpec::Box, &odd),
        Err(ShapeError::OddCells { axis: 'x' })
    ));
}

/// The cell edges along x at `level`, for 20 base cells with ratio 1.08, and the ratio used.
fn nested_edges(level: u32) -> (Vec<f64>, f64) {
    let cells = 20u64 << level;
    let mut mesh = request(cells, 4);
    mesh.level = level;
    mesh.ratio_x = Some(1.08);
    let l = build(&mesh);
    let g = &l.blocks[0].grading[0];
    let growth = g[0].expansion.powf(1.0 / ((cells / 2 - 1) as f64));
    (edges(g, cells, 0.1), growth)
}

#[test]
fn refinement_nests_exactly() {
    let levels: Vec<(Vec<f64>, f64)> = (0..=2).map(nested_edges).collect();
    for pair in levels.windows(2) {
        let (coarse, fine) = (&pair[0].0, &pair[1].0);
        assert_eq!(fine.len(), 2 * (coarse.len() - 1) + 1);
        for (i, c) in coarse.iter().enumerate() {
            assert!((c - fine[2 * i]).abs() <= 1e-12 * 0.1, "edge {i}");
        }
        assert!(close(pair[1].1, pair[0].1.sqrt(), 1e-12));
    }
    assert!(close(levels[0].1, 1.08, 1e-12));
    assert!(close(grading::refine(1.08, 1), 1.08f64.sqrt(), 1e-15));
}

#[test]
fn the_box_passes_the_soundness_check() {
    for (rx, ry) in [(None, None), (Some(1.1), Some(1.05))] {
        for level in 0..=4u32 {
            let cells = 20u64 << level;
            let mut mesh = request(cells, cells);
            mesh.level = level;
            mesh.ratio_x = rx;
            mesh.ratio_y = ry;
            let l = build(&mesh);
            let found = problems(&l, &domain(), &ShapeSpec::Box);
            assert!(found.is_empty(), "level {level}: {found:?}");
        }
    }

    let good = build(&request(16, 16));
    let check = |l: &Layout| problems(l, &domain(), &ShapeSpec::Box);

    let mut inverted = good.clone();
    inverted.blocks[0].vertices.swap(0, 1);
    assert!(
        check(&inverted)
            .iter()
            .any(|p| p.kind == ProblemKind::Jacobian && p.block == Some(0))
    );

    let mut gap = good.clone();
    gap.quads.pop();
    assert!(check(&gap).iter().any(|p| p.kind == ProblemKind::Coverage));

    let mut wrong_area = good.clone();
    wrong_area.area *= 1.5;
    assert!(
        check(&wrong_area)
            .iter()
            .any(|p| p.kind == ProblemKind::Area)
    );

    let mut wrong_count = good;
    wrong_count.cell_count += 1;
    assert!(
        check(&wrong_count)
            .iter()
            .any(|p| p.kind == ProblemKind::Count)
    );
}
