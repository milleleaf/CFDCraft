//! The model, time and numerics nodes of the setup: the turbulence model, the length of a run and
//! the pressure coupling, all as constants.
//!
//! The solver's inlet turbulence choices that need no formula take a turbulence intensity and a
//! hydraulic diameter, and derive the turbulent kinetic energy and the dissipation from them. The
//! study states the kinetic energy and the specific dissipation rate instead, so the two numbers
//! written here are the ones whose derived values equal the study's: see [`inlet_values`].

use crate::setup::{Writer, invalid, number, unsupported};
use cfdcraft_backend::CompileFailure;
use cfdcraft_model::{
    BoundaryKind, ResolvedStudy, ResolvedTime, TurbulenceModel, inlet_turbulence,
};

/// The model constant the solver's inlet relations use (its `cs_turb_cmu`).
pub const CMU: f64 = 0.09;

/// The von Karman constant the solver's inlet relations use (its `cs_turb_xkappa`).
pub const KAPPA: f64 = 0.42;

/// The length of a run: an iteration count, and the time step of a run that marches in time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Run {
    /// The time step in seconds; `None` for a steady run.
    pub dt: Option<f64>,
    /// The number of iterations.
    pub iterations: u64,
}

/// The values the inlet turbulence choice takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InletValues {
    /// The turbulence intensity in percent, as the setup states it.
    pub intensity_percent: f64,
    /// The hydraulic diameter in metres.
    pub hydraulic_diameter: f64,
}

/// The turbulence model of a study.
pub fn model_of(study: &ResolvedStudy) -> TurbulenceModel {
    study
        .physics
        .as_ref()
        .map_or(TurbulenceModel::Laminar, |p| p.turbulence.model)
}

/// The name the setup gives a turbulence model; a model the writer has no node for is refused.
pub fn model_name(model: TurbulenceModel) -> Result<&'static str, CompileFailure> {
    match model {
        TurbulenceModel::Laminar => Ok("off"),
        TurbulenceModel::KOmegaSst => Ok("k-omega-SST"),
        _ => Err(unsupported(
            "turbulence model",
            "only laminar flow and the k-omega SST model are written for this solver",
        )),
    }
}

/// A whole number of at least one within `tolerance` (relative) of `count`.
fn whole(count: f64, tolerance: f64, what: &str) -> Result<u64, CompileFailure> {
    let nearest = count.round();
    let ok = count.is_finite()
        && nearest >= 1.0
        && nearest <= f64::from(i32::MAX)
        && (count - nearest).abs() <= tolerance * nearest;
    if ok {
        Ok(nearest as u64)
    } else {
        Err(invalid(&format!(
            "{what} must be a whole number between 1 and {}",
            i32::MAX
        )))
    }
}

/// The length of the run of a study: a steady run counts iterations as the study counts seconds, a
/// run in time takes the end time over the time step of its mesh level, which must come out whole.
pub fn run_of(study: &ResolvedStudy) -> Result<Run, CompileFailure> {
    let time = study
        .physics
        .as_ref()
        .map_or(ResolvedTime::Steady, |p| p.time);
    match time {
        ResolvedTime::Steady => Ok(Run {
            dt: None,
            iterations: whole(study.solver.end_time, 0.0, "the iteration count")?,
        }),
        ResolvedTime::Transient { .. } => {
            let dt = study
                .effective_delta_t()
                .filter(|dt| dt.is_finite() && *dt > 0.0)
                .ok_or_else(|| invalid("the time step must be above zero"))?;
            Ok(Run {
                dt: Some(dt),
                iterations: whole(
                    study.solver.end_time / dt,
                    1e-9,
                    "the end time divided by the time step",
                )?,
            })
        }
    }
}

