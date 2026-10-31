use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::Quantity;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Study {
    #[serde(deserialize_with = "schema_version")]
    pub schema_version: u32,
    pub domain: Domain,
    pub fluid: Fluid,
    pub boundary_conditions: BoundaryConditions,
    pub mesh: MeshSpec,
    pub solver: SolverControl,
    pub monitors: Monitors,
}

#[derive(Debug, Error)]
pub enum StudyError {
    #[error("UnsupportedSchemaVersion: found {found}, supported version is 0")]
    UnsupportedSchemaVersion { found: u64 },
    #[error("invalid study JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl Study {
    pub fn from_json(json: &str) -> Result<Self, StudyError> {
        let value: serde_json::Value = serde_json::from_str(json)?;
        if let Some(found) = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            && found != 0
        {
            return Err(StudyError::UnsupportedSchemaVersion { found });
        }
        Ok(serde_json::from_value(value)?)
    }
}

fn schema_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let found = u32::deserialize(deserializer)?;
    if found == 0 {
        Ok(found)
    } else {
        Err(de::Error::custom(format!(
            "UnsupportedSchemaVersion: found {found}, supported version is 0"
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Domain {
    pub length_x: Quantity,
    pub length_y: Quantity,
    pub length_z: Quantity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fluid {
    pub kinematic_viscosity: Quantity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundaryConditions {
    pub lid_velocity: Quantity,
    pub moving_wall: WallKind,
    pub stationary_walls: StationaryWallKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum WallKind {
    Lid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum StationaryWallKind {
    NoSlip,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Monitors {
    pub centreline_u_sample_count: u64,
}
