//! Block layouts for the box, the backward-facing step and the cylinder channel.
//!
//! A layout is plain data: vertices, hexahedral blocks with their cell counts and gradings, arc
//! edges, and the quadrilaterals that make up each boundary patch. Nothing here reads a file or a
//! clock. Refining by a level multiplies the cells of every block by 2^level in each in-plane
//! direction and takes the matching root of every growth ratio, so a coarse cell edge is always a
//! fine cell edge. [`problems`] checks a layout for the defects a hand-derived multi-block
//! topology is prone to, before any solver sees it.

mod box_shape;
mod check;
mod cylinder;
pub mod grading;
mod step;

pub use check::{Problem, ProblemKind, problems};
pub use grading::edges;
use thiserror::Error;

/// The largest refinement level.
pub const MAX_LEVEL: u32 = 4;

/// Which part of the boundary a patch face belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// The face at the smallest x (the inlet side).
    XMin,
    /// The face at the largest x (the outlet side).
    XMax,
    /// The floor, including a step face.
    YMin,
    /// The top wall.
    YMax,
    /// The surface of the obstacle inside the channel.
    Obstacle,
    /// The symmetry sections of the start-up block.
    Startup,
    /// Both z faces of every block.
    FrontBack,
}

/// The shape of the domain, in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeSpec {
    /// A rectangle.
    Box,
    /// A channel whose floor drops by `step_height` at x = `upstream_length`; the first
    /// `startup_length` of the inlet section is a symmetry section (0 for none).
    BackwardStep {
        /// Height of the step.
        step_height: f64,
        /// Distance from the inlet to the step.
        upstream_length: f64,
        /// Length of the symmetry section at the inlet.
        startup_length: f64,
    },
    /// A channel with a circular cylinder across it.
    CylinderChannel {
        /// Diameter of the cylinder.
        diameter: f64,
        /// x of the cylinder axis.
        centre_x: f64,
        /// y of the cylinder axis.
        centre_y: f64,
    },
}

/// The size of the domain: [0, length_x] x [0, length_y] x [0, length_z].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DomainSpec {
    /// Length along x.
    pub length_x: f64,
    /// Length along y.
    pub length_y: f64,
    /// Depth along z.
    pub length_z: f64,
}

/// What mesh is asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshRequest {
    /// Effective cells along x (box only; other shapes use their own counts).
    pub cells_x: u64,
    /// Effective cells along y (box only).
    pub cells_y: u64,
    /// Cells along z.
    pub cells_z: u64,
    /// The refinement level, 0 to [`MAX_LEVEL`].
    pub level: u32,
    /// Level-0 per-cell growth ratio from each x wall toward the middle (box only).
    pub ratio_x: Option<f64>,
    /// Level-0 per-cell growth ratio from each y wall toward the middle (box only).
    pub ratio_y: Option<f64>,
}

/// One section of a graded axis: an OpenFOAM multi-grading triple.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Share of the axis length.
    pub length_fraction: f64,
    /// Share of the axis cells.
    pub cell_fraction: f64,
    /// Size of the last cell over the size of the first cell.
    pub expansion: f64,
}

/// A hexahedral block.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// The eight corners in hexahedron order.
    pub vertices: [usize; 8],
    /// Cells along the block's three directions.
    pub cells: [u64; 3],
    /// The grading of each direction.
    pub grading: [Vec<Segment>; 3],
}

/// A circular arc between two vertices through a third point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcEdge {
    /// First vertex.
    pub from: usize,
    /// Second vertex.
    pub to: usize,
    /// A point on the arc between the two.
    pub through: [f64; 3],
}

/// One boundary face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quad {
    /// The part of the boundary it belongs to.
    pub side: Side,
    /// Its four vertices.
    pub vertices: [usize; 4],
}

/// A complete block layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Vertex positions in metres.
    pub vertices: Vec<[f64; 3]>,
    /// The blocks.
    pub blocks: Vec<Block>,
    /// The curved edges.
    pub arcs: Vec<ArcEdge>,
    /// The boundary faces.
    pub quads: Vec<Quad>,
    /// Total cells over all blocks.
    pub cell_count: u64,
    /// Area of the domain in the x-y plane.
    pub area: f64,
}

impl Layout {
    /// The area of the mesh in the x-y plane. It differs from [`Layout::area`] only where the
    /// outline has circular edges: the mesh joins the points on an arc by straight chords, so it
    /// covers slightly more than the exact outline does.
    pub fn meshed_area(&self) -> f64 {
        check::meshed_area(self)
    }
}

/// Why a layout could not be built.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ShapeError {
    /// A graded axis needs an even cell count.
    #[error("the cell count along {axis} must be even for a graded axis")]
    OddCells {
        /// The axis, `x` or `y`.
        axis: char,
    },
    /// A size is not positive, not finite, or does not fit in the domain.
    #[error("{what} must be a positive, finite size that fits the domain")]
    BadSize {
        /// What was refused.
        what: &'static str,
    },
    /// The level is above the largest.
    #[error("the refinement level {level} is above the largest level")]
    BadLevel {
        /// The requested level.
        level: u32,
    },
    /// A growth ratio is outside 1 to 1.5.
    #[error("the growth ratio along {axis} must be between 1 and 1.5")]
    BadGrading {
        /// The axis, `x` or `y`.
        axis: char,
    },
    /// The request asks for something this shape does not offer.
    #[error("{what} is not supported")]
    Unsupported {
        /// What is not supported.
        what: &'static str,
    },
}