/// The speed the inflow turbulence values are built from: the reference velocity of the study,
/// else the speed of a velocity inlet, else the speed of a moving wall.
pub fn reference_speed(study: &ResolvedStudy) -> Option<f64> {
    let driven = |kind: BoundaryKind| {
        study
            .boundary_conditions
            .values()
            .filter(|b| b.kind == kind)
            .find_map(|b| b.velocity.filter(|v| *v != 0.0).map(f64::abs))
    };
    study
        .physics
        .as_ref()
        .and_then(|p| p.turbulence.reference_velocity)
        .or_else(|| driven(BoundaryKind::VelocityInlet))
        .or_else(|| driven(BoundaryKind::MovingWall))
}

/// The intensity and the hydraulic diameter that make the solver derive the study's inflow
/// values at an inlet that moves the flow at `inlet_speed`.
///
/// The solver takes the kinetic energy as k = 1.5 (I U)^2 with the speed U of the inlet face, the
/// dissipation as eps = 10 Cmu^0.75 k^1.5 / (kappa D), and, for the k-omega SST model, the specific
/// dissipation rate as omega = eps / (Cmu k). Solving for the study's k and omega gives
/// I = sqrt(k / 1.5) / U and D = 10 sqrt(k) / (Cmu^0.25 kappa omega).
pub fn inlet_values(
    study: &ResolvedStudy,
    inlet_speed: f64,
) -> Result<InletValues, CompileFailure> {
    let turbulence = study.physics.as_ref().map(|p| &p.turbulence);
    let (Some(intensity), Some(ratio)) = (
        turbulence.and_then(|t| t.intensity),
        turbulence.and_then(|t| t.viscosity_ratio),
    ) else {
        return Err(invalid(
            "an inlet with a turbulence model needs a turbulence intensity and a viscosity ratio",
        ));
    };
    let speed = reference_speed(study)
        .ok_or_else(|| invalid("a turbulence model needs a reference speed"))?;
    let values = inlet_turbulence(intensity, ratio, speed, study.fluid.kinematic_viscosity)
        .ok_or_else(|| invalid("the inflow turbulence values are not finite numbers above zero"))?;
    let diameter = 10.0 * values.k.sqrt() / (CMU.powf(0.25) * KAPPA * values.omega);
    let percent = 100.0 * (values.k / 1.5).sqrt() / inlet_speed;
    if !(percent.is_finite() && percent > 0.0 && diameter.is_finite() && diameter > 0.0) {
        return Err(invalid(
            "the inlet turbulence values are not finite numbers above zero",
        ));
    }
    Ok(InletValues {
        intensity_percent: percent,
        hydraulic_diameter: diameter,
    })
}

/// The time parameters: the stepping mode, the time step of a run in time, the iteration count.
pub(crate) fn write_time(w: &mut Writer, run: &Run) -> Result<(), CompileFailure> {
    w.open("time_parameters", &[]);
    match run.dt {
        None => w.leaf("time_passing", &[], "-1"),
        Some(dt) => {
            w.leaf("time_passing", &[], "0");
            w.leaf("time_step_ref", &[], &number(dt)?);
        }
    }
    w.leaf("iterations", &[], &run.iterations.to_string());
    w.close("time_parameters");
    Ok(())
}

/// The turbulence node. A model other than laminar also states the speed the solver starts its
/// turbulence fields from.
pub(crate) fn write_turbulence(
    w: &mut Writer,
    study: &ResolvedStudy,
) -> Result<(), CompileFailure> {
    let model = model_of(study);
    let name = model_name(model)?;
    if model == TurbulenceModel::Laminar {
        w.empty("turbulence", &[("model", name)]);
        return Ok(());
    }
    let speed = reference_speed(study)
        .ok_or_else(|| invalid("a turbulence model needs a reference speed"))?;
    w.open("turbulence", &[("model", name)]);
    w.leaf("reference_velocity", &[], &number(speed)?);
    w.close("turbulence");
    Ok(())
}

/// The pressure coupling: the steady algorithm for a steady run, the one the solver's own setup
/// picks for a constant time step otherwise.
pub(crate) fn write_algorithm(w: &mut Writer, run: &Run) {
    let choice = if run.dt.is_some() {
        "simplec"
    } else {
        "simple"
    };
    w.empty("velocity_pressure_algo", &[("choice", choice)]);
}
