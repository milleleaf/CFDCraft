//! The box: one block, with an optional symmetric growth from the walls.

use crate::{
    Block, DomainSpec, Layout, MeshRequest, Quad, ShapeError, ShapeSpec, Side, check, grading,
    product,
};

/// The grading of one in-plane axis: even cells grown from both walls toward the middle.
fn axis_grading(
    cells: u64,
    ratio: Option<f64>,
    level: u32,
    axis: char,
) -> Result<Vec<crate::Segment>, ShapeError> {
    let Some(q) = ratio else {
        return Ok(grading::uniform());
    };
    if !(1.0..=1.5).contains(&q) {
        return Err(ShapeError::BadGrading { axis });
    }
    if (q - 1.0).abs() < f64::EPSILON {
        return Ok(grading::uniform());
    }
    if !cells.is_multiple_of(2) {
        return Err(ShapeError::OddCells { axis });
    }
    Ok(grading::two_sided(cells, grading::refine(q, level)))
}

pub(crate) fn layout(domain: &DomainSpec, mesh: &MeshRequest) -> Result<Layout, ShapeError> {
    for (c, what) in [
        (mesh.cells_x, "cells_x"),
        (mesh.cells_y, "cells_y"),
        (mesh.cells_z, "cells_z"),
    ] {
        if c == 0 {
            return Err(ShapeError::BadSize { what });
        }
    }
    let gx = axis_grading(mesh.cells_x, mesh.ratio_x, mesh.level, 'x')?;
    let gy = axis_grading(mesh.cells_y, mesh.ratio_y, mesh.level, 'y')?;
    let (x, y, z) = (domain.length_x, domain.length_y, domain.length_z);
    let cells = [mesh.cells_x, mesh.cells_y, mesh.cells_z];
    let quad = |side, vertices| Quad { side, vertices };
    Ok(Layout {
        vertices: vec![
            [0.0, 0.0, 0.0],
            [x, 0.0, 0.0],
            [x, y, 0.0],
            [0.0, y, 0.0],
            [0.0, 0.0, z],
            [x, 0.0, z],
            [x, y, z],
            [0.0, y, z],
        ],
        blocks: vec![Block {
            vertices: [0, 1, 2, 3, 4, 5, 6, 7],
            cells,
            grading: [gx, gy, grading::uniform()],
        }],
        arcs: Vec::new(),
        quads: vec![
            quad(Side::XMin, [0, 4, 7, 3]),
            quad(Side::XMax, [1, 2, 6, 5]),
            quad(Side::YMin, [0, 1, 5, 4]),
            quad(Side::YMax, [3, 7, 6, 2]),
            quad(Side::FrontBack, [0, 3, 2, 1]),
            quad(Side::FrontBack, [4, 5, 6, 7]),
        ],
        cell_count: product(cells)?,
        area: check::domain_area(domain, &ShapeSpec::Box),
    })
}
