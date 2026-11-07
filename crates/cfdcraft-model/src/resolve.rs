use std::collections::BTreeMap;

use cfdcraft_shapes::{DomainSpec, Layout, MeshRequest, ShapeSpec};
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

use crate::{
    Quantity, Study,
    diagnostics::{Capabilities, Diagnostic, diagnose, has_errors, render_text},
    raw::RawOverride,
    study::{
        BoundaryKind, Convergence, Face, InitMode, InletProfile, MAX_LEVEL, MonitorField,
        SettleSettings, Shape, SurfaceQuantity, ThermalCondition, TimeMode, TurbulenceModel,
        is_zero_level,
    },
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
    /// Absent when every member is at its default (steady, laminar, no buoyancy, uniform start),
    /// so a study that states none of it keeps its canonical text and hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physics: Option<ResolvedPhysics>,
}

/// The physics in SI. A member at its default is left out of the text.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedPhysics {
    #[serde(skip_serializing_if = "ResolvedTime::is_steady")]
    pub time: ResolvedTime,
    #[serde(skip_serializing_if = "ResolvedTurbulence::is_default")]
    pub turbulence: ResolvedTurbulence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buoyancy: Option<ResolvedBuoyancy>,
    #[serde(skip_serializing_if = "InitMode::is_uniform")]
    pub init: InitMode,
}

/// Steady, or transient with the time step of mesh level 0 in seconds. The step a run uses is
/// [`ResolvedStudy::effective_delta_t`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ResolvedTime {
    Steady,
    Transient { delta_t: f64 },
}

impl ResolvedTime {
    fn is_steady(&self) -> bool {
        matches!(self, Self::Steady)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedTurbulence {
    #[serde(skip_serializing_if = "TurbulenceModel::is_laminar")]
    pub model: TurbulenceModel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viscosity_ratio: Option<f64>,
    /// In m/s.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_velocity: Option<f64>,
}

impl ResolvedTurbulence {
    fn is_default(&self) -> bool {
        self.model.is_laminar()
            && self.intensity.is_none()
            && self.viscosity_ratio.is_none()
            && self.reference_velocity.is_none()
    }
}

/// Boussinesq buoyancy in SI: gravity in m/s2, beta in 1/K, the reference temperature in K.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedBuoyancy {
    pub gravity: f64,
    pub beta: f64,
    pub reference_temperature: f64,
    pub prandtl: f64,
    pub turbulent_prandtl: f64,
}

/// The temperature condition of a wall, in kelvin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolvedThermal {
    FixedTemperature { value: f64 },
    Adiabatic,
}

/// The turbulence values at an inlet, from the intensity and the viscosity ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InletTurbulence {
    /// Turbulent kinetic energy, m2/s2.
    pub k: f64,
    /// Specific dissipation rate, 1/s.
    pub omega: f64,
    /// Dissipation rate, m2/s3.
    pub epsilon: f64,
}

/// The inlet values of the turbulence fields: k = 1.5 (I U)^2, omega = k / (nu x ratio) and
/// epsilon = 0.09 k omega. `None` when an input is not a finite number above zero, or a result
/// overflows.
pub fn inlet_turbulence(
    intensity: f64,
    viscosity_ratio: f64,
    speed: f64,
    nu: f64,
) -> Option<InletTurbulence> {
    let usable = |v: f64| v.is_finite() && v > 0.0;
    if ![intensity, viscosity_ratio, speed, nu]
        .into_iter()
        .all(usable)
    {
        return None;
    }
    let k = 1.5 * (intensity * speed).powi(2);
    let omega = k / (nu * viscosity_ratio);
    let epsilon = 0.09 * k * omega;
    [k, omega, epsilon]
        .into_iter()
        .all(usable)
        .then_some(InletTurbulence { k, omega, epsilon })
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedDomain {
    pub length_x: f64,
    pub length_y: f64,
    pub length_z: f64,
    pub open: bool,
    /// Absent for a box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<ResolvedShape>,
}

/// A shape in metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolvedShape {
    BackwardStep {
        step_height: f64,
        upstream_length: f64,
        startup_length: f64,
    },
    CylinderChannel {
        diameter: f64,
        centre_x: f64,
        centre_y: f64,
    },
}

/// Wall grading ratios. An axis is present only when its ratio is not 1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedGrading {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thermal: Option<ResolvedThermal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<InletProfile>,
}