/// Build the layout of `shape` in `domain` at the requested mesh.
///
/// A box takes its cell counts and optional wall grading from `mesh`. The step and the cylinder
/// use their own cell counts (see [`reference_totals`]) and only refine by `mesh.level`.
pub fn layout(
    domain: &DomainSpec,
    shape: &ShapeSpec,
    mesh: &MeshRequest,
) -> Result<Layout, ShapeError> {
    if mesh.level > MAX_LEVEL {
        return Err(ShapeError::BadLevel { level: mesh.level });
    }
    for (v, what) in [
        (domain.length_x, "length_x"),
        (domain.length_y, "length_y"),
        (domain.length_z, "length_z"),
    ] {
        positive(v, what)?;
    }
    match shape {
        ShapeSpec::Box => box_shape::layout(domain, mesh),
        ShapeSpec::BackwardStep { .. } => {
            no_ratios(mesh)?;
            step::layout(domain, shape, mesh)
        }
        ShapeSpec::CylinderChannel { .. } => {
            no_ratios(mesh)?;
            cylinder::layout(domain, shape, mesh)
        }
    }
}

fn no_ratios(mesh: &MeshRequest) -> Result<(), ShapeError> {
    if mesh.ratio_x.is_some() || mesh.ratio_y.is_some() {
        return Err(ShapeError::Unsupported {
            what: "wall grading on this shape",
        });
    }
    Ok(())
}

/// The level-0 cells along the x reference line and the y reference line; `None` for the box.
pub fn reference_totals(shape: &ShapeSpec, _domain: &DomainSpec) -> Option<(u64, u64)> {
    match *shape {
        ShapeSpec::Box => None,
        ShapeSpec::BackwardStep { startup_length, .. } => {
            Some(step::reference_totals(startup_length > 0.0))
        }
        ShapeSpec::CylinderChannel { .. } => Some(cylinder::reference_totals()),
    }
}

/// A size that must be positive and finite.
pub(crate) fn positive(v: f64, what: &'static str) -> Result<f64, ShapeError> {
    if v.is_finite() && v > 0.0 {
        Ok(v)
    } else {
        Err(ShapeError::BadSize { what })
    }
}

/// The product of three cell counts, or a size error on overflow.
pub(crate) fn product(cells: [u64; 3]) -> Result<u64, ShapeError> {
    cells
        .iter()
        .try_fold(1u64, |acc, &c| acc.checked_mul(c))
        .ok_or(ShapeError::BadSize {
            what: "the cell count",
        })
}

/// A block of a flat shape: four corners counter-clockwise, x from corner 0 to 1, y from 0 to 3.
pub(crate) struct PlanarBlock {
    pub corners: [usize; 4],
    pub cells: [u64; 2],
    pub grading: [Vec<Segment>; 2],
}

/// A flat shape to extrude along z.
pub(crate) struct Planar {
    pub points: Vec<[f64; 2]>,
    pub blocks: Vec<PlanarBlock>,
    /// Arcs as (from point, to point, a point between them).
    pub arcs: Vec<(usize, usize, [f64; 2])>,
    /// Boundary edges as (side, from point, to point), following the block's counter-clockwise
    /// corner order.
    pub edges: Vec<(Side, usize, usize)>,
    pub area: f64,
}

/// Extrude a flat shape from z = 0 to the domain depth.
///
/// Vertices come in two planes: the points at z = 0 first, then the same points at the domain
/// depth, so a point `p` has the vertex `p + points.len()` above it.
pub(crate) fn extrude(
    plane: Planar,
    domain: &DomainSpec,
    cells_z: u64,
) -> Result<Layout, ShapeError> {
    if cells_z == 0 {
        return Err(ShapeError::BadSize { what: "cells_z" });
    }
    let n = plane.points.len();
    let mut vertices = Vec::with_capacity(2 * n);
    for z in [0.0, domain.length_z] {
        for p in &plane.points {
            vertices.push([p[0], p[1], z]);
        }
    }
    let mut cell_count = 0u64;
    let mut blocks = Vec::with_capacity(plane.blocks.len());
    let mut quads = Vec::new();
    for b in plane.blocks {
        let [a, bb, c, d] = b.corners;
        let cells = [b.cells[0], b.cells[1], cells_z];
        cell_count = cell_count
            .checked_add(product(cells)?)
            .ok_or(ShapeError::BadSize {
                what: "the cell count",
            })?;
        let [gx, gy] = b.grading;
        blocks.push(Block {
            vertices: [a, bb, c, d, a + n, bb + n, c + n, d + n],
            cells,
            grading: [gx, gy, grading::uniform()],
        });
    }
    for (side, p, q) in plane.edges {
        quads.push(Quad {
            side,
            vertices: [p, q, q + n, p + n],
        });
    }
    for b in &blocks {
        let [a, bb, c, d, a2, b2, c2, d2] = b.vertices;
        quads.push(Quad {
            side: Side::FrontBack,
            vertices: [a, d, c, bb],
        });
        quads.push(Quad {
            side: Side::FrontBack,
            vertices: [a2, b2, c2, d2],
        });
    }
    let mut arcs = Vec::with_capacity(2 * plane.arcs.len());
    for (z, shift) in [(0.0, 0), (domain.length_z, n)] {
        for &(from, to, t) in &plane.arcs {
            arcs.push(ArcEdge {
                from: from + shift,
                to: to + shift,
                through: [t[0], t[1], z],
            });
        }
    }
    Ok(Layout {
        vertices,
        blocks,
        arcs,
        quads,
        cell_count,
        area: plane.area,
    })
}
