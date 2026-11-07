//! Complete, valid studies for the benchmark cases, one builder per case.
//!
//! A template gives a whole study for a mesh preset and a few options (turbulence model, steady
//! or transient, how the fields start, and for two cases the Reynolds or Rayleigh number). The
//! studies are written as JSON and read back through the model's own reader, so a template can
//! only produce what a user could write by hand. Nothing here reads a file or a clock.

mod cavity;
mod cylinder;
mod heated;
mod list;
mod options;
mod step;

use cfdcraft_model::{InitMode, Study, TurbulenceModel};
use serde_json::{Map, Value, json};

pub use heated::viscosity_for_rayleigh;
pub use list::{TemplateInfo, list};
pub use options::{MeshPreset, TemplateError, TemplateOptions, TimeChoice};

/// Build the study of template `id` for the given options.
pub fn build(id: &str, options: &TemplateOptions) -> Result<Study, TemplateError> {
    match id {
        "lid-cavity" => cavity::build(options),
        "backward-step" => step::turbulent(options),
        "backward-step-laminar" => step::laminar(options),
        "cylinder" => cylinder::build(options),
        "heated-cavity" => heated::build(options),
        _ => Err(TemplateError::UnknownTemplate { id: id.to_string() }),
    }
}

/// A quantity as it is written in a study file.
pub(crate) fn q(value: f64, unit: &str) -> Value {
    json!({"value": value, "unit": unit})
}

/// The mesh member: the level-0 cell counts, one cell through z, and the level of the preset
/// (left out at level 0, as a study file leaves it out).
pub(crate) fn mesh_json(cells_x: u64, cells_y: u64, preset: MeshPreset) -> Value {
    let mut mesh = json!({"cells_x": cells_x, "cells_y": cells_y, "cells_z": 1});
    if preset.level() > 0 {
        mesh["level"] = json!(preset.level());
    }
    mesh
}

/// An option that this template does not offer.
pub(crate) fn refuse(value: Option<f64>, option: &'static str) -> Result<(), TemplateError> {
    if value.is_some() {
        Err(TemplateError::BadOption { option })
    } else {
        Ok(())
    }
}

/// How long a run goes on, in the seconds the study counts (iterations for a steady run).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Run {
    pub end: f64,
    pub interval: f64,
}

/// What a template says about its physics and its run; [`apply`] turns it and the user's options
/// into the study's `physics` and `solver` members.
pub(crate) struct Recipe {
    /// The turbulence model when the user does not choose one.
    pub model: TurbulenceModel,
    /// Inflow turbulence intensity and viscosity ratio, used for whichever model is chosen.
    pub intensity: f64,
    pub viscosity_ratio: f64,
    /// The speed the inflow values are built from, for a domain with no inlet or moving wall.
    pub reference_velocity: Option<f64>,
    /// Steady or transient when the user does not choose.
    pub time: TimeChoice,
    pub steady: Run,
    pub transient: Run,
    /// The time step at level 0 of a transient run.
    pub delta_t: f64,
    /// Boussinesq buoyancy, as a study file writes it.
    pub buoyancy: Option<Value>,
    /// Whether a potential-flow start applies to this case.
    pub potential: bool,
    /// Keep the plain steady laminar solver kind while the physics is plain.
    pub plain_solver: bool,
}

/// Write the `physics` member and the solver kind and times into `study`.
pub(crate) fn apply(
    study: &mut Value,
    recipe: &Recipe,
    options: &TemplateOptions,
) -> Result<(), TemplateError> {
    let time = options.time.unwrap_or(recipe.time);
    let transient = time == TimeChoice::Transient;
    // A step is a number above zero, and only a transient run has one.
    if let Some(step) = options.delta_t
        && !(transient && step.is_finite() && step > 0.0)
    {
        return Err(TemplateError::BadOption { option: "delta_t" });
    }
    let potential = options.init == Some(InitMode::Potential);
    if potential && !recipe.potential {
        return Err(TemplateError::BadOption { option: "init" });
    }
    let model = options.turbulence.unwrap_or(recipe.model);
    let mut physics = Map::new();
    if transient {
        let step = options.delta_t.unwrap_or(recipe.delta_t);
        physics.insert(
            "time".to_string(),
            json!({"mode": "transient", "delta_t": q(step, "s")}),
        );
    }
    if model != TurbulenceModel::Laminar {
        let mut turbulence = json!({
            "model": model.word(),
            "intensity": recipe.intensity,
            "viscosity_ratio": recipe.viscosity_ratio
        });
        if let Some(speed) = recipe.reference_velocity {
            turbulence["reference_velocity"] = q(speed, "m/s");
        }
        physics.insert("turbulence".to_string(), turbulence);
    }
    if let Some(buoyancy) = &recipe.buoyancy {
        physics.insert("buoyancy".to_string(), buoyancy.clone());
    }
    if potential {
        physics.insert("init".to_string(), json!("potential"));
    }
    let run = if transient {
        recipe.transient
    } else {
        recipe.steady
    };
    study["solver"]["solver"] = if physics.is_empty() && recipe.plain_solver {
        json!("SimpleLaminar")
    } else {
        json!("FromPhysics")
    };
    study["solver"]["end_time"] = q(run.end, "s");
    study["solver"]["write_interval"] = q(run.interval, "s");
    if !physics.is_empty() {
        study["physics"] = Value::Object(physics);
    }
    Ok(())
}

/// Read the finished JSON through the model's reader.
pub(crate) fn finish(study: &Value) -> Result<Study, TemplateError> {
    Study::from_json(&study.to_string()).map_err(|error| TemplateError::Internal {
        detail: error.to_string(),
    })
}
