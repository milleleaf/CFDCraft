//! The recorded inputs and one column of the table.

use std::collections::BTreeMap;

use cfdcraft_compare::{KeyNumbers, Tolerance, Verdict, compare};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ParityIssue;
use crate::levels::Levels;

/// One recorded run of the benchmark under one backend and image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecordedColumn {
    pub backend: String,
    pub image: String,
    pub run: String,
    /// A preview backend is labelled in the report and is never the baseline.
    pub preview: bool,
    /// Whether the run passed the benchmark's own gate.
    pub gate_passed: bool,
    /// `None` means the run has no convergence assessment, which counts as not converged.
    #[serde(default)]
    pub converged: Option<bool>,
    pub numbers: KeyNumbers,
}

impl RecordedColumn {
    /// A column counts towards admission only if its gate passed and it is converged.
    pub(crate) fn counts(&self) -> bool {
        self.gate_passed && self.converged == Some(true)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum GateResult {
    #[serde(rename = "PASS")]
    Pass,
    #[serde(rename = "FAIL")]
    Fail,
}

impl GateResult {
    pub fn from_passed(passed: bool) -> Self {
        if passed { Self::Pass } else { Self::Fail }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
        }
    }
}

/// One quantity of one column against the baseline. The tolerance is the one supplied for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParityRow {
    pub quantity: String,
    pub value: f64,
    pub baseline: f64,
    pub abs_diff: f64,
    pub tolerance: Tolerance,
    pub verdict: Verdict,
}

/// A column of the report: shown whatever its gate and convergence say.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParityColumn {
    pub backend: String,
    pub image: String,
    pub run: String,
    pub preview: bool,
    pub gate: GateResult,
    pub converged: bool,
    pub levels: Levels,
    pub rows: Vec<ParityRow>,
}

/// The rows of one column against the baseline numbers, and the quantities it lacks.
/// A quantity without a supplied tolerance gets no row (the caller reports it once).
pub(crate) fn rows_for(
    baseline: &KeyNumbers,
    column: &RecordedColumn,
    tolerances: &BTreeMap<String, Tolerance>,
) -> (Vec<ParityRow>, Vec<ParityIssue>) {
    let mut rows = Vec::new();
    let mut issues = Vec::new();
    for (quantity, &base) in baseline {
        let Some(&tolerance) = tolerances.get(quantity) else {
            continue;
        };
        let Some(value) = column
            .numbers
            .get(quantity)
            .copied()
            .filter(|v| v.is_finite())
        else {
            issues.push(ParityIssue::QuantityMissing {
                backend: column.backend.clone(),
                quantity: quantity.clone(),
            });
            continue;
        };
        let one = |x: f64| KeyNumbers::from([(quantity.clone(), x)]);
        let c = compare(&one(base), &one(value), tolerance);
        rows.push(ParityRow {
            quantity: quantity.clone(),
            value,
            baseline: base,
            abs_diff: c.max_abs_diff,
            tolerance: c.tolerance,
            verdict: c.verdict,
        });
    }
    (rows, issues)
}
