//! The backward-facing step: four blocks around the step corner.
//!
//! The domain is [0, length_x] x [0, length_y]. The step is at x = `upstream_length`; the upstream
//! channel spans y in [step_height, length_y] and the downstream channel spans [0, length_y].
//! Blocks, in order: the start-up section (absent without one), the upstream main block, the
//! downstream lower block (below the step height) and the downstream upper block.
//!
//! Points are numbered in a fixed order: the inlet floor corner (0, step_height) and the inlet
//! top corner (0, length_y); then, with a start-up section, its far corners at x = startup_length
//! (floor level, then top); then the corners at the step: (upstream_length, step_height),
//! (upstream_length, length_y), (upstream_length, 0), (length_x, 0), (length_x, step_height) and
//! (length_x, length_y).

use crate::{
    DomainSpec, Layout, MeshRequest, Planar, PlanarBlock, ShapeError, ShapeSpec, Side, check,
    extrude, grading, positive,
};

/// Level-0 cells along x in the start-up block.
const STARTUP_CELLS_X: u64 = 2;
/// Level-0 cells along x in the upstream main block.
const UPSTREAM_CELLS_X: u64 = 88;
/// Level-0 cells along x in both downstream blocks.
const DOWNSTREAM_CELLS_X: u64 = 130;
/// Level-0 cells along y below the step height (even, so it grades from both sides).
const LOWER_CELLS_Y: u64 = 48;
/// Level-0 cells along y above the step height (even, so it grades from both sides).
const UPPER_CELLS_Y: u64 = 66;

/// Per-cell growth ratio in the upstream main block, clustering toward the step.
const UPSTREAM_X_RATIO: f64 = 1.02;
/// Per-cell growth ratio in the downstream blocks, clustering toward the step and its shear layer.
const DOWNSTREAM_X_RATIO: f64 = 1.02;
/// Per-cell growth ratio below the step height, clustering toward the floor and the shear layer.
const LOWER_Y_RATIO: f64 = 1.03;
/// Per-cell growth ratio above the step height, clustering toward the shear layer and the top wall.
const UPPER_Y_RATIO: f64 = 1.09;

/// Level-0 cells along the x and y reference lines.
pub(crate) fn reference_totals(startup: bool) -> (u64, u64) {
    let startup_cells = if startup { STARTUP_CELLS_X } else { 0 };
    (
        startup_cells + UPSTREAM_CELLS_X + DOWNSTREAM_CELLS_X,
        LOWER_CELLS_Y + UPPER_CELLS_Y,
    )
}

pub(crate) fn layout(
    domain: &DomainSpec,
    shape: &ShapeSpec,
    mesh: &MeshRequest,
) -> Result<Layout, ShapeError> {
    let ShapeSpec::BackwardStep {
        step_height,
        upstream_length,
        startup_length,
    } = *shape
    else {
        return Err(ShapeError::Unsupported {
            what: "this shape here",
        });
    };
    let (h, u, s) = (step_height, upstream_length, startup_length);
    let (lx, ly) = (domain.length_x, domain.length_y);
    if positive(h, "step_height")? >= ly {
        return Err(ShapeError::BadSize {
            what: "step_height",
        });
    }
    if positive(u, "upstream_length")? >= lx {
        return Err(ShapeError::BadSize {
            what: "upstream_length",
        });
    }
    if !s.is_finite() || s < 0.0 || s >= u {
        return Err(ShapeError::BadSize {
            what: "startup_length",
        });
    }
    let startup = s > 0.0;
    let level = mesh.level;
    let scale = 1u64 << level;

    let upstream_x = grading::one_sided(
        UPSTREAM_CELLS_X * scale,
        grading::refine(UPSTREAM_X_RATIO, level),
        false,
    );
    let downstream_x = grading::one_sided(
        DOWNSTREAM_CELLS_X * scale,
        grading::refine(DOWNSTREAM_X_RATIO, level),
        true,
    );
    let lower_y = grading::two_sided(LOWER_CELLS_Y * scale, grading::refine(LOWER_Y_RATIO, level));
    let upper_y = grading::two_sided(UPPER_CELLS_Y * scale, grading::refine(UPPER_Y_RATIO, level));

    let mut points = vec![[0.0, h], [0.0, ly]];
    let (mut left_bottom, mut left_top) = (0, 1);
    if startup {
        points.push([s, h]);
        points.push([s, ly]);
        left_bottom = 2;
        left_top = 3;
    }
    let base = points.len();
    points.extend([[u, h], [u, ly], [u, 0.0], [lx, 0.0], [lx, h], [lx, ly]]);
    let (step_corner, step_top, foot, out_floor, out_step, out_top) =
        (base, base + 1, base + 2, base + 3, base + 4, base + 5);

    let mut blocks = Vec::new();
    let mut edges = Vec::new();
    if startup {
        blocks.push(PlanarBlock {
            corners: [0, 2, 3, 1],
            cells: [STARTUP_CELLS_X * scale, UPPER_CELLS_Y * scale],
            grading: [grading::uniform(), upper_y.clone()],
        });
        edges.push((Side::Startup, 0, 2));
        edges.push((Side::Startup, 3, 1));
        edges.push((Side::XMin, 1, 0));
    } else {
        edges.push((Side::XMin, left_top, left_bottom));
    }
    blocks.push(PlanarBlock {
        corners: [left_bottom, step_corner, step_top, left_top],
        cells: [UPSTREAM_CELLS_X * scale, UPPER_CELLS_Y * scale],
        grading: [upstream_x, upper_y.clone()],
    });
    edges.push((Side::YMin, left_bottom, step_corner));
    edges.push((Side::YMax, step_top, left_top));
    blocks.push(PlanarBlock {
        corners: [foot, out_floor, out_step, step_corner],
        cells: [DOWNSTREAM_CELLS_X * scale, LOWER_CELLS_Y * scale],
        grading: [downstream_x.clone(), lower_y],
    });
    edges.push((Side::YMin, foot, out_floor));
    edges.push((Side::XMax, out_floor, out_step));
    edges.push((Side::YMin, step_corner, foot));
    blocks.push(PlanarBlock {
        corners: [step_corner, out_step, out_top, step_top],
        cells: [DOWNSTREAM_CELLS_X * scale, UPPER_CELLS_Y * scale],
        grading: [downstream_x, upper_y],
    });
    edges.push((Side::XMax, out_step, out_top));
    edges.push((Side::YMax, out_top, step_top));

    extrude(
        Planar {
            points,
            blocks,
            arcs: Vec::new(),
            edges,
            area: check::domain_area(domain, shape),
        },
        domain,
        mesh.cells_z,
    )
}
