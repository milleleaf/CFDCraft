use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

use crate::{
    Quantity, Study,
    diagnostics::{Capabilities, Diagnostic, diagnose, has_errors, render_text},
    raw::RawOverride,
    study::{BoundaryKind, Convergence, Face},
    units::to_si,
};

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedStudy {
    pub schema_version: u32,
    pub domain: ResolvedDomain,
    pub fluid: ResolvedFluid,
    pub boundary_conditions: BTreeMap<String, ResolvedBoundary>,
    pub mesh: ResolvedMesh,
    pub solver: ResolvedSolver,
    pub monitors: ResolvedMonitors,
    /// The study's raw overrides, unchanged. Absent when there are none, so a study without
    /// them keeps the same canonical text and hash.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub raw_overrides: BTreeMap<String, RawOverride>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedDomain {
    pub length_x: f64,
    pub length_y: f64,
    pub length_z: f64,
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedFluid {
    pub kinematic_viscosity: f64,
}

/// A boundary in SI: faces sorted and deduplicated, velocity in m/s where the kind has one.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedBoundary {
    pub faces: Vec<Face>,
    pub kind: BoundaryKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ResolvedMesh {
    pub cells_x: u64,
    pub cells_y: u64,
    pub cells_z: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedSolver {
    pub end_time: f64,
    pub write_interval: f64,
    pub convergence: Convergence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ResolvedMonitors {
    pub centreline_u_sample_count: u64,
}

/// A study the checks refuse. `Display` is the rendered findings.
#[derive(Debug, Error, PartialEq)]
pub enum ResolveError {
    #[error("{}", render_text(.0))]
    Diagnostics(Vec<Diagnostic>),
}

/// The velocity vector a boundary of `kind` imposes on `face` for a signed `speed`.
/// An inlet moves along the inward normal; a moving wall moves along +x on the top and bottom
/// faces and +y on the left and right faces. Other kinds impose no velocity.
pub fn velocity_vector(kind: BoundaryKind, face: Face, speed: f64) -> [f64; 3] {
    match (kind, face) {
        (BoundaryKind::VelocityInlet, Face::XMin) => [speed, 0.0, 0.0],
        (BoundaryKind::VelocityInlet, Face::XMax) => [-speed, 0.0, 0.0],
        (BoundaryKind::VelocityInlet, Face::YMin) => [0.0, speed, 0.0],
        (BoundaryKind::VelocityInlet, Face::YMax) => [0.0, -speed, 0.0],
        (BoundaryKind::MovingWall, Face::YMin | Face::YMax) => [speed, 0.0, 0.0],
        (BoundaryKind::MovingWall, Face::XMin | Face::XMax) => [0.0, speed, 0.0],
        _ => [0.0; 3],
    }
}

/// Inverse of [`velocity_vector`]: the signed speed, or `None` when the kind carries no speed
/// or the vector has a component off the axis the kind allows on that face.
pub fn speed_from_vector(kind: BoundaryKind, face: Face, vector: [f64; 3]) -> Option<f64> {
    let (axis, sign) = match (kind, face) {
        (BoundaryKind::VelocityInlet, Face::XMin) => (0, 1.0),
        (BoundaryKind::VelocityInlet, Face::XMax) => (0, -1.0),
        (BoundaryKind::VelocityInlet, Face::YMin) => (1, 1.0),
        (BoundaryKind::VelocityInlet, Face::YMax) => (1, -1.0),
        (BoundaryKind::MovingWall, Face::YMin | Face::YMax) => (0, 1.0),
        (BoundaryKind::MovingWall, Face::XMin | Face::XMax) => (1, 1.0),
        _ => return None,
    };
    (0..3)
        .all(|i| i == axis || vector[i] == 0.0)
        .then(|| sign * vector[axis])
}

impl ResolvedStudy {
    /// The lid speed (m/s along +x) of a study with exactly one moving wall on the top face.
    pub fn lid_velocity(&self) -> Option<f64> {
        let mut walls = self
            .boundary_conditions
            .values()
            .filter(|b| b.kind == BoundaryKind::MovingWall);
        let wall = walls.next()?;
        if walls.next().is_some() || wall.faces != [Face::YMax] {
            return None;
        }
        wall.velocity
    }
}

/// Resolve unit-carrying inputs to SI exactly once. Validity is `diagnose`'s alone: a study
/// with any error is refused with every finding, and one without is converted.
pub fn resolve(study: &Study) -> Result<ResolvedStudy, ResolveError> {
    let found = diagnose(study, &Capabilities::unrestricted());
    if has_errors(&found) {
        return Err(ResolveError::Diagnostics(found));
    }
    let si = |quantity: &Quantity| {
        let value = to_si(quantity);
        if value == 0.0 { 0.0 } else { value }
    };
    let boundary_conditions = study
        .boundary_conditions
        .iter()
        .map(|(name, boundary)| {
            let mut faces = boundary.faces.clone();
            faces.sort();
            faces.dedup();
            let resolved = ResolvedBoundary {
                faces,
                kind: boundary.kind,
                velocity: boundary.velocity.as_ref().map(si),
            };
            (name.clone(), resolved)
        })
        .collect();
    Ok(ResolvedStudy {
        schema_version: 1,
        domain: ResolvedDomain {
            length_x: si(&study.domain.length_x),
            length_y: si(&study.domain.length_y),
            length_z: si(&study.domain.length_z),
            open: study.domain.open,
        },
        fluid: ResolvedFluid {
            kinematic_viscosity: si(&study.fluid.kinematic_viscosity),
        },
        boundary_conditions,
        mesh: ResolvedMesh {
            cells_x: study.mesh.cells_x,
            cells_y: study.mesh.cells_y,
            cells_z: study.mesh.cells_z,
        },
        solver: ResolvedSolver {
            end_time: si(&study.solver.end_time),
            write_interval: si(&study.solver.write_interval),
            convergence: study.solver.convergence.clone(),
        },
        monitors: ResolvedMonitors {
            centreline_u_sample_count: study.monitors.centreline_u_sample_count,
        },
        raw_overrides: study.raw_overrides.clone(),
    })
}
