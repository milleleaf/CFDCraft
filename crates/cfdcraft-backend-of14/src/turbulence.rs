//! The turbulence models of a Foundation 14 case: the selection file and the extra field files.
//!
//! The boundary text of the fields (wall functions, inflow values) and the inflow values
//! themselves are the older writer's public items, which work out the values from the model's own
//! `inlet_turbulence` and nowhere else; only the file that selects the model is written here, in
//! the vocabulary of this line.

use cfdcraft_backend::CompileFailure;
use cfdcraft_backend_esi::{case::Patch, fields};
use cfdcraft_model::{ResolvedStudy, TurbulenceModel};

/// The turbulence model of a study; laminar when it has no physics.
pub fn model_of(s: &ResolvedStudy) -> TurbulenceModel {
    s.physics
        .as_ref()
        .map_or(TurbulenceModel::Laminar, |p| p.turbulence.model)
}

/// The model word of a Reynolds-averaged model as this line spells it; `None` for laminar.
pub fn model_word(model: TurbulenceModel) -> Option<&'static str> {
    match model {
        TurbulenceModel::Laminar => None,
        TurbulenceModel::KOmegaSst => Some("kOmegaSST"),
        TurbulenceModel::RealizableKe => Some("realizableKE"),
        TurbulenceModel::RngKEpsilon => Some("RNGkEpsilon"),
    }
}

/// The body of `constant/momentumTransport` of a Reynolds-averaged model.
pub fn momentum_transport_body(word: &str) -> String {
    format!("simulationType RAS;\nRAS\n{{\n    model {word};\n    turbulence on;\n}}\n")
}

/// The turbulence field files, `(file, text)` in the order they are written: `k`, then `omega` or
/// `epsilon`, then `nut`. Empty for a laminar study. A study whose inflow values cannot be worked
/// out is refused, naming the reason.
pub fn field_files(
    s: &ResolvedStudy,
    patches: &[Patch],
) -> Result<Vec<(&'static str, String)>, CompileFailure> {
    let inputs = fields::turbulence_inputs(s).map_err(|e| CompileFailure::Invalid {
        reason: e.to_string(),
    })?;
    Ok(match inputs {
        Some((model, inlet)) => fields::turbulence_files(patches, model, &inlet),
        None => Vec::new(),
    })
}

/// The transported quantities of a model besides the velocity, for the schemes and the solver
/// entries.
pub fn transported(model: TurbulenceModel) -> &'static [&'static str] {
    match model {
        TurbulenceModel::Laminar => &[],
        TurbulenceModel::KOmegaSst => &["k", "omega"],
        TurbulenceModel::RealizableKe | TurbulenceModel::RngKEpsilon => &["k", "epsilon"],
    }
}
