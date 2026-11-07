//! Frozen schema 0 study types and the migration to schema 1.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    Quantity,
    study::{
        BoundaryCondition, BoundaryKind, Domain, Face, Fluid, MeshSpec, Monitors, SolverControl,
        Study,
    },
};

/// Schema 0 study, as written by earlier releases. Read-only: it exists to be migrated.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudyV0 {
    #[serde(deserialize_with = "version_zero")]
    pub schema_version: u32,
    pub domain: DomainV0,
    pub fluid: Fluid,
    pub boundary_conditions: BoundaryConditionsV0,
    pub mesh: MeshSpec,
    pub solver: SolverControl,
    pub monitors: Monitors,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainV0 {
    pub length_x: Quantity,
    pub length_y: Quantity,
    pub length_z: Quantity,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryConditionsV0 {
    pub lid_velocity: Quantity,
    pub moving_wall: WallKind,
    pub stationary_walls: StationaryWallKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum WallKind {
    Lid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum StationaryWallKind {
    NoSlip,
}

fn version_zero<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let found = u32::deserialize(deserializer)?;
    if found == 0 {
        Ok(found)
    } else {
        Err(de::Error::custom(format!(
            "UnsupportedSchemaVersion: found {found}, expected 0"
        )))
    }
}

/// What a migration changed, in plain language, for the caller to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MigrationNote {
    pub from: u32,
    pub to: u32,
    pub summary: String,
}

/// Migrate a schema 0 study to schema 1. Every authored quantity is carried over untouched.
pub fn migrate_v0_to_v1(old: StudyV0) -> (Study, MigrationNote) {
    let boundary_conditions = BTreeMap::from([
        (
            "lid".to_owned(),
            BoundaryCondition {
                faces: vec![Face::YMax],
                kind: BoundaryKind::MovingWall,
                velocity: Some(old.boundary_conditions.lid_velocity),
            },
        ),
        (
            "walls".to_owned(),
            BoundaryCondition {
                faces: vec![Face::XMin, Face::XMax, Face::YMin],
                kind: BoundaryKind::NoSlipWall,
                velocity: None,
            },
        ),
    ]);
    let study = Study {
        schema_version: 1,
        domain: Domain {
            length_x: old.domain.length_x,
            length_y: old.domain.length_y,
            length_z: old.domain.length_z,
            open: false,
        },
        fluid: old.fluid,
        boundary_conditions,
        mesh: old.mesh,
        solver: old.solver,
        monitors: old.monitors,
    };
    let note = MigrationNote {
        from: 0,
        to: 1,
        summary: "Migrated from schema 0 to schema 1. The moving lid became the boundary \
                  \"lid\" (top face, moving wall, same speed). The stationary walls became the \
                  boundary \"walls\" (left, right and bottom faces, no-slip). The domain is \
                  closed."
            .to_owned(),
    };
    (study, note)
}
