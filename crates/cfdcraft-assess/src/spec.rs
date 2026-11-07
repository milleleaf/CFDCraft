//! What a run monitors, and the text of the files it left.

use std::collections::BTreeMap;

use cfdcraft_monitors::FieldKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A probe function object. `fo` is its name in the case (`probe_1`); `label` is the study's own
/// name for it, shown to people and never written into a case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProbeSpec {
    pub fo: String,
    pub label: String,
    pub field: FieldKind,
}

/// A sampled-line function object (`line_1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineSpec {
    pub fo: String,
    pub label: String,
    pub field: FieldKind,
}

/// What a surface monitor reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceQuantityKind {
    /// The summed flux, the `sum(phi)` column.
    Flux,
    /// The area-averaged pressure, the `areaAverage(p)` column.
    PressureAverage,
    /// The area-integrated pressure, the `areaIntegrate(p)` column.
    PressureIntegral,
    /// The area-integrated temperature gradient, the `areaIntegrate(gradT)` column: a vector, so a
    /// component is named.
    HeatFlux,
}

impl SurfaceQuantityKind {
    /// The column as the solver writes it and the word the series is named by.
    pub(crate) fn column(self) -> (&'static str, &'static str) {
        match self {
            Self::Flux => ("sum(phi)", "flux"),
            Self::PressureAverage => ("areaAverage(p)", "pressure_average"),
            Self::PressureIntegral => ("areaIntegrate(p)", "pressure_integral"),
            Self::HeatFlux => ("areaIntegrate(gradT)", "heat_flux"),
        }
    }
}

/// A surface function object (`surface_1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceSpec {
    pub fo: String,
    pub label: String,
    pub quantity: SurfaceQuantityKind,
    /// Which part (0 x, 1 y, 2 z) of a vector column is read; `None` reads a scalar column.
    #[serde(default)]
    pub component: Option<usize>,
}

/// A force coefficient function object (`force_1`), read for its drag and lift series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForceSpec {
    pub fo: String,
    pub label: String,
}

/// The two wall surfaces whose heat flows must balance when energy is solved. Each names a
/// [`SurfaceSpec`] of the heat flux kind by its function-object name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeatSpec {
    pub hot: String,
    pub cold: String,
}

/// A flux function object on one patch (`balance_<patch>`), kept for the mass balance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchSpec {
    pub fo: String,
    pub patch: String,
    pub inlet: bool,
}

fn default_log_file() -> String {
    "log.simpleFoam".to_owned()
}

fn default_pressure_field() -> String {
    "p".to_owned()
}

/// Everything the verdict depends on besides the files and the settings.
///
/// A member added after the first version carries a default, so a specification written before it
/// still parses and is assessed as it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub probes: Vec<ProbeSpec>,
    pub lines: Vec<LineSpec>,
    pub surfaces: Vec<SurfaceSpec>,
    pub patches: Vec<PatchSpec>,
    pub solves_energy: bool,
    /// The largest last initial residual of the pressure and of the velocity, or `None` for no check.
    pub residual_limits: Option<(f64, f64)>,
    pub transient: bool,
    /// The name of the solver log in the results folder, for the caller that reads it.
    #[serde(default = "default_log_file")]
    pub log_file: String,
    /// The field held to the pressure limit: `p`, or `p_rgh` for a buoyant solver.
    #[serde(default = "default_pressure_field")]
    pub pressure_field: String,
    /// Whether the temperature residual is held to the velocity limit.
    #[serde(default)]
    pub temperature: bool,
    /// Whether probe, surface and force series are judged on periodic settledness (a transient run).
    #[serde(default)]
    pub periodic: bool,
    #[serde(default)]
    pub forces: Vec<ForceSpec>,
    /// The hot and cold wall heat flows of a run that solves energy.
    #[serde(default)]
    pub heat: Option<HeatSpec>,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            probes: Vec::new(),
            lines: Vec::new(),
            surfaces: Vec::new(),
            patches: Vec::new(),
            solves_energy: false,
            residual_limits: None,
            transient: false,
            log_file: default_log_file(),
            pressure_field: default_pressure_field(),
            temperature: false,
            periodic: false,
            forces: Vec::new(),
            heat: None,
        }
    }
}

/// The text of the files, as the caller read them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Files {
    pub log: String,
    /// Function-object name to (start-time folder, file text) pairs in folder order. A line set
    /// carries one pair per write time.
    pub monitor_files: BTreeMap<String, Vec<(String, String)>>,
}
