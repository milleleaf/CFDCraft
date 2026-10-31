use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

use crate::{Quantity, Study, Unit, quantity::Dimension, study::Convergence, units::to_si};

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedStudy {
    pub schema_version: u32,
    pub domain: ResolvedDomain,
    pub nu: f64,
    pub lid_velocity: f64,
    pub mesh: ResolvedMesh,
    pub solver: ResolvedSolver,
    pub monitors: ResolvedMonitors,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ResolvedDomain {
    pub length_x: f64,
    pub length_y: f64,
    pub length_z: f64,
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

#[derive(Debug, Error, PartialEq)]
pub enum ResolveError {
    #[error("unsupported schema version {found}; supported version is 0")]
    UnsupportedSchemaVersion { found: u32 },
    #[error("{field} has incompatible unit {unit:?}; expected {expected}")]
    WrongUnit {
        field: &'static str,
        unit: Unit,
        expected: &'static str,
    },
    #[error("{field} is not finite after SI conversion")]
    NonFinite { field: &'static str },
    #[error("{field} must be {requirement}")]
    Invalid {
        field: &'static str,
        requirement: &'static str,
    },
}

fn convert(
    quantity: &Quantity,
    dimension: Dimension,
    field: &'static str,
    expected: &'static str,
) -> Result<f64, ResolveError> {
    if quantity.unit.dimension() != dimension {
        return Err(ResolveError::WrongUnit {
            field,
            unit: quantity.unit,
            expected,
        });
    }
    let value = to_si(quantity);
    if !value.is_finite() {
        return Err(ResolveError::NonFinite { field });
    }
    Ok(if value == 0.0 { 0.0 } else { value })
}

fn positive(value: f64, field: &'static str) -> Result<(), ResolveError> {
    if value <= 0.0 {
        Err(ResolveError::Invalid {
            field,
            requirement: "greater than zero",
        })
    } else {
        Ok(())
    }
}

/// Resolve unit-carrying inputs to SI exactly once and validate the recipe.
pub fn resolve(study: &Study) -> Result<ResolvedStudy, ResolveError> {
    if study.schema_version != 0 {
        return Err(ResolveError::UnsupportedSchemaVersion {
            found: study.schema_version,
        });
    }
    let domain = ResolvedDomain {
        length_x: convert(
            &study.domain.length_x,
            Dimension::Length,
            "domain.length_x",
            "length",
        )?,
        length_y: convert(
            &study.domain.length_y,
            Dimension::Length,
            "domain.length_y",
            "length",
        )?,
        length_z: convert(
            &study.domain.length_z,
            Dimension::Length,
            "domain.length_z",
            "length",
        )?,
    };
    for (field, value) in [
        ("domain.length_x", domain.length_x),
        ("domain.length_y", domain.length_y),
        ("domain.length_z", domain.length_z),
    ] {
        positive(value, field)?;
    }
    let nu = convert(
        &study.fluid.kinematic_viscosity,
        Dimension::KinematicViscosity,
        "fluid.kinematic_viscosity",
        "kinematic viscosity",
    )?;
    positive(nu, "fluid.kinematic_viscosity")?;
    let lid_velocity = convert(
        &study.boundary_conditions.lid_velocity,
        Dimension::Velocity,
        "boundary_conditions.lid_velocity",
        "velocity",
    )?;
    if lid_velocity == 0.0 {
        return Err(ResolveError::Invalid {
            field: "boundary_conditions.lid_velocity",
            requirement: "nonzero",
        });
    }
    for (field, count) in [
        ("mesh.cells_x", study.mesh.cells_x),
        ("mesh.cells_y", study.mesh.cells_y),
        ("mesh.cells_z", study.mesh.cells_z),
    ] {
        if count == 0 {
            return Err(ResolveError::Invalid {
                field,
                requirement: "at least one",
            });
        }
    }
    let end_time = convert(
        &study.solver.end_time,
        Dimension::Time,
        "solver.end_time",
        "time",
    )?;
    let write_interval = convert(
        &study.solver.write_interval,
        Dimension::Time,
        "solver.write_interval",
        "time",
    )?;
    positive(end_time, "solver.end_time")?;
    positive(write_interval, "solver.write_interval")?;
    if end_time.fract() != 0.0
        || write_interval.fract() != 0.0
        || write_interval > end_time
        || end_time % write_interval != 0.0
    {
        return Err(ResolveError::Invalid {
            field: "solver.write_interval",
            requirement: "a positive whole iteration count dividing the whole end_time",
        });
    }
    for (field, value) in [
        ("solver.convergence.p", study.solver.convergence.p),
        ("solver.convergence.U", study.solver.convergence.u),
    ] {
        if !value.is_finite() {
            return Err(ResolveError::NonFinite { field });
        }
        positive(value, field)?;
    }
    if study.monitors.centreline_u_sample_count < 2 {
        return Err(ResolveError::Invalid {
            field: "monitors.centreline_u_sample_count",
            requirement: "at least two",
        });
    }
    Ok(ResolvedStudy {
        schema_version: 0,
        domain,
        nu,
        lid_velocity,
        mesh: ResolvedMesh {
            cells_x: study.mesh.cells_x,
            cells_y: study.mesh.cells_y,
            cells_z: study.mesh.cells_z,
        },
        solver: ResolvedSolver {
            end_time,
            write_interval,
            convergence: study.solver.convergence.clone(),
        },
        monitors: ResolvedMonitors {
            centreline_u_sample_count: study.monitors.centreline_u_sample_count,
        },
    })
}
