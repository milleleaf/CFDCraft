use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

use crate::{
    Quantity, Study,
    diagnostics::{Capabilities, Diagnostic, diagnose, has_errors, render_text},
    raw::RawOverride,
    study::{BoundaryKind, Convergence, Face, MonitorField, SettleSettings, SurfaceQuantity},
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

/// The monitors in SI. Each list is in label order and left out when empty, and `settle` when
/// absent, so a study that lists none keeps its canonical text and hash.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedMonitors {
    pub centreline_u_sample_count: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub probes: Vec<ResolvedProbe>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<ResolvedLine>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<ResolvedSurface>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle: Option<SettleSettings>,
}

/// A probe with its position in metres.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedProbe {
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub field: MonitorField,
}

/// A line with its ends in metres.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedLine {
    pub label: String,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub samples: u64,
    pub field: MonitorField,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedSurface {
    pub label: String,
    pub boundary: String,
    pub quantity: SurfaceQuantity,
}

/// What a run watches, with the names the written case gives each monitor. A name counts from 1
/// in label order within its kind and never comes from the label text, so a label cannot reach a
/// case. `label` is the study's own label, for people only; the default probes have none.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MonitorPlan {
    pub probes: Vec<PlannedProbe>,
    pub lines: Vec<PlannedLine>,
    pub surfaces: Vec<PlannedSurface>,
    /// True when the study lists no monitor and the plan holds the five default probes.
    pub defaulted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedProbe {
    /// Function-object name: `probe_1`, `probe_2`, ...
    pub fo: String,
    pub label: Option<String>,
    pub x: f64,
    pub y: f64,
    pub field: MonitorField,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedLine {
    /// Function-object name: `line_1`, `line_2`, ...
    pub fo: String,
    pub label: Option<String>,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub samples: u64,
    pub field: MonitorField,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedSurface {
    /// Function-object name: `surface_1`, `surface_2`, ...
    pub fo: String,
    pub label: Option<String>,
    pub boundary: String,
    /// What the named boundary is, which decides the patch the case writes for it.
    pub kind: BoundaryKind,
    pub quantity: SurfaceQuantity,
}

/// Where the default probes sit, as fractions of the domain's x and y lengths.
const DEFAULT_PROBES: [(f64, f64); 5] = [
    (0.5, 0.5),
    (0.25, 0.25),
    (0.75, 0.25),
    (0.25, 0.75),
    (0.75, 0.75),
];

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
    /// The monitors a run watches. A study that lists none is watched at five default velocity
    /// probes; the defaults are computed here and never written into the resolved study, so the
    /// study keeps its hash.
    pub fn monitor_plan(&self) -> MonitorPlan {
        let m = &self.monitors;
        if m.probes.is_empty() && m.lines.is_empty() && m.surfaces.is_empty() {
            let probes = DEFAULT_PROBES
                .iter()
                .enumerate()
                .map(|(i, (fx, fy))| PlannedProbe {
                    fo: format!("probe_{}", i + 1),
                    label: None,
                    x: fx * self.domain.length_x,
                    y: fy * self.domain.length_y,
                    field: MonitorField::Velocity,
                })
                .collect();
            return MonitorPlan {
                probes,
                lines: Vec::new(),
                surfaces: Vec::new(),
                defaulted: true,
            };
        }
        let probes = m
            .probes
            .iter()
            .enumerate()
            .map(|(i, p)| PlannedProbe {
                fo: format!("probe_{}", i + 1),
                label: Some(p.label.clone()),
                x: p.x,
                y: p.y,
                field: p.field,
            })
            .collect();
        let lines = m
            .lines
            .iter()
            .enumerate()
            .map(|(i, l)| PlannedLine {
                fo: format!("line_{}", i + 1),
                label: Some(l.label.clone()),
                x0: l.x0,
                y0: l.y0,
                x1: l.x1,
                y1: l.y1,
                samples: l.samples,
                field: l.field,
            })
            .collect();
        // A boundary the study lacks cannot reach here (`resolve` refuses it); the numbering is
        // taken before the filter so a name never shifts.
        let surfaces = m
            .surfaces
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let kind = self.boundary_conditions.get(&s.boundary)?.kind;
                Some(PlannedSurface {
                    fo: format!("surface_{}", i + 1),
                    label: Some(s.label.clone()),
                    boundary: s.boundary.clone(),
                    kind,
                    quantity: s.quantity,
                })
            })
            .collect();
        MonitorPlan {
            probes,
            lines,
            surfaces,
            defaulted: false,
        }
    }

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
            probes: study
                .monitors
                .probes
                .iter()
                .map(|(label, p)| ResolvedProbe {
                    label: label.clone(),
                    x: si(&p.x),
                    y: si(&p.y),
                    field: p.field,
                })
                .collect(),
            lines: study
                .monitors
                .lines
                .iter()
                .map(|(label, l)| ResolvedLine {
                    label: label.clone(),
                    x0: si(&l.x0),
                    y0: si(&l.y0),
                    x1: si(&l.x1),
                    y1: si(&l.y1),
                    samples: l.samples,
                    field: l.field,
                })
                .collect(),
            surfaces: study
                .monitors
                .surfaces
                .iter()
                .map(|(label, s)| ResolvedSurface {
                    label: label.clone(),
                    boundary: s.boundary.clone(),
                    quantity: s.quantity,
                })
                .collect(),
            settle: study.monitors.settle,
        },
        raw_overrides: study.raw_overrides.clone(),
    })
}
