use cfdcraft_model::{InitMode, TurbulenceModel};
use thiserror::Error;

/// How fine the mesh is: each step up doubles the cells along x and y.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeshPreset {
    /// Mesh level 0.
    Coarse,
    /// Mesh level 1.
    #[default]
    Medium,
    /// Mesh level 2.
    Fine,
}

impl MeshPreset {
    /// The three presets, coarsest first.
    pub const ALL: [MeshPreset; 3] = [Self::Coarse, Self::Medium, Self::Fine];

    /// The mesh level the preset stands for.
    pub const fn level(self) -> u32 {
        match self {
            Self::Coarse => 0,
            Self::Medium => 1,
            Self::Fine => 2,
        }
    }
}

/// Whether the run iterates to a steady state or marches in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeChoice {
    Steady,
    Transient,
}

/// What a user may choose on top of a template. Everything left out keeps the template's own
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TemplateOptions {
    /// The mesh preset.
    pub mesh: MeshPreset,
    /// The turbulence model, laminar included.
    pub turbulence: Option<TurbulenceModel>,
    /// Steady or transient.
    pub time: Option<TimeChoice>,
    /// The time step at mesh level 0, in seconds; for a transient run only.
    pub delta_t: Option<f64>,
    /// How the fields start.
    pub init: Option<InitMode>,
    /// The Rayleigh number; the heated cavity only.
    pub rayleigh: Option<f64>,
    /// The Reynolds number; the laminar step only.
    pub reynolds: Option<f64>,
}

/// Why a template could not be built.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TemplateError {
    /// No template has this id.
    #[error("there is no template called {id}")]
    UnknownTemplate { id: String },
    /// An option is out of range, or does not apply to this template.
    #[error("the option {option} is out of range or does not apply to this template")]
    BadOption { option: &'static str },
    /// The template's own numbers did not make a study. This is a defect of the template, not of
    /// the options.
    #[error("the template did not make a valid study: {detail}")]
    Internal { detail: String },
}
