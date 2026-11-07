use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    Quantity,
    migrate::{MigrationNote, migrate_v0_to_v1},
    raw::RawOverride,
};

/// A unit-carrying study, schema version 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Study {
    #[serde(deserialize_with = "schema_version")]
    pub schema_version: u32,
    pub domain: Domain,
    pub fluid: Fluid,
    /// Boundaries keyed by the user's own label. The label appears in diagnostics only and
    /// never in the written case. Labels match `[A-Za-z][A-Za-z0-9_-]{0,31}`.
    pub boundary_conditions: BTreeMap<String, BoundaryCondition>,
    pub mesh: MeshSpec,
    pub solver: SolverControl,
    pub monitors: Monitors,
    /// Raw dictionary overrides keyed by a label (same naming rule as boundaries). For expert
    /// use: each one sets a solver dictionary entry the study does not model. They show in
    /// diffs and are part of the result's identity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub raw_overrides: BTreeMap<String, RawOverride>,
    /// Steady or transient, the turbulence model, buoyancy and how the flow starts. Left out
    /// for the plain steady laminar study.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physics: Option<Physics>,
}

#[derive(Debug, Error)]
pub enum StudyError {
    #[error(
        "UnsupportedSchemaVersion: found {found}, supported versions are 0 (migrated forward) and 1"
    )]
    UnsupportedSchemaVersion { found: u64 },
    #[error("invalid study JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl Study {
    /// The number a study of this shape carries in its own `schema_version`; it is separate from
    /// the number of the file the study is saved in.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Read a study of any supported version, dropping the migration note.
    pub fn from_json(json: &str) -> Result<Self, StudyError> {
        Ok(Self::from_json_with_note(json)?.0)
    }

    /// Read a study of any supported version. A version 0 document is migrated to version 1
    /// and the returned note says what changed.
    pub fn from_json_with_note(json: &str) -> Result<(Self, Option<MigrationNote>), StudyError> {
        let value: serde_json::Value = serde_json::from_str(json)?;
        match value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
        {
            Some(0) => {
                let (study, note) = migrate_v0_to_v1(serde_json::from_value(value)?);
                Ok((study, Some(note)))
            }
            Some(1) | None => Ok((serde_json::from_value(value)?, None)),
            Some(found) => Err(StudyError::UnsupportedSchemaVersion { found }),
        }
    }
}

fn schema_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let found = u32::deserialize(deserializer)?;
    if found == 1 {
        Ok(found)
    } else {
        Err(de::Error::custom(format!(
            "UnsupportedSchemaVersion: found {found}, supported version is 1"
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Domain {
    pub length_x: Quantity,
    pub length_y: Quantity,
    pub length_z: Quantity,
    /// True when the box has open (inlet or outlet) faces. A closed domain has only walls.
    /// Closed unless stated.
    #[serde(default)]
    pub open: bool,
    /// A shape other than the plain box. Left out for a box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
}

/// The outline of the domain when it is not a plain box. The domain lengths still give the
/// bounding box, `[0, length_x]` by `[0, length_y]`; a shape needs an open domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    BackwardStep(BackwardStep),
    CylinderChannel(CylinderChannel),
}

/// A channel that widens by a step. The step is at x = `upstream_length`; upstream of it the
/// channel spans y from `step_height` to `length_y`, downstream it spans the full height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackwardStep {
    pub step_height: Quantity,
    pub upstream_length: Quantity,
    /// Length of a slip section at the inlet, from 0 (none) up to `upstream_length`.
    pub startup_length: Quantity,
}

/// A channel with a round cylinder across the flow. The surface of the cylinder is the face
/// `Obstacle`. The cylinder and a clearance of one diameter around it must lie inside the domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CylinderChannel {
    pub diameter: Quantity,
    pub centre_x: Quantity,
    pub centre_y: Quantity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fluid {
    pub kinematic_viscosity: Quantity,
}

/// A face of the domain: left (x min), right (x max), bottom (y min) or top (y max), and the
/// surface of the cylinder in a cylinder channel. The front and back faces are always empty
/// (two-dimensional case).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum Face {
    XMin,
    XMax,
    YMin,
    YMax,
    /// The surface of the cylinder of a cylinder channel; no other domain has it.
    Obstacle,
}

/// What a boundary does to the flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum BoundaryKind {
    /// Fixed inflow velocity. `velocity` is the speed into the domain, along the inward normal.
    VelocityInlet,
    /// Outflow at gauge pressure zero in kinematic units. It has no value field.
    PressureOutlet,
    /// Wall with zero velocity at the wall.
    NoSlipWall,
    /// Wall without friction.
    SlipWall,
    /// Wall that moves in its own plane. `velocity` is the tangential speed along +x (bottom
    /// and top faces) or +y (left and right faces).
    MovingWall,
    /// Mirror plane.
    Symmetry,
}

