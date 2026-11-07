//! The parity table: recorded runs of one benchmark under different backends and images, set
//! against a baseline, quantity by quantity.
//!
//! A backend or image becomes supported or default only after golden compile, reference oracle and
//! parity all pass. This crate derives the parity part from recorded runs each time, with no new
//! stored shape.
//!
//! Rules the builder keeps:
//!
//! - the tolerance of a quantity is the one supplied for it; this crate only reads tolerances and
//!   never widens, rounds or replaces one;
//! - a column whose gate failed, or that is not converged, is shown and marked, never hidden, and
//!   never counts towards admission;
//! - with no column that counts, every level of the table is `not_run`.
//!
//! The crate is pure: no files, no clock, no solver.

mod column;
mod levels;
mod render;

use std::collections::{BTreeMap, BTreeSet};

use cfdcraft_compare::{KeyNumbers, Tolerance, Verdict};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use column::{GateResult, ParityColumn, ParityRow, RecordedColumn};
pub use levels::{Level, Levels};
pub use render::render_table;

use column::rows_for;
use levels::{aggregate, column_levels};

/// Everything the table is built from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParityInput {
    pub benchmark: String,
    pub case: String,
    pub baseline: RecordedColumn,
    pub columns: Vec<RecordedColumn>,
    /// The tolerance of each quantity: the width the benchmark's own gate allows.
    pub tolerances: BTreeMap<String, Tolerance>,
    /// The golden compile result of each backend, by backend id; a missing id is `not_run`.
    pub golden: BTreeMap<String, Level>,
}

/// The run all columns are compared with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Baseline {
    pub backend: String,
    pub image: String,
    pub run: String,
    /// The baseline's own gate result, shown beside the table. A failing gate does not stop it being the baseline.
    pub gate: GateResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParityReport {
    pub benchmark: String,
    pub case: String,
    pub baseline: Baseline,
    pub columns: Vec<ParityColumn>,
    /// The levels over the columns that count (gate passed and converged).
    pub levels: Levels,
    /// Whether each column is converged, by backend id.
    pub converged: BTreeMap<String, bool>,
}

impl ParityReport {
    /// True only when all three levels pass over at least one counted column.
    pub fn admitted(&self) -> bool {
        self.levels.all_pass()
    }
}

/// Something the table could not show or that stops a column from counting.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParityIssue {
    /// The baseline is a preview run, or holds no usable number.
    #[error("there is no usable baseline column")]
    BaselineMissing,
    /// A quantity of the baseline is missing, or not a finite number, in a column.
    #[error("backend {backend} has no usable value for {quantity}")]
    QuantityMissing { backend: String, quantity: String },
    #[error("backend {backend} is not converged or has no convergence assessment")]
    NotConverged { backend: String },
    #[error("backend {backend} failed its gate")]
    GateFailed { backend: String },
    /// A baseline quantity has no supplied tolerance, so it has no row in any column.
    #[error("quantity {quantity} has no tolerance")]
    NoTolerance { quantity: String },
}

/// The baseline numbers that are finite numbers; a value JSON cannot hold is left out.
fn finite(numbers: &KeyNumbers) -> KeyNumbers {
    numbers
        .iter()
        .filter(|(_, v)| v.is_finite())
        .map(|(k, v)| (k.clone(), *v))
        .collect()
}

/// Build the report and the list of issues. The report always holds every column.
pub fn build(input: &ParityInput) -> (ParityReport, Vec<ParityIssue>) {
    let mut issues = Vec::new();

    let usable = if input.baseline.preview {
        KeyNumbers::new()
    } else {
        finite(&input.baseline.numbers)
    };
    let have_baseline = !usable.is_empty();
    if !have_baseline {
        issues.push(ParityIssue::BaselineMissing);
    }

    let without_tolerance: BTreeSet<&String> = usable
        .keys()
        .filter(|q| !input.tolerances.contains_key(*q))
        .collect();
    for quantity in &without_tolerance {
        issues.push(ParityIssue::NoTolerance {
            quantity: (*quantity).clone(),
        });
    }

    let mut columns = Vec::new();
    let mut counted = Vec::new();
    for col in &input.columns {
        if !col.gate_passed {
            issues.push(ParityIssue::GateFailed {
                backend: col.backend.clone(),
            });
        }
        if col.converged != Some(true) {
            issues.push(ParityIssue::NotConverged {
                backend: col.backend.clone(),
            });
        }
        let (rows, missing) = rows_for(&usable, col, &input.tolerances);
        let parity = if !have_baseline {
            Level::NotRun
        } else if !rows.is_empty()
            && missing.is_empty()
            && without_tolerance.is_empty()
            && rows.iter().all(|r| r.verdict != Verdict::Differs)
        {
            Level::Pass
        } else {
            Level::Fail
        };
        issues.extend(missing);
        let golden = input
            .golden
            .get(&col.backend)
            .copied()
            .unwrap_or(Level::NotRun);
        let levels = column_levels(golden, col.gate_passed, parity);
        if col.counts() {
            counted.push(levels);
        }
        columns.push(ParityColumn {
            backend: col.backend.clone(),
            image: col.image.clone(),
            run: col.run.clone(),
            preview: col.preview,
            gate: GateResult::from_passed(col.gate_passed),
            converged: col.converged == Some(true),
            levels,
            rows,
        });
    }

    let converged = columns
        .iter()
        .map(|c| (c.backend.clone(), c.converged))
        .collect();
    let report = ParityReport {
        benchmark: input.benchmark.clone(),
        case: input.case.clone(),
        baseline: Baseline {
            backend: input.baseline.backend.clone(),
            image: input.baseline.image.clone(),
            run: input.baseline.run.clone(),
            gate: GateResult::from_passed(input.baseline.gate_passed),
        },
        columns,
        levels: aggregate(&counted),
        converged,
    };
    (report, issues)
}
