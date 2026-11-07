//! What a benchmark comparison is: an object with an id, a test for whether a study is that
//! benchmark, a comparison of result files and the quantities it records.

use cfdcraft_model::ResolvedStudy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::report::GateReport;

/// Read access to the files of a finished run, whether they sit in a results folder or inside a
/// stored result. Paths are relative to the root of the results, with `/` between parts.
///
/// An implementation must refuse an absolute path and any path with a `..` part: `read` returns
/// `None` and `list` returns nothing for it.
pub trait ResultFiles {
    /// The text of a file, or `None` when it is missing, unreadable, not text or refused.
    fn read(&self, rel: &str) -> Option<String>;
    /// The names of the entries directly below `rel_dir`, sorted. Nothing for a missing directory.
    fn list(&self, rel_dir: &str) -> Vec<String>;
}

/// One way in which a study is not the benchmark it was checked against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Mismatch {
    pub what: String,
    pub found: String,
    pub expected: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is {}, expected {}",
            self.what, self.found, self.expected
        )
    }
}

pub trait Benchmark: Send {
    /// The short name, for example `ghia`.
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    /// The source of the published values.
    fn reference(&self) -> &'static str;
    /// Whether the study is this benchmark; the list says what differs when it is not.
    fn matches(&self, study: &ResolvedStudy) -> Result<(), Vec<Mismatch>>;
    /// The verdict for the files of a finished run. Missing or corrupt output is a failed report
    /// with a reason, not a panic.
    fn compare(&self, files: &dyn ResultFiles, study: &ResolvedStudy) -> GateReport;
    /// The numbers the benchmark records for a run, by name.
    fn quantities(&self, files: &dyn ResultFiles, study: &ResolvedStudy) -> Vec<(String, f64)>;
}

/// A tolerance chosen by the user: finite, above zero and at most `factor` times the shipped
/// default, so that a pinned default cannot be widened without limit.
pub fn bound_override(name: &str, value: f64, default: f64, factor: f64) -> Result<f64, String> {
    if value.is_finite() && value > 0.0 && value <= default * factor {
        Ok(value)
    } else {
        Err(format!(
            "{name} must be finite, above zero and at most {factor} times the shipped value {default}"
        ))
    }
}