/// One named boundary: the box faces it covers, its kind, and a speed where the kind needs one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundaryCondition {
    pub faces: Vec<Face>,
    pub kind: BoundaryKind,
    /// Required for a velocity inlet and a moving wall; absent for every other kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<Quantity>,
    /// The shape of the inflow across an inlet on a left or right face. The velocity is the mean
    /// speed; the peak of a parabola is 1.5 times it. Left out for a flat profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<InletProfile>,
    /// The temperature condition of a wall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal: Option<ThermalCondition>,
}

/// How the inflow speed varies across an inlet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InletProfile {
    /// The same speed everywhere on the inlet.
    Uniform,
    /// Zero at the walls and 1.5 times the mean speed in the middle of the inlet.
    Parabolic,
}

/// The temperature condition of a wall.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ThermalCondition {
    /// The wall is held at this temperature.
    FixedTemperature { value: Quantity },
    /// No heat passes through the wall.
    Adiabatic,
}

/// The physics of the run. A study that states none of it is steady, laminar, without buoyancy
/// and started from a uniform field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Physics {
    #[serde(default, skip_serializing_if = "TimeMode::is_steady")]
    pub time: TimeMode,
    #[serde(default, skip_serializing_if = "Turbulence::is_default")]
    pub turbulence: Turbulence,
    /// Present means the Boussinesq buoyancy model is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buoyancy: Option<Buoyancy>,
    #[serde(default, skip_serializing_if = "InitMode::is_uniform")]
    pub init: InitMode,
}

impl Physics {
    /// True when every member is at its default, so the study means the plain steady laminar run.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Whether the run marches in time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimeMode {
    /// Iterate to a steady state.
    #[default]
    Steady,
    /// March in time. `delta_t` is the step at mesh level 0; each level halves it.
    Transient { delta_t: Quantity },
}

impl TimeMode {
    pub(crate) fn is_steady(&self) -> bool {
        matches!(self, Self::Steady)
    }
}

/// The turbulence model and the inflow values it needs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Turbulence {
    #[serde(default, skip_serializing_if = "TurbulenceModel::is_laminar")]
    pub model: TurbulenceModel,
    /// Turbulence intensity of the inflow, above 0 and at most 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f64>,
    /// Ratio of the turbulent to the molecular viscosity of the inflow, above 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viscosity_ratio: Option<f64>,
    /// The speed the inflow values are built from. Left out to use the speed of the inlet or
    /// moving wall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_velocity: Option<Quantity>,
}

impl Turbulence {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The flow model: none (laminar) or one of three Reynolds-averaged models.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurbulenceModel {
    #[default]
    Laminar,
    KOmegaSst,
    RealizableKe,
    RngKEpsilon,
}

impl TurbulenceModel {
    pub(crate) fn is_laminar(&self) -> bool {
        matches!(self, Self::Laminar)
    }

    /// The model in the words of a study file.
    pub fn word(self) -> &'static str {
        match self {
            Self::Laminar => "laminar",
            Self::KOmegaSst => "k_omega_sst",
            Self::RealizableKe => "realizable_ke",
            Self::RngKEpsilon => "rng_k_epsilon",
        }
    }
}

/// Boussinesq buoyancy: the density varies linearly with temperature in the gravity term only.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Buoyancy {
    /// Gravity in m/s2, acting along -y.
    pub gravity: f64,
    /// Thermal expansion coefficient in 1/K.
    pub beta: f64,
    pub reference_temperature: Quantity,
    pub prandtl: f64,
    pub turbulent_prandtl: f64,
}

/// How the fields start.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InitMode {
    /// The same values everywhere.
    #[default]
    Uniform,
    /// A potential-flow solution of the velocity field.
    Potential,
}

impl InitMode {
    pub(crate) fn is_uniform(&self) -> bool {
        matches!(self, Self::Uniform)
    }
}