/// The mesh as the solver gets it: `cells_x` and `cells_y` are the effective counts, already
/// multiplied by 2^level; `level` is kept so the choice stays visible.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedMesh {
    pub cells_x: u64,
    pub cells_y: u64,
    pub cells_z: u64,
    #[serde(skip_serializing_if = "is_zero_level")]
    pub level: u32,
    /// Absent when no axis is graded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grading: Option<ResolvedGrading>,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forces: Vec<ResolvedForce>,
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
    /// The boundary the point lies on; absent for a point in the flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boundary: Option<String>,
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

/// The force coefficients of one wall, with the reference values in SI.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedForce {
    pub label: String,
    pub boundary: String,
    pub reference_velocity: f64,
    pub reference_length: f64,
}

/// What a run watches, with the names the written case gives each monitor. A name counts from 1
/// in label order within its kind and never comes from the label text, so a label cannot reach a
/// case. `label` is the study's own label, for people only; the default probes have none.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MonitorPlan {
    pub probes: Vec<PlannedProbe>,
    pub lines: Vec<PlannedLine>,
    pub surfaces: Vec<PlannedSurface>,
    pub forces: Vec<PlannedForce>,
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
    /// The boundary the point lies on; `None` for a point in the flow.
    pub boundary: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedForce {
    /// Function-object name: `force_1`, `force_2`, ...
    pub fo: String,
    pub label: Option<String>,
    pub boundary: String,
    pub reference_velocity: f64,
    pub reference_length: f64,
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
        if m.probes.is_empty() && m.lines.is_empty() && m.surfaces.is_empty() && m.forces.is_empty()
        {
            let probes = DEFAULT_PROBES
                .iter()
                .enumerate()
                .map(|(i, (fx, fy))| PlannedProbe {
                    fo: format!("probe_{}", i + 1),
                    label: None,
                    x: fx * self.domain.length_x,
                    y: fy * self.domain.length_y,
                    field: MonitorField::Velocity,
                    boundary: None,
                })
                .collect();
            return MonitorPlan {
                probes,
                lines: Vec::new(),
                surfaces: Vec::new(),
                forces: Vec::new(),
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
                boundary: p.boundary.clone(),
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
        let forces = m
            .forces
            .iter()
            .enumerate()
            .map(|(i, f)| PlannedForce {
                fo: format!("force_{}", i + 1),
                label: Some(f.label.clone()),
                boundary: f.boundary.clone(),
                reference_velocity: f.reference_velocity,
                reference_length: f.reference_length,
            })
            .collect();
        MonitorPlan {
            probes,
            lines,
            surfaces,
            forces,
            defaulted: false,
        }
    }

    /// The time step a run uses: the authored step divided by 2^level, so space and time refine
    /// together. `None` for a steady run.
    pub fn effective_delta_t(&self) -> Option<f64> {
        match self.physics.as_ref()?.time {
            ResolvedTime::Transient { delta_t } => {
                Some(delta_t / f64::from(1u32 << self.mesh.level.min(MAX_LEVEL)))
            }
            ResolvedTime::Steady => None,
        }
    }

    /// The block layout of the mesh, or `None` when the shapes crate refuses it.
    pub(crate) fn block_layout(&self) -> Option<Layout> {
        let domain = DomainSpec {
            length_x: self.domain.length_x,
            length_y: self.domain.length_y,
            length_z: self.domain.length_z,
        };
        let shape = match self.domain.shape {
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
        let grading = self.mesh.grading;
        let request = MeshRequest {
            cells_x: self.mesh.cells_x,
            cells_y: self.mesh.cells_y,
            cells_z: self.mesh.cells_z,
            level: self.mesh.level,
            ratio_x: grading.and_then(|g| g.x),
            ratio_y: grading.and_then(|g| g.y),
        };
        cfdcraft_shapes::layout(&domain, &shape, &request).ok()
    }

    /// The cells of the whole mesh as the shape lays them out, `None` when the count overflows
    /// or the layout is refused.
    pub fn cell_count(&self) -> Option<u64> {
        self.block_layout().map(|layout| layout.cell_count)
    }

    /// The volume of the flow domain in m3: the area of its outline times the depth. The plain
    /// box volume when the layout is refused.
    pub fn domain_volume(&self) -> f64 {
        match self.block_layout() {
            Some(layout) => layout.area * self.domain.length_z,
            None => self.domain.length_x * self.domain.length_y * self.domain.length_z,
        }
    }

    /// The volume checkMesh reports for the mesh in m3: the area of the mesh times the depth. It
    /// is larger than [`ResolvedStudy::domain_volume`] when the outline has a circle, because the
    /// mesh joins the points on the circle by straight chords. The plain box volume when the
    /// layout is refused.
    pub fn meshed_volume(&self) -> f64 {
        match self.block_layout() {
            Some(layout) => layout.meshed_area() * self.domain.length_z,
            None => self.domain.length_x * self.domain.length_y * self.domain.length_z,
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
    // 2^level per direction; a level `diagnose` would refuse cannot get here, and the shift
    // saturates rather than panics anyway.
    let refinement = 1u64.checked_shl(study.mesh.level).unwrap_or(u64::MAX);
    // A ratio of exactly 1 is an even mesh, the same as no grading.
    let grading = study.mesh.grading.and_then(|g| {
        let uneven = |r: Option<f64>| r.filter(|r| *r != 1.0);
        let (x, y) = (uneven(g.x), uneven(g.y));
        (x.is_some() || y.is_some()).then_some(ResolvedGrading { x, y })
    });
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
                thermal: boundary.thermal.as_ref().map(|t| match t {
                    ThermalCondition::FixedTemperature { value } => {
                        ResolvedThermal::FixedTemperature { value: si(value) }
                    }
                    ThermalCondition::Adiabatic => ResolvedThermal::Adiabatic,
                }),
                profile: boundary.profile,
            };
            (name.clone(), resolved)
        })
        .collect();
    let physics = study
        .physics
        .as_ref()
        .filter(|p| !p.is_default())
        .map(|p| ResolvedPhysics {
            time: match p.time {
                TimeMode::Steady => ResolvedTime::Steady,
                TimeMode::Transient { delta_t } => ResolvedTime::Transient {
                    delta_t: si(&delta_t),
                },
            },
            turbulence: ResolvedTurbulence {
                model: p.turbulence.model,
                intensity: p.turbulence.intensity,
                viscosity_ratio: p.turbulence.viscosity_ratio,
                reference_velocity: p.turbulence.reference_velocity.as_ref().map(si),
            },
            buoyancy: p.buoyancy.map(|b| ResolvedBuoyancy {
                gravity: b.gravity,
                beta: b.beta,
                reference_temperature: si(&b.reference_temperature),
                prandtl: b.prandtl,
                turbulent_prandtl: b.turbulent_prandtl,
            }),
            init: p.init,
        });
    Ok(ResolvedStudy {
        schema_version: 1,
        domain: ResolvedDomain {
            length_x: si(&study.domain.length_x),
            length_y: si(&study.domain.length_y),
            length_z: si(&study.domain.length_z),
            open: study.domain.open,
            shape: study.domain.shape.as_ref().map(|shape| match shape {
                Shape::BackwardStep(s) => ResolvedShape::BackwardStep {
                    step_height: si(&s.step_height),
                    upstream_length: si(&s.upstream_length),
                    startup_length: si(&s.startup_length),
                },
                Shape::CylinderChannel(c) => ResolvedShape::CylinderChannel {
                    diameter: si(&c.diameter),
                    centre_x: si(&c.centre_x),
                    centre_y: si(&c.centre_y),
                },
            }),
        },
        fluid: ResolvedFluid {
            kinematic_viscosity: si(&study.fluid.kinematic_viscosity),
        },
        boundary_conditions,
        mesh: ResolvedMesh {
            cells_x: study.mesh.cells_x.saturating_mul(refinement),
            cells_y: study.mesh.cells_y.saturating_mul(refinement),
            cells_z: study.mesh.cells_z,
            level: study.mesh.level,
            grading,
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
                    boundary: p.boundary.clone(),
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
            forces: study
                .monitors
                .forces
                .iter()
                .map(|(label, f)| ResolvedForce {
                    label: label.clone(),
                    boundary: f.boundary.clone(),
                    reference_velocity: si(&f.reference_velocity),
                    reference_length: si(&f.reference_length),
                })
                .collect(),
            settle: study.monitors.settle,
        },
        raw_overrides: study.raw_overrides.clone(),
        physics,
    })
}
