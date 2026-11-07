//! A two-dimensional mesh from a block layout: nodes, quadrilaterals and the edges of every
//! boundary group.
//!
//! The shapes crate describes a flat channel as hexahedral blocks one cell thick. [`build`]
//! evaluates that description itself, with no external mesher: every block becomes a structured
//! grid whose interior follows its four edge curves (straight or circular) by transfinite
//! blending, and the grids of neighbouring blocks share their edge nodes. Which nodes are shared
//! is decided by the identity of the block corners and the grid index along an edge, never by
//! comparing coordinates, so no tolerance decides the topology. Nothing here reads a file or a
//! clock.

mod boundary;
mod curve;
mod grid;

use cfdcraft_shapes::{Layout, Side};
use thiserror::Error;

/// The largest cell count a layout may have; a bigger layout is refused before any allocation.
pub const MAX_CELLS: u64 = 50_000_000;

/// The mesh in the x-y plane.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh2d {
    /// Node positions in metres.
    pub nodes: Vec<[f64; 2]>,
    /// The quadrilaterals as node indices, counter-clockwise.
    pub quads: Vec<[u32; 4]>,
    /// The boundary edges of each group, ordered by [`Side`]. The front and back faces are not
    /// boundary groups of a two-dimensional mesh.
    pub boundary: Vec<BoundaryEdges>,
    /// The depth of the layout along z, in metres.
    pub thickness: f64,
}

/// The mesh edges on one part of the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryEdges {
    /// The part of the boundary.
    pub side: Side,
    /// Node pairs. Each edge runs with the cell it belongs to on its left.
    pub edges: Vec<[u32; 2]>,
}

/// Why a layout could not become a mesh.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlockMeshError {
    /// The layout has more cells than [`MAX_CELLS`].
    #[error("the layout has {cells} cells, above the limit of {limit}")]
    TooManyCells {
        /// The cells found.
        cells: u64,
        /// The limit.
        limit: u64,
    },
    /// A block or a face names a vertex that does not exist.
    #[error("the layout names a vertex that does not exist")]
    MissingVertex,
    /// A block is not a flat two-dimensional block.
    #[error("block {block} is not a flat two-dimensional block: {reason}")]
    NotFlat {
        /// The block.
        block: usize,
        /// What is wrong.
        reason: &'static str,
    },
    /// A block has no area or two of its corners are the same vertex.
    #[error("block {block} has no area")]
    DegenerateBlock {
        /// The block.
        block: usize,
    },
    /// A grading does not place the cells it should.
    #[error("block {block}: the grading along direction {direction} does not give {cells} cells")]
    BadGrading {
        /// The block.
        block: usize,
        /// The direction, 0 or 1.
        direction: usize,
        /// The cells expected.
        cells: u64,
    },
    /// The three points of an arc lie on a line.
    #[error("the arc between vertices {from} and {to} is degenerate")]
    DegenerateArc {
        /// First vertex.
        from: usize,
        /// Second vertex.
        to: usize,
    },
    /// A cell has no area or is turned inside out.
    #[error("cell {i}, {j} of block {block} is degenerate or inverted")]
    DegenerateCell {
        /// The block.
        block: usize,
        /// The cell index along the first direction.
        i: usize,
        /// The cell index along the second direction.
        j: usize,
    },
    /// The blocks do not add up to the cell count of the layout.
    #[error("the blocks hold {blocks} cells but the layout says {layout}")]
    CountMismatch {
        /// The cells in the blocks.
        blocks: u64,
        /// The cells the layout names.
        layout: u64,
    },
    /// Two blocks that share an edge place different nodes along it.
    #[error("the blocks that share the edge between vertices {a} and {b} do not agree on it")]
    EdgeMismatch {
        /// First vertex of the edge.
        a: usize,
        /// Second vertex of the edge.
        b: usize,
    },
    /// A boundary face is not one edge of exactly one block.
    #[error("boundary face {quad} matches no block edge")]
    UnmatchedBoundary {
        /// The index of the face in the layout.
        quad: usize,
    },
}

/// Evaluate a layout into a mesh.
///
/// The layout must be a flat shape extruded one cell along z, as the shapes crate builds it.
pub fn build(layout: &Layout) -> Result<Mesh2d, BlockMeshError> {
    if layout.cell_count > MAX_CELLS {
        return Err(BlockMeshError::TooManyCells {
            cells: layout.cell_count,
            limit: MAX_CELLS,
        });
    }
    let flat = grid::flatten(layout)?;
    let filled = grid::fill(layout, &flat)?;
    let boundary = boundary::group(layout, &flat.canon, &filled.blocks)?;
    Ok(Mesh2d {
        nodes: filled.nodes,
        quads: filled.quads,
        boundary,
        thickness: flat.thickness,
    })
}
