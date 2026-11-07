//! The lattice of a study and the obstacle that stands in it.
//!
//! The solver fills a box with cubic cells and takes a body as a closed surface, so a shape is
//! written as a prism over its outline: the cylinder as a polygon whose chords stay within a
//! quarter of a cell of the circle, the step as the rectangle under it. The solid cells are the
//! cells whose centre lies inside, so every wall follows the cell faces (a staircase); the outline
//! itself is exact.
//!
//! A box takes its cell size from the study's cell counts, which must give equal square cells. A
//! shape lays out its own blocks, and its cell counts describe those blocks and not a lattice, so
//! a shaped domain takes the coarsest cell size that holds every extent of the shape in whole
//! cells, halved once for every mesh level. Nothing here narrows the study: a size that does not
//! exist is refused with the setting named.

use crate::{markers::SLAB_CELLS_Z, threemf};
use cfdcraft_backend::{CaseFile, CompileFailure};
use cfdcraft_model::{MAX_LEVEL, ResolvedShape, ResolvedStudy};
use std::f64::consts::PI;

/// The most cells the solver takes by default (`max_cells` of its fluid solve).
pub const MAX_CELLS: u64 = 16_000_000;

/// The cells of an outline are cut at this fraction of the cell width at most.
const CHORD_FRACTION: f64 = 0.25;

/// The most corners a circle is cut into.
const MAX_SEGMENTS: u32 = 100_000;

/// Lengths are compared as whole nanometres to find the common cell size.
const NANOMETRE: f64 = 1e-9;

/// The least cells across an inlet or an outlet face that the solver accepts.
pub const MIN_OPEN_CELLS: u64 = 2;

/// A body in the lattice: its name inside the model, the file that holds its surface and the
/// corners of its outline.
#[derive(Debug, Clone, PartialEq)]
pub struct Obstacle {
    /// The name of the object in the package, which the domain refers to.
    pub name: &'static str,
    /// The file inside the case, always `obstacles/<word>.3mf`.
    pub file: &'static str,
    /// Counter-clockwise corners in the x-y plane.
    pub outline: Vec<[f64; 2]>,
}

/// The lattice of a study.
#[derive(Debug, Clone, PartialEq)]
pub struct Lattice {
    pub spacing: f64,
    pub cells_x: u64,
    pub cells_y: u64,
    pub obstacle: Option<Obstacle>,
}

impl Lattice {
    /// The thickness of the slab that stands for the two-dimensional case.
    pub fn slab(&self) -> f64 {
        f64::from(SLAB_CELLS_Z) * self.spacing
    }
}

fn unsupported(setting: &str, reason: impl Into<String>) -> CompileFailure {
    CompileFailure::NotSupported {
        setting: setting.to_owned(),
        reason: reason.into(),
    }
}

fn invalid(reason: impl Into<String>) -> CompileFailure {
    CompileFailure::Invalid {
        reason: reason.into(),
    }
}

/// The corners of a circle's polygon: the smallest even number of them (at least four) for which
/// no chord lies further than a quarter of a cell from the circle, every corner on the circle.
pub fn circle_outline(
    centre: [f64; 2],
    radius: f64,
    spacing: f64,
) -> Result<Vec<[f64; 2]>, CompileFailure> {
    let usable = radius.is_finite() && radius > 0.0 && spacing.is_finite() && spacing > 0.0;
    if !usable {
        return Err(invalid("the cylinder needs a size above zero"));
    }
    let mut segments = 4_u32;
    while radius * (1.0 - (PI / f64::from(segments)).cos()) > CHORD_FRACTION * spacing {
        segments += 2;
        if segments > MAX_SEGMENTS {
            return Err(unsupported(
                "cell size",
                "the cylinder is too large for its cells to be cut finely enough",
            ));
        }
    }
    Ok((0..segments)
        .map(|k| {
            let angle = 2.0 * PI * f64::from(k) / f64::from(segments);
            [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
            ]
        })
        .collect())
}

/// The greatest length, in whole nanometres, that divides every one of `lengths`.
fn common_size(lengths: &[f64]) -> Result<u64, CompileFailure> {
    let mut common = 0_u64;
    for length in lengths {
        let nanometres = length / NANOMETRE;
        let whole = nanometres.round();
        let fits = length.is_finite() && whole >= 1.0 && (nanometres - whole).abs() <= 1e-4;
        if !fits {
            return Err(unsupported(
                "cell size",
                "the lengths of the shape do not share a cell size of a whole number of nanometres",
            ));
        }
        let (mut a, mut b) = (common, whole as u64);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        common = a;
    }
    Ok(common)
}

