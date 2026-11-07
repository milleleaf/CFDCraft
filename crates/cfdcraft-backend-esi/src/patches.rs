//! The patch table of a written case.
//!
//! Patch names come from a fixed table keyed by boundary kind and from the names the shape owns
//! (`obstacle`, `startup`), never from the author's labels, so a label cannot reach the case and
//! renaming a boundary changes no byte. The faces of each patch are the quadrilaterals of the shape
//! layout, so the table and the mesh dictionary always agree.

use crate::case::CompileError;
use cfdcraft_model::{
    BoundaryKind, Face, ResolvedBoundary, ResolvedShape, ResolvedStudy, ResolvedThermal,
    resolve::velocity_vector,
};
use cfdcraft_shapes::{DomainSpec, Layout, MeshRequest, ShapeError, ShapeSpec, Side};
use std::collections::BTreeSet;

/// One written OpenFOAM patch.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Patch {
    pub name: &'static str,
    /// `None` is the front/back `empty` patch.
    pub kind: Option<BoundaryKind>,
    pub velocity: Option<[f64; 3]>,
    /// The fixed temperature of an isothermal wall patch, in kelvin.
    pub temperature: Option<f64>,
    /// The vertex tuples of the block faces of this patch, in the order they are written.
    pub quads: Vec<[usize; 4]>,
}

impl Patch {
    /// The mesh patch type written in `blockMeshDict`.
    pub fn mesh_type(&self) -> &'static str {
        match self.kind {
            None => "empty",
            Some(BoundaryKind::VelocityInlet | BoundaryKind::PressureOutlet) => "patch",
            Some(BoundaryKind::MovingWall | BoundaryKind::NoSlipWall | BoundaryKind::SlipWall) => {
                "wall"
            }
            Some(BoundaryKind::Symmetry) => self.symmetry_type(),
        }
    }

    /// The type of a symmetry patch, in the mesh and on every field. The start-up section of a
    /// step lies on both the floor and the roof of the channel, which is not one plane, and the
    /// mesh builder refuses `symmetryPlane` for it ("Symmetry plane 'startup' is not planar"); the
    /// general `symmetry` type accepts it.
    pub fn symmetry_type(&self) -> &'static str {
        if self.name == STARTUP {
            "symmetry"
        } else {
            "symmetryPlane"
        }
    }

    /// Vertex tuples of the faces as `blockMeshDict` lists them.
    pub fn faces_text(&self) -> String {
        self.quads
            .iter()
            .map(|q| format!("({} {} {} {})", q[0], q[1], q[2], q[3]))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

const KIND_ORDER: [(BoundaryKind, &str); 6] = [
    (BoundaryKind::VelocityInlet, "inlet"),
    (BoundaryKind::PressureOutlet, "outlet"),
    (BoundaryKind::MovingWall, "movingWall"),
    (BoundaryKind::NoSlipWall, "fixedWalls"),
    (BoundaryKind::SlipWall, "slipWalls"),
    (BoundaryKind::Symmetry, "symmetry"),
];

/// The names of the walls held at a fixed temperature, by rank in ascending temperature.
const ISOTHERMAL: [&str; 4] = ["isothermal1", "isothermal2", "isothermal3", "isothermal4"];

/// The fixed temperature of a boundary, in kelvin.
fn fixed_temperature(b: &ResolvedBoundary) -> Option<f64> {
    match b.thermal {
        Some(ResolvedThermal::FixedTemperature { value }) => Some(value),
        _ => None,
    }
}

/// The distinct fixed wall temperatures of the study, ascending.
fn wall_temperatures(s: &ResolvedStudy) -> Vec<f64> {
    let mut found: Vec<f64> = s
        .boundary_conditions
        .values()
        .filter_map(fixed_temperature)
        .collect();
    found.sort_by(f64::total_cmp);
    found.dedup();
    found
}

/// The patch of the cylinder surface.
const OBSTACLE: &str = "obstacle";
/// The symmetry patch of the start-up section of a step.
const STARTUP: &str = "startup";

/// The written patch of the boundary with this label; `None` for a label the study does not have.
/// A boundary that holds the obstacle face among others is answered with the patch of its other
/// faces; one that holds only the obstacle face is answered with `obstacle`.
pub fn patch_of_label(s: &ResolvedStudy, label: &str) -> Option<&'static str> {
    let b = s.boundary_conditions.get(label)?;
    if !b.faces.is_empty() && b.faces.iter().all(|f| *f == Face::Obstacle) {
        return Some(OBSTACLE);
    }
    if b.kind == BoundaryKind::NoSlipWall
        && let Some(t) = fixed_temperature(b)
        && let Some(name) = wall_temperatures(s)
            .iter()
            .position(|x| *x == t)
            .and_then(|rank| ISOTHERMAL.get(rank).copied())
    {
        return Some(name);
    }
    KIND_ORDER
        .iter()
        .find(|(kind, _)| *kind == b.kind)
        .map(|(_, name)| *name)
}

