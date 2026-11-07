//! The heat setup of a buoyant study: the thermal member of the fluid material and of the fluid
//! domain.
//!
//! Names and shapes come from the solver's fluid manual, page `docs/manual/fluid/heat-transfer.md`
//! (parameters of `thermal`, thermal conditions, the fluid's material) and page
//! `docs/manual/fluid/lattice-boltzmann.md` (the time step). Every one of them is `UNPROBED` until
//! a hosted run of the pinned image has shown that the server takes it as written:
//!
//! - the material takes `thermal: {conductivity, specific_heat}` beside its density;
//! - the domain takes `thermal: {initial_temperature, gravity, expansion, temperature_zero, faces}`,
//!   where `faces` holds `{type: "temperature", value}` for a wall held at a temperature and
//!   leaves every other face adiabatic, which is the solver's default;
//! - with the SI display system a bare temperature is kelvin;
//! - `gravity` of the thermal member is the buoyancy's gravity, not the domain's `acceleration`,
//!   which is a plain body force and gives no buoyancy;
//! - a step with heat needs a `time_step` the study's flow speeds can set, because the solver's
//!   automatic one reads inlet and wall speeds and a closed buoyant cavity has none.

use cfdcraft_backend::CompileFailure;
use cfdcraft_model::{BoundaryKind, Face, ResolvedBoundary, ResolvedStudy, ResolvedThermal};
use serde_json::{Map, Value, json};

/// UNPROBED: the specific heat of the fluid in J/(kg K). Only the diffusivity, conductivity over
/// density and specific heat, enters the flow, so any positive value gives the same flow; the
/// conductivity is chosen with it. A wall heat flux read back from the solver is in watts per
/// square metre for this specific heat and the conductivity this module writes.
pub const REFERENCE_SPECIFIC_HEAT: f64 = 1.0;

/// The lattice speed the solver aims at for the fastest flow, from its page on the time step.
pub const TARGET_LATTICE_VELOCITY: f64 = 0.05;

fn unsupported(setting: &str, reason: impl Into<String>) -> CompileFailure {
    CompileFailure::NotSupported {
        setting: setting.to_owned(),
        reason: reason.into(),
    }
}

/// The fixed wall temperatures of the study, in kelvin, by side.
fn fixed(boundary: &ResolvedBoundary) -> Option<f64> {
    match boundary.thermal {
        Some(ResolvedThermal::FixedTemperature { value }) => Some(value),
        _ => None,
    }
}

fn wall_temperatures(study: &ResolvedStudy) -> Vec<f64> {
    study
        .boundary_conditions
        .values()
        .filter_map(fixed)
        .collect()
}

/// The temperature the lattice counts from: the lowest wall temperature, so that the lattice
/// holds small numbers as the solver's verified natural-convection cases do, or the reference
/// temperature when no wall is held. The solver's walls are not invariant under a shift of the
/// temperature scale, which is why this belongs to the case.
pub fn zero(study: &ResolvedStudy) -> Option<f64> {
    let buoyancy = study.physics.as_ref()?.buoyancy?;
    Some(
        wall_temperatures(study)
            .into_iter()
            .fold(buoyancy.reference_temperature, f64::min),
    )
}

/// The speed buoyancy can drive: the square root of gravity times expansion times the wall
/// temperature difference times the height. `None` without buoyancy, or with no temperature
/// difference between walls.
pub fn buoyancy_speed(study: &ResolvedStudy) -> Option<f64> {
    let buoyancy = study.physics.as_ref()?.buoyancy?;
    let temperatures = wall_temperatures(study);
    let high = temperatures
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let low = temperatures.iter().copied().fold(f64::INFINITY, f64::min);
    let speed = (buoyancy.gravity * buoyancy.beta * (high - low) * study.domain.length_y).sqrt();
    (speed.is_finite() && speed > 0.0).then_some(speed)
}

/// The time step for a buoyant study: the one that holds the lattice speed of the fastest
/// possible flow at the solver's target. `None` when the study is not buoyant or has no
/// temperature difference.
pub fn time_step(study: &ResolvedStudy, spacing: f64) -> Option<f64> {
    buoyancy_speed(study).map(|speed| TARGET_LATTICE_VELOCITY * spacing / speed)
}

/// The thermal member of the fluid material: the conductivity that gives the study's Prandtl
/// number for the fluid's viscosity. `None` for a study without buoyancy.
pub fn material(study: &ResolvedStudy, density: f64) -> Result<Option<Value>, CompileFailure> {
    let Some(buoyancy) = study.physics.as_ref().and_then(|p| p.buoyancy) else {
        return Ok(None);
    };
    let diffusivity = study.fluid.kinematic_viscosity / buoyancy.prandtl;
    if !(diffusivity.is_finite() && diffusivity > 0.0) {
        return Err(unsupported(
            "buoyancy",
            "the Prandtl number must be above zero to give the fluid a conductivity",
        ));
    }
    Ok(Some(json!({
        "conductivity": diffusivity * density * REFERENCE_SPECIFIC_HEAT,
        "specific_heat": REFERENCE_SPECIFIC_HEAT,
    })))
}

/// The side of the box a face stands for.
fn side(face: Face) -> Option<&'static str> {
    match face {
        Face::XMin => Some("x_min"),
        Face::XMax => Some("x_max"),
        Face::YMin => Some("y_min"),
        Face::YMax => Some("y_max"),
        Face::Obstacle => None,
    }
}

/// The thermal member of the fluid domain. `None` for a study without buoyancy; a study that
/// holds a wall at a temperature without buoyancy is refused, because the conductivity comes from
/// the Prandtl number of the buoyancy setting.
pub fn domain(study: &ResolvedStudy) -> Result<Option<Value>, CompileFailure> {
    let buoyancy = study.physics.as_ref().and_then(|p| p.buoyancy);
    let held = wall_temperatures(study);
    let Some(buoyancy) = buoyancy else {
        if held.is_empty() {
            return Ok(None);
        }
        return Err(unsupported(
            "thermal condition",
            "a wall held at a temperature needs the buoyancy setting, which gives the fluid its \
             Prandtl number",
        ));
    };
    if study.domain.shape.is_some() {
        return Err(unsupported(
            "thermal condition",
            "no thermal setup is written beside an obstacle",
        ));
    }
    let mut faces = Map::new();
    for boundary in study.boundary_conditions.values() {
        let Some(value) = fixed(boundary) else {
            continue;
        };
        if matches!(
            boundary.kind,
            BoundaryKind::VelocityInlet | BoundaryKind::PressureOutlet
        ) {
            return Err(unsupported(
                "thermal condition",
                "a temperature on an inlet or an outlet is not written",
            ));
        }
        for face in &boundary.faces {
            let key = side(*face).ok_or_else(|| {
                unsupported("thermal condition", "no thermal setup is written on a body")
            })?;
            faces.insert(
                key.to_owned(),
                json!({"type": "temperature", "value": value}),
            );
        }
    }
    let lattice_zero = zero(study).unwrap_or(buoyancy.reference_temperature);
    Ok(Some(json!({
        "initial_temperature": buoyancy.reference_temperature,
        "gravity": [0.0, -buoyancy.gravity, 0.0],
        "expansion": buoyancy.beta,
        "temperature_zero": lattice_zero,
        "faces": Value::Object(faces),
    })))
}
