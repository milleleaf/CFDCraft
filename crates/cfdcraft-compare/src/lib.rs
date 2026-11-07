//! Compare the key numbers of an original run with those of a rerun.
//!
//! The answer is one of three verdicts:
//!
//! - `reproduced`: both sides hold the same names and every pair is exactly equal;
//! - `within_tolerance`: the names match, some pairs differ, and every difference is inside the stated tolerance;
//! - `differs`: a pair is outside the tolerance, a name is on one side only, or a value is not finite.
//!
//! A pair `a`, `b` is inside the tolerance when both are finite and
//! `|a - b| <= abs + rel * max(|a|, |b|)`.
//!
//! The crate is pure: it reads no files and has no clock, so the same inputs always give the same answer.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Named numbers a run produced, such as iteration counts, residuals and profile samples.
pub type KeyNumbers = BTreeMap<String, f64>;

/// A tolerance was negative or not a finite number.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum CompareError {
    #[error("the {part} tolerance must be a finite number that is not negative, but it is {value}")]
    BadTolerance { part: &'static str, value: f64 },
}

/// How far two numbers may differ and still count as the same: an absolute part plus a part relative to their size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tolerance {
    pub abs: f64,
    pub rel: f64,
}

impl Tolerance {
    /// The tight default for a rerun in the same image.
    pub fn same_image() -> Self {
        Self {
            abs: 1e-12,
            rel: 1e-9,
        }
    }

    /// Refuses a part that is negative or not finite.
    pub fn new(abs: f64, rel: f64) -> Result<Self, CompareError> {
        for (part, value) in [("absolute", abs), ("relative", rel)] {
            if !(value.is_finite() && value >= 0.0) {
                return Err(CompareError::BadTolerance { part, value });
            }
        }
        Ok(Self { abs, rel })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Reproduced,
    WithinTolerance,
    Differs,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reproduced => "reproduced",
            Self::WithinTolerance => "within_tolerance",
            Self::Differs => "differs",
        }
    }
}

/// One quantity that is not identical on both sides. A side that lacks the name is `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QuantityDiff {
    pub name: String,
    pub original: Option<f64>,
    pub rerun: Option<f64>,
    /// `None` when a side is missing or a value is not finite.
    pub abs_diff: Option<f64>,
    pub within: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Comparison {
    pub verdict: Verdict,
    pub tolerance: Tolerance,
    /// How many names were compared: the union of both sides.
    pub compared: usize,
    /// The largest difference over the names present on both sides, 0 when there are none.
    pub max_abs_diff: f64,
    /// Every quantity that is not identical (including those within tolerance), sorted by name.
    pub differing: Vec<QuantityDiff>,
}

/// Compare a rerun with the original under a stated tolerance.
pub fn compare(original: &KeyNumbers, rerun: &KeyNumbers, tolerance: Tolerance) -> Comparison {
    let names: BTreeSet<&str> = original
        .keys()
        .chain(rerun.keys())
        .map(String::as_str)
        .collect();
    let mut differing = Vec::new();
    let mut max_abs_diff = 0.0_f64;
    for &name in &names {
        let (a, b) = (original.get(name).copied(), rerun.get(name).copied());
        let (abs_diff, within) = match (a, b) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => {
                if x == y {
                    continue;
                }
                let diff = (x - y).abs();
                // A difference too large to hold is saturated, so it stays writable as JSON.
                let held = diff.min(f64::MAX);
                max_abs_diff = max_abs_diff.max(held);
                let allowed = tolerance.abs + tolerance.rel * x.abs().max(y.abs());
                (Some(held), diff <= allowed)
            }
            _ => (None, false),
        };
        differing.push(QuantityDiff {
            name: name.to_owned(),
            original: a,
            rerun: b,
            abs_diff,
            within,
        });
    }
    let verdict = if differing.iter().any(|d| !d.within) {
        Verdict::Differs
    } else if differing.is_empty() {
        Verdict::Reproduced
    } else {
        Verdict::WithinTolerance
    };
    Comparison {
        verdict,
        tolerance,
        compared: names.len(),
        max_abs_diff,
        differing,
    }
}