/// The shape, domain and mesh request the layout of a resolved study is built from.
pub fn layout_inputs(s: &ResolvedStudy) -> (DomainSpec, ShapeSpec, MeshRequest) {
    let domain = DomainSpec {
        length_x: s.domain.length_x,
        length_y: s.domain.length_y,
        length_z: s.domain.length_z,
    };
    let shape = match s.domain.shape {
        None => ShapeSpec::Box,
        Some(ResolvedShape::BackwardStep {
            step_height,
            upstream_length,
            startup_length,
        }) => ShapeSpec::BackwardStep {
            step_height,
            upstream_length,
            startup_length,
        },
        Some(ResolvedShape::CylinderChannel {
            diameter,
            centre_x,
            centre_y,
        }) => ShapeSpec::CylinderChannel {
            diameter,
            centre_x,
            centre_y,
        },
    };
    let ratios = s.mesh.grading.as_ref();
    let mesh = MeshRequest {
        cells_x: s.mesh.cells_x,
        cells_y: s.mesh.cells_y,
        cells_z: s.mesh.cells_z,
        level: s.mesh.level,
        ratio_x: ratios.and_then(|g| g.x),
        ratio_y: ratios.and_then(|g| g.y),
    };
    (domain, shape, mesh)
}

fn refusal(e: &ShapeError) -> &'static str {
    match e {
        ShapeError::OddCells { .. } => "a graded axis needs an even cell count",
        ShapeError::BadSize { .. } => "a size or a cell count does not fit the shape",
        ShapeError::BadLevel { .. } => "the refinement level is above the largest level",
        ShapeError::BadGrading { .. } => "the growth ratio must be between 1 and 1.5",
        ShapeError::Unsupported { .. } => "the shape does not offer this mesh request",
    }
}

/// The block layout of a resolved study.
pub fn layout_of(s: &ResolvedStudy) -> Result<Layout, CompileError> {
    let (domain, shape, mesh) = layout_inputs(s);
    cfdcraft_shapes::layout(&domain, &shape, &mesh).map_err(|e| CompileError::Invalid(refusal(&e)))
}

fn side(face: Face) -> Side {
    match face {
        Face::XMin => Side::XMin,
        Face::XMax => Side::XMax,
        Face::YMin => Side::YMin,
        Face::YMax => Side::YMax,
        Face::Obstacle => Side::Obstacle,
    }
}

/// The vertex tuples of the layout faces on these sides, side by side in the order given.
fn quads_of(layout: &Layout, sides: &[Side]) -> Vec<[usize; 4]> {
    let mut out = Vec::new();
    for wanted in sides {
        for q in &layout.quads {
            if q.side == *wanted {
                out.push(q.vertices);
            }
        }
    }
    out
}

/// Group the resolved boundaries by kind into the fixed patch order, then the walls held at a
/// fixed temperature, the obstacle, the start-up section, and `frontAndBack`.
pub(crate) fn patch_plan(s: &ResolvedStudy) -> Result<Vec<Patch>, CompileError> {
    patch_plan_with(s, &layout_of(s)?)
}

