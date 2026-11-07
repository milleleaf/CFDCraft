//! The ways to give the values of one parameter.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::points::SweepError;

/// The most values one parameter may have.
pub const MAX_VALUES: usize = 1000;

/// The values of one parameter: a list, a range or a number of even steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Values {
    /// The values in the order given.
    List { items: Vec<f64> },
    /// From `start` by `step`; `stop` is included when the steps land on it.
    Range { start: f64, stop: f64, step: f64 },
    /// `count` values spread evenly from `start` to `stop`, both included.
    Steps { start: f64, stop: f64, count: u32 },
}

fn bad(reason: &str) -> SweepError {
    SweepError::BadValues {
        name: String::new(),
        reason: reason.to_owned(),
    }
}

fn too_many(count: usize) -> SweepError {
    SweepError::TooManyValues {
        name: String::new(),
        count,
        max: MAX_VALUES,
    }
}

/// Rounds to 13 significant digits, so `0.1 + 2 x 0.1` reads `0.3`, and turns negative zero into
/// zero.
pub(crate) fn tidy(v: f64) -> f64 {
    let r = format!("{v:.12e}").parse::<f64>().unwrap_or(v);
    if r == 0.0 { 0.0 } else { r }
}

/// How many values there are, checked before any value is built.
pub(crate) fn value_count(values: &Values) -> Result<usize, SweepError> {
    match values {
        Values::List { items } => {
            if items.is_empty() {
                return Err(bad("the list is empty"));
            }
            if items.len() > MAX_VALUES {
                return Err(too_many(items.len()));
            }
            if items.iter().any(|v| !v.is_finite()) {
                return Err(bad("a value is not a finite number"));
            }
            Ok(items.len())
        }
        Values::Range { start, stop, step } => {
            if ![start, stop, step].iter().all(|v| v.is_finite()) {
                return Err(bad("a number of the range is not finite"));
            }
            if *step <= 0.0 {
                return Err(bad("the step must be greater than zero"));
            }
            if stop < start {
                return Err(bad("the stop is below the start"));
            }
            let steps = (stop - start) / step;
            if !steps.is_finite() {
                return Err(too_many(usize::MAX));
            }
            let n = (steps * (1.0 + 1e-12) + 1e-9).floor() + 1.0;
            if n > MAX_VALUES as f64 {
                return Err(too_many(n as usize));
            }
            Ok(n as usize)
        }
        Values::Steps { start, stop, count } => {
            if !start.is_finite() || !stop.is_finite() {
                return Err(bad("a number of the steps is not finite"));
            }
            if *count < 2 {
                return Err(bad("the number of steps must be at least 2"));
            }
            if (*count as usize) > MAX_VALUES {
                return Err(too_many(*count as usize));
            }
            if stop <= start {
                return Err(bad("the stop must be above the start"));
            }
            Ok(*count as usize)
        }
    }
}

/// The values, in order. Each is rounded to 13 significant digits, so a range written as
/// decimals gives those decimals.
pub fn expand(values: &Values) -> Result<Vec<f64>, SweepError> {
    let n = value_count(values)?;
    let out: Vec<f64> = match values {
        Values::List { items } => items.iter().map(|v| tidy(*v)).collect(),
        Values::Range { start, step, .. } => {
            (0..n).map(|i| tidy(start + i as f64 * step)).collect()
        }
        Values::Steps { start, stop, .. } => {
            let gap = (stop - start) / (n - 1) as f64;
            (0..n)
                .map(|i| {
                    if i + 1 == n {
                        tidy(*stop)
                    } else {
                        tidy(start + i as f64 * gap)
                    }
                })
                .collect()
        }
    };
    if out.iter().any(|v| !v.is_finite()) {
        return Err(bad("a value is not a finite number"));
    }
    Ok(out)
}