fn whole_cells(length: f64, spacing: f64) -> Result<u64, CompileFailure> {
    let cells = (length / spacing).round();
    if cells >= 1.0 && (cells * spacing - length).abs() <= 1e-9 * length {
        Ok(cells as u64)
    } else {
        Err(unsupported(
            "cell size",
            "an extent of the domain is not a whole number of cells",
        ))
    }
}

/// The side of the cells of a plain box, which must be the same along x and y.
fn box_spacing(study: &ResolvedStudy) -> Result<f64, CompileFailure> {
    const SQUARE_TOLERANCE: f64 = 1e-9;
    let width = study.domain.length_x / study.mesh.cells_x as f64;
    let height = study.domain.length_y / study.mesh.cells_y as f64;
    let square = width.is_finite()
        && height.is_finite()
        && width > 0.0
        && (width - height).abs() <= SQUARE_TOLERANCE * width.max(height);
    if square {
        Ok(width)
    } else {
        Err(unsupported(
            "cell shape",
            format!(
                "the cells are {width:.6} m wide and {height:.6} m high, and the solver needs \
                 equal square cells"
            ),
        ))
    }
}

/// The lattice and the obstacle of a study.
pub fn lattice(study: &ResolvedStudy) -> Result<Lattice, CompileFailure> {
    let (lx, ly) = (study.domain.length_x, study.domain.length_y);
    let Some(shape) = study.domain.shape else {
        let spacing = box_spacing(study)?;
        return Ok(Lattice {
            spacing,
            cells_x: study.mesh.cells_x,
            cells_y: study.mesh.cells_y,
            obstacle: None,
        });
    };
    let aligned = match shape {
        ResolvedShape::BackwardStep {
            step_height,
            upstream_length,
            startup_length,
        } => {
            if startup_length != 0.0 {
                return Err(unsupported(
                    "start-up section",
                    "a symmetry section on the floor of the inlet cannot be a surface of the lattice, \
                     which takes only walls on a body",
                ));
            }
            let fits = step_height > 0.0
                && step_height < ly
                && upstream_length > 0.0
                && upstream_length < lx;
            if !fits {
                return Err(invalid("the step does not fit in the domain"));
            }
            vec![lx, ly, step_height, upstream_length]
        }
        ResolvedShape::CylinderChannel {
            diameter,
            centre_x,
            centre_y,
        } => {
            let r = diameter / 2.0;
            let fits = r > 0.0
                && centre_x - r > 0.0
                && centre_x + r < lx
                && centre_y - r > 0.0
                && centre_y + r < ly;
            if !fits {
                return Err(invalid("the cylinder does not fit in the channel"));
            }
            vec![lx, ly]
        }
    };
    let coarse = common_size(&aligned)? as f64 * NANOMETRE;
    let spacing = coarse / f64::from(1_u32 << study.mesh.level.min(MAX_LEVEL));
    let (cells_x, cells_y) = (whole_cells(lx, spacing)?, whole_cells(ly, spacing)?);
    let total = cells_x
        .checked_mul(cells_y)
        .and_then(|plane| plane.checked_mul(u64::from(SLAB_CELLS_Z)));
    if total.is_none_or(|cells| cells > MAX_CELLS) {
        return Err(unsupported(
            "cell count",
            format!(
                "the lattice would hold {cells_x} by {cells_y} cells across, and the solver takes \
                 at most {MAX_CELLS} cells; use a coarser mesh level"
            ),
        ));
    }
    let obstacle = match shape {
        ResolvedShape::BackwardStep {
            step_height,
            upstream_length,
            ..
        } => Obstacle {
            name: "Step",
            file: "obstacles/step.3mf",
            outline: vec![
                [0.0, 0.0],
                [upstream_length, 0.0],
                [upstream_length, step_height],
                [0.0, step_height],
            ],
        },
        ResolvedShape::CylinderChannel {
            diameter,
            centre_x,
            centre_y,
        } => Obstacle {
            name: "Cylinder",
            file: "obstacles/cylinder.3mf",
            outline: circle_outline([centre_x, centre_y], diameter / 2.0, spacing)?,
        },
    };
    Ok(Lattice {
        spacing,
        cells_x,
        cells_y,
        obstacle: Some(obstacle),
    })
}

/// The file of an obstacle: its prism, one slab thick, as a 3MF package.
pub fn file_of(lattice: &Lattice) -> Result<Option<CaseFile>, CompileFailure> {
    let Some(obstacle) = &lattice.obstacle else {
        return Ok(None);
    };
    let reason = |e: threemf::ThreeMfError| invalid(format!("the obstacle surface: {e:?}"));
    let mesh = threemf::prism(&obstacle.outline, lattice.slab()).map_err(reason)?;
    let bytes = threemf::write(obstacle.name, &mesh).map_err(reason)?;
    Ok(Some(CaseFile {
        rel_path: obstacle.file.into(),
        bytes,
    }))
}