pub(crate) fn patch_plan_with(
    s: &ResolvedStudy,
    layout: &Layout,
) -> Result<Vec<Patch>, CompileError> {
    let mut seen = BTreeSet::new();
    for boundary in s.boundary_conditions.values() {
        for face in &boundary.faces {
            if !seen.insert(*face) {
                return Err(CompileError::Invalid("a face is assigned twice"));
            }
        }
    }
    for boundary in s.boundary_conditions.values() {
        if fixed_temperature(boundary).is_none() {
            continue;
        }
        if boundary.kind != BoundaryKind::NoSlipWall {
            return Err(CompileError::Invalid(
                "a fixed temperature is supported on no-slip walls only",
            ));
        }
        if boundary.faces.contains(&Face::Obstacle) {
            return Err(CompileError::Invalid(
                "a fixed temperature is not supported on the obstacle",
            ));
        }
    }
    let temperatures = wall_temperatures(s);
    if temperatures.len() > ISOTHERMAL.len() {
        return Err(CompileError::Invalid(
            "at most four distinct wall temperatures are supported",
        ));
    }
    let cylinder = matches!(s.domain.shape, Some(ResolvedShape::CylinderChannel { .. }));
    if seen.contains(&Face::Obstacle) && !cylinder {
        return Err(CompileError::Invalid(
            "the obstacle face needs a cylinder channel",
        ));
    }
    if seen.len() != 4 + usize::from(cylinder) {
        return Err(CompileError::Invalid(
            "every face of the shape must belong to exactly one boundary",
        ));
    }
    let mut patches = Vec::new();
    for (kind, name) in KIND_ORDER {
        let mut faces = Vec::new();
        let mut velocity: Option<[f64; 3]> = None;
        for boundary in s
            .boundary_conditions
            .values()
            .filter(|b| b.kind == kind && fixed_temperature(b).is_none())
        {
            let speed = match (kind, boundary.velocity) {
                (BoundaryKind::VelocityInlet | BoundaryKind::MovingWall, Some(v))
                    if v.is_finite() && v != 0.0 =>
                {
                    Some(v)
                }
                (BoundaryKind::VelocityInlet | BoundaryKind::MovingWall, _) => {
                    return Err(CompileError::Invalid(
                        "a finite nonzero velocity is required for this boundary kind",
                    ));
                }
                (_, Some(_)) => {
                    return Err(CompileError::Invalid(
                        "this boundary kind carries no velocity",
                    ));
                }
                (_, None) => None,
            };
            for face in &boundary.faces {
                // The obstacle is always its own patch, handled below.
                if *face == Face::Obstacle {
                    continue;
                }
                if let Some(speed) = speed {
                    let vector = velocity_vector(kind, *face, speed);
                    if velocity.is_some_and(|v| v != vector) {
                        return Err(CompileError::Invalid(
                            "boundaries of one kind must impose the same velocity vector",
                        ));
                    }
                    velocity = Some(vector);
                }
                faces.push(*face);
            }
        }
        if !faces.is_empty() {
            faces.sort();
            let sides: Vec<Side> = faces.iter().map(|f| side(*f)).collect();
            patches.push(Patch {
                name,
                kind: Some(kind),
                quads: quads_of(layout, &sides),
                velocity,
                temperature: None,
            });
        }
    }
    // Walls held at a fixed temperature are patches of their own, one per distinct temperature,
    // so the heat through each can be told apart.
    for (name, temperature) in ISOTHERMAL.iter().zip(&temperatures) {
        let mut faces: Vec<Face> = s
            .boundary_conditions
            .values()
            .filter(|b| fixed_temperature(b) == Some(*temperature))
            .flat_map(|b| b.faces.iter().copied())
            .collect();
        faces.sort();
        let sides: Vec<Side> = faces.iter().map(|f| side(*f)).collect();
        patches.push(Patch {
            name,
            kind: Some(BoundaryKind::NoSlipWall),
            velocity: None,
            temperature: Some(*temperature),
            quads: quads_of(layout, &sides),
        });
    }
    if let Some(b) = s
        .boundary_conditions
        .values()
        .find(|b| b.faces.contains(&Face::Obstacle))
    {
        if !matches!(b.kind, BoundaryKind::NoSlipWall | BoundaryKind::SlipWall) {
            return Err(CompileError::Invalid(
                "the obstacle must be a wall without a velocity",
            ));
        }
        patches.push(Patch {
            name: OBSTACLE,
            kind: Some(b.kind),
            velocity: None,
            temperature: None,
            quads: quads_of(layout, &[Side::Obstacle]),
        });
    }
    let startup = quads_of(layout, &[Side::Startup]);
    if !startup.is_empty() {
        patches.push(Patch {
            name: STARTUP,
            kind: Some(BoundaryKind::Symmetry),
            velocity: None,
            temperature: None,
            quads: startup,
        });
    }
    patches.push(Patch {
        name: "frontAndBack",
        kind: None,
        velocity: None,
        temperature: None,
        quads: quads_of(layout, &[Side::FrontBack]),
    });
    Ok(patches)
}