/// The most refinement levels a mesh may ask for; each level doubles the cells along x and y.
pub const MAX_LEVEL: u32 = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshSpec {
    pub cells_x: u64,
    pub cells_y: u64,
    pub cells_z: u64,
    /// Refinement level, 0 to 4. The cell counts above are the level 0 counts; level L
    /// multiplies the counts along x and y by 2^L and leaves z alone. Left out for level 0.
    #[serde(default, skip_serializing_if = "is_zero_level")]
    pub level: u32,
    /// Wall grading: cells shrink toward the walls on the axes given. Box domains only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grading: Option<Grading>,
}

/// Symmetric wall grading. Each ratio is the growth of one cell over the next from a wall toward
/// the middle of the domain, from 1 (even cells) up to 1.5. An axis left out is not graded, and
/// a graded axis needs an even cell count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Grading {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

pub(crate) fn is_zero_level(level: &u32) -> bool {
    *level == 0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SolverControl {
    pub solver: SolverKind,
    /// Steady SIMPLE iteration count, represented as solver pseudo-time in seconds.
    pub end_time: Quantity,
    pub write_interval: Quantity,
    #[serde(default)]
    pub convergence: Convergence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SolverKind {
    /// Steady, laminar, no buoyancy. Any physics that differs is refused.
    SimpleLaminar,
    /// The solver follows the study's physics.
    FromPhysics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Convergence {
    pub p: f64,
    #[serde(rename = "U")]
    pub u: f64,
}

impl Default for Convergence {
    fn default() -> Self {
        Self { p: 1e-6, u: 1e-6 }
    }
}

/// What to watch while the solver runs. Every monitor is keyed by the user's own label; the
/// label is shown to people and never written into a case (the case names monitors by their
/// position). Labels match `[A-Za-z][A-Za-z0-9_-]{0,31}`. A study that lists no probe, line or
/// surface is watched at five default velocity points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Monitors {
    pub centreline_u_sample_count: u64,
    /// Point probes, at most 16.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub probes: BTreeMap<String, Probe>,
    /// Straight sample lines, at most 8.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lines: BTreeMap<String, LineMonitor>,
    /// Integrals over a named boundary, at most 8.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub surfaces: BTreeMap<String, SurfaceMonitor>,
    /// Force coefficients on a wall, at most 4.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub forces: BTreeMap<String, ForceMonitor>,
    /// How steady a monitored quantity must be to count as settled. It judges a run and does not
    /// change what is run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settle: Option<SettleSettings>,
}

/// The field a probe or line reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MonitorField {
    Velocity,
    Pressure,
}

/// What a surface monitor reports over its boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceQuantity {
    /// The volumetric flux through the boundary.
    Flux,
    /// The area-averaged pressure on the boundary.
    PressureAverage,
    /// The area-integrated pressure on the boundary.
    PressureIntegral,
    /// The area integral of the temperature gradient along the boundary's axis, in K m. The
    /// boundary needs a thermal condition.
    HeatFlux,
}

/// A point in the plane of the case (the case is flat, so a point has x and y only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub x: Quantity,
    pub y: Quantity,
    pub field: MonitorField,
    /// The boundary the point lies on, for a reading at a wall. Left out for a point in the
    /// flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<String>,
}

/// A straight line of evenly spaced samples; `samples` counts the points, ends included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineMonitor {
    pub x0: Quantity,
    pub y0: Quantity,
    pub x1: Quantity,
    pub y1: Quantity,
    pub samples: u64,
    pub field: MonitorField,
}

/// A quantity integrated over one boundary of the study, named by its label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceMonitor {
    pub boundary: String,
    pub quantity: SurfaceQuantity,
}

/// Drag and lift coefficients of one wall boundary. The coefficients are the force over
/// 1/2 times the reference velocity squared times the reference length (per unit depth).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForceMonitor {
    pub boundary: String,
    pub reference_velocity: Quantity,
    pub reference_length: Quantity,
}

/// Optional tolerances for judging a monitored quantity settled. A member left out uses the
/// built-in value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettleSettings {
    /// The share of the iterations, counted from the end, that is judged (above 0, up to 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_fraction: Option<f64>,
    /// The fewest iterations the judged tail may hold (5 to 100000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_floor: Option<u64>,
    /// The largest spread of the tail, relative to its mean (above 0, up to 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_rel: Option<f64>,
    /// The largest drift across the tail, relative to its mean (above 0, up to 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift_rel: Option<f64>,
}
