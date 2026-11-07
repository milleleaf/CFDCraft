//! The cylinder channel: a 3 x 3 lattice of blocks whose middle cell is four ring blocks around
//! the circle.
//!
//! With R the cylinder radius and a = 2R, the lattice lines are x = 0, cx - a, cx + a, length_x and
//! y = 0, cy - a, cy + a, length_y. The middle cell is the square of half width a around the axis;
//! it is filled by the south, east, north and west ring blocks between the square and the circle.
//!
//! Points: the 16 lattice points first (point `i + 4 j` for column `i`, row `j`), then the four
//! inner circle points at 45, 135, 225 and 315 degrees (points 16, 17, 18, 19).

use crate::{
    DomainSpec, Layout, MeshRequest, Planar, PlanarBlock, ShapeError, ShapeSpec, Side, check,
    extrude, grading, positive,
};

/// Level-0 cells of the three lattice columns (the middle one also fixes the ring blocks' count).
const COLUMN_CELLS_X: [u64; 3] = [10, 24, 80];
/// Level-0 cells of the three lattice rows (the middle one also fixes the ring blocks' count).
const ROW_CELLS_Y: [u64; 3] = [10, 24, 10];
/// Level-0 cells of a ring block from the square to the circle.
const RING_RADIAL_CELLS: u64 = 14;

/// Per-cell growth ratio in the right column, clustering toward the cylinder.
const WAKE_X_RATIO: f64 = 1.03;
/// Per-cell growth ratio in the bottom and top rows, clustering toward the channel walls.
const WALL_Y_RATIO: f64 = 1.1;
/// Per-cell growth ratio in the ring blocks, clustering toward the circle.
const RING_RADIAL_RATIO: f64 = 1.1;

/// Level-0 cells along the x and y reference lines.
pub(crate) fn reference_totals() -> (u64, u64) {
    (COLUMN_CELLS_X.iter().sum(), ROW_CELLS_Y.iter().sum())
}

const INNER_NE: usize = 16;
const INNER_NW: usize = 17;
const INNER_SW: usize = 18;
const INNER_SE: usize = 19;

/// The lattice point of column `i`, row `j`.
fn lat(i: usize, j: usize) -> usize {
    i + 4 * j
}

pub(crate) fn layout(
    domain: &DomainSpec,
    shape: &ShapeSpec,
    mesh: &MeshRequest,
) -> Result<Layout, ShapeError> {
    let ShapeSpec::CylinderChannel {
        diameter,
        centre_x,
        centre_y,
    } = *shape
    else {
        return Err(ShapeError::Unsupported {
            what: "this shape here",
        });
    };
    let a = positive(diameter, "diameter")?;
    let r = a / 2.0;
    let (lx, ly) = (domain.length_x, domain.length_y);
    let xs = [0.0, centre_x - a, centre_x + a, lx];
    let ys = [0.0, centre_y - a, centre_y + a, ly];
    let fits =
        |lines: [f64; 4]| lines.iter().all(|v| v.is_finite()) && lines.is_sorted_by(|p, q| p < q);
    if !fits(xs) || !fits(ys) {
        return Err(ShapeError::BadSize {
            what: "the cylinder position",
        });
    }
    let level = mesh.level;
    let scale = 1u64 << level;

    let wake_x = grading::one_sided(
        COLUMN_CELLS_X[2] * scale,
        grading::refine(WAKE_X_RATIO, level),
        true,
    );
    let bottom_y = grading::one_sided(
        ROW_CELLS_Y[0] * scale,
        grading::refine(WALL_Y_RATIO, level),
        true,
    );
    let top_y = grading::one_sided(
        ROW_CELLS_Y[2] * scale,
        grading::refine(WALL_Y_RATIO, level),
        false,
    );
    let radial = grading::one_sided(
        RING_RADIAL_CELLS * scale,
        grading::refine(RING_RADIAL_RATIO, level),
        false,
    );

    let mut points = Vec::with_capacity(20);
    for y in ys {
        for x in xs {
            points.push([x, y]);
        }
    }
    let k = r * std::f64::consts::FRAC_1_SQRT_2;
    points.push([centre_x + k, centre_y + k]);
    points.push([centre_x - k, centre_y + k]);
    points.push([centre_x - k, centre_y - k]);
    points.push([centre_x + k, centre_y - k]);

    let mut blocks = Vec::new();
    let mut edges = Vec::new();
    for (j, &ny) in ROW_CELLS_Y.iter().enumerate() {
        for (i, &nx) in COLUMN_CELLS_X.iter().enumerate() {
            if i == 1 && j == 1 {
                continue;
            }
            let gx = if i == 2 {
                wake_x.clone()
            } else {
                grading::uniform()
            };
            let gy = match j {
                0 => bottom_y.clone(),
                2 => top_y.clone(),
                _ => grading::uniform(),
            };
            blocks.push(PlanarBlock {
                corners: [lat(i, j), lat(i + 1, j), lat(i + 1, j + 1), lat(i, j + 1)],
                cells: [nx * scale, ny * scale],
                grading: [gx, gy],
            });
            if i == 0 {
                edges.push((Side::XMin, lat(0, j + 1), lat(0, j)));
            }
            if i == 2 {
                edges.push((Side::XMax, lat(3, j), lat(3, j + 1)));
            }
            if j == 0 {
                edges.push((Side::YMin, lat(i, 0), lat(i + 1, 0)));
            }
            if j == 2 {
                edges.push((Side::YMax, lat(i + 1, 3), lat(i, 3)));
            }
        }
    }

    let (sw, se, ne, nw) = (lat(1, 1), lat(2, 1), lat(2, 2), lat(1, 2));
    let ring_x = COLUMN_CELLS_X[1] * scale;
    let ring_y = ROW_CELLS_Y[1] * scale;
    let ring_radial = RING_RADIAL_CELLS * scale;
    // (outer corner a, outer corner b, inner corner c, inner corner d, tangential cells)
    let rings = [
        (sw, se, INNER_SE, INNER_SW, ring_x),
        (se, ne, INNER_NE, INNER_SE, ring_y),
        (ne, nw, INNER_NW, INNER_NE, ring_x),
        (nw, sw, INNER_SW, INNER_NW, ring_y),
    ];
    for (a, b, c, d, tangential) in rings {
        blocks.push(PlanarBlock {
            corners: [a, b, c, d],
            cells: [tangential, ring_radial],
            grading: [grading::uniform(), radial.clone()],
        });
        edges.push((Side::Obstacle, c, d));
    }

    let arcs = vec![
        (INNER_SW, INNER_SE, [centre_x, centre_y - r]),
        (INNER_SE, INNER_NE, [centre_x + r, centre_y]),
        (INNER_NE, INNER_NW, [centre_x, centre_y + r]),
        (INNER_NW, INNER_SW, [centre_x - r, centre_y]),
    ];

    extrude(
        Planar {
            points,
            blocks,
            arcs,
            edges,
            area: check::domain_area(domain, shape),
        },
        domain,
        mesh.cells_z,
    )
}
