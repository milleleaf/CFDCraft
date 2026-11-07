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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fluid {
    pub kinematic_viscosity: Quantity,
}

/// A face of the box domain: left (x min), right (x max), bottom (y min) or top (y max).
/// The front and back faces are always empty (two-dimensional case).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum Face {
    XMin,
    XMax,
    YMin,
    YMax,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshSpec {
    pub cells_x: u64,
    pub cells_y: u64,
    pub cells_z: u64,
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
    SimpleLaminar,
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
}

/// A point in the plane of the case (the case is flat, so a point has x and y only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub x: Quantity,
    pub y: Quantity,
    pub field: MonitorField,
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
