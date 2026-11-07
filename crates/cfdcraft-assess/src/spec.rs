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
}

impl SurfaceQuantityKind {
    /// The column as the solver writes it and the word the series is named by.
    pub(crate) fn column(self) -> (&'static str, &'static str) {
        match self {
            Self::Flux => ("sum(phi)", "flux"),
            Self::PressureAverage => ("areaAverage(p)", "pressure_average"),
            Self::PressureIntegral => ("areaIntegrate(p)", "pressure_integral"),
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
}

/// A flux function object on one patch (`balance_<patch>`), kept for the mass balance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchSpec {
    pub fo: String,
    pub patch: String,
    pub inlet: bool,
}

/// Everything the verdict depends on besides the files and the settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
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
