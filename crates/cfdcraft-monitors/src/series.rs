//! A named series of samples, the joining of series across start-time folders, and the error every parser shares.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::probes::FieldKind;
use crate::xy::LineFile;

/// The most probes one probes file may name.
pub const MAX_PROBES: usize = 64;
/// The most data rows one file may hold.
pub const MAX_ROWS: usize = 200_000;
/// The largest file, in bytes, that is parsed at all.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

/// Why a monitor file was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MonitorError {
    #[error("line {line}: a number is not finite")]
    NonFinite { line: usize },
    #[error("line {line}: {reason}")]
    Malformed { line: usize, reason: String },
    #[error("the file is {bytes} bytes, more than the {max} allowed")]
    TooLarge { bytes: usize, max: usize },
    #[error("the file holds more than {max} rows")]
    TooManyRows { max: usize },
}

pub(crate) fn malformed(line: usize, reason: impl Into<String>) -> MonitorError {
    MonitorError::Malformed {
        line,
        reason: reason.into(),
    }
}

/// One finite number from one word: text that is not a number is malformed, `nan` and `inf` are not finite.
pub(crate) fn number(line: usize, word: &str) -> Result<f64, MonitorError> {
    let value: f64 = word
        .parse()
        .map_err(|_| malformed(line, "a word is not a number"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(MonitorError::NonFinite { line })
    }
}

/// The size ceiling, checked before any parsing.
pub(crate) fn check_size(text: &str) -> Result<(), MonitorError> {
    if text.len() > MAX_BYTES {
        return Err(MonitorError::TooLarge {
            bytes: text.len(),
            max: MAX_BYTES,
        });
    }
    Ok(())
}

/// Samples of one quantity: `times[i]` pairs with `values[i]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Series {
    pub name: String,
    pub times: Vec<f64>,
    pub values: Vec<f64>,
}

impl Series {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            times: Vec::new(),
            values: Vec::new(),
        }
    }

    pub fn push(&mut self, time: f64, value: f64) {
        self.times.push(time);
        self.values.push(value);
    }
}

fn start_of(part: &Series) -> f64 {
    part.times.first().copied().unwrap_or(f64::INFINITY)
}

/// One series from the parts a continued run leaves, one per start-time folder.
///
/// The parts are taken in order of their first time, empty parts are skipped, and the samples are
/// laid out in time order; where a time is repeated the later part's value is kept. The name is the
/// first part's.
pub fn join_series(parts: Vec<Series>) -> Series {
    let name = parts.first().map(|p| p.name.clone()).unwrap_or_default();
    let mut parts: Vec<Series> = parts.into_iter().filter(|p| !p.times.is_empty()).collect();
    parts.sort_by(|a, b| start_of(a).total_cmp(&start_of(b)));
    let mut samples: Vec<(f64, f64)> = parts
        .iter()
        .flat_map(|p| p.times.iter().copied().zip(p.values.iter().copied()))
        .collect();
    // The sort is stable, so equal times stay in part order and the last of them is the later one.
    samples.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut joined = Series::new(name);
    for (time, value) in samples {
        if joined.times.last() == Some(&time) {
            if let Some(slot) = joined.values.last_mut() {
                *slot = value;
            }
        } else {
            joined.push(time, value);
        }
    }
    joined
}

/// The mean of each field component over the points of each sampled line, one sample per file.
///
/// Each pair is a folder time and the line sampled then. The first three columns are the point's
/// coordinates and the field components follow: three for a velocity, one for a pressure. The
/// series are `<prefix>.mean_Ux` and `<prefix>.mean_Uy`, or `<prefix>.mean_p`. A file with no
/// points, or too few columns for the field, adds no sample.
pub fn line_mean_series(files: &[(f64, LineFile)], prefix: &str, field: FieldKind) -> Vec<Series> {
    let (names, offsets): (&[&str], &[usize]) = match field {
        FieldKind::Velocity => (&["mean_Ux", "mean_Uy"], &[3, 4]),
        FieldKind::Pressure => (&["mean_p"], &[3]),
    };
    let width = match field {
        FieldKind::Velocity => 6,
        FieldKind::Pressure => 4,
    };
    let mut out: Vec<Series> = names
        .iter()
        .map(|name| Series::new(format!("{prefix}.{name}")))
        .collect();
    for (time, file) in files {
        if file.rows.is_empty() || file.columns < width {
            continue;
        }
        let count = file.rows.len() as f64;
        for (series, offset) in out.iter_mut().zip(offsets) {
            let sum: f64 = file.rows.iter().filter_map(|row| row.get(*offset)).sum();
            series.push(*time, sum / count);
        }
    }
    out
}
