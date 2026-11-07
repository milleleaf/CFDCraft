//! Force coefficient files: header lines that name the drag and lift directions, a `# Time` line naming the columns, then a time and one value per column.
//!
//! The header says which way the lift points. A file written with the lift along another axis than
//! the one a comparison expects must be refused by the caller, because the lift column would be
//! read as if it were another force.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::probes::{Row, Value, read_row};
use crate::series::{MAX_ROWS, MonitorError, Series, check_size, malformed};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoefficientFile {
    /// The column names as written after `# Time`, for example `Cd`, `Cl`, `Cl(f)`.
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    /// The direction the lift is measured along, when the header gives it.
    pub lift_dir: Option<[f64; 3]>,
    /// The direction the drag is measured along, when the header gives it.
    pub drag_dir: Option<[f64; 3]>,
}

/// A direction written as `(x y z)`: three finite numbers.
fn direction(line: usize, name: &str, text: &str) -> Result<[f64; 3], MonitorError> {
    let spaced = text.replace(['(', ')'], " ");
    let words: Vec<&str> = spaced.split_whitespace().collect();
    let bad = || malformed(line, format!("{name} is not three finite numbers"));
    if words.len() != 3 {
        return Err(bad());
    }
    let mut out = [0.0; 3];
    for (slot, word) in out.iter_mut().zip(words) {
        *slot = word
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(bad)?;
    }
    Ok(out)
}

/// Read a force coefficient file.
///
/// A header line other than the two directions and the column line is ignored, because the exact
/// wording of the others differs between solver versions; only a data row can be refused.
pub fn parse_coefficients(text: &str) -> Result<CoefficientFile, MonitorError> {
    check_size(text)?;
    let mut columns: Option<Vec<String>> = None;
    let mut lift_dir = None;
    let mut drag_dir = None;
    let mut rows: Vec<Row> = Vec::new();
    for (at, raw) in text.lines().enumerate() {
        let line = at + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(comment) = trimmed.strip_prefix('#') {
            let comment = comment.trim_start();
            let mut words = comment.split_whitespace();
            if words.next() == Some("Time") {
                if !rows.is_empty() {
                    return Err(malformed(line, "a column line follows data"));
                }
                columns = Some(words.map(str::to_owned).collect());
            } else if let Some((key, value)) = comment.split_once(':') {
                match key.trim() {
                    "liftDir" => lift_dir = Some(direction(line, "the lift direction", value)?),
                    "dragDir" => drag_dir = Some(direction(line, "the drag direction", value)?),
                    _ => {}
                }
            }
            continue;
        }
        let Some(names) = &columns else {
            return Err(malformed(line, "data comes before the # Time line"));
        };
        if rows.len() == MAX_ROWS {
            return Err(MonitorError::TooManyRows { max: MAX_ROWS });
        }
        let row = read_row(line, trimmed)?;
        if row.values.len() != names.len() {
            return Err(malformed(
                line,
                format!(
                    "a row holds {} values for {} columns",
                    row.values.len(),
                    names.len()
                ),
            ));
        }
        rows.push(row);
    }
    Ok(CoefficientFile {
        columns: columns.ok_or_else(|| malformed(0, "the header has no # Time line"))?,
        rows,
        lift_dir,
        drag_dir,
    })
}

/// A column name as the last part of a series name: every character that is not a letter, a digit or
/// an underscore becomes an underscore, and trailing underscores go (`Cl(f)` is `Cl_f`).
fn series_suffix(column: &str) -> String {
    let replaced: String = column
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    replaced.trim_end_matches('_').to_owned()
}

/// One series per column, named `<prefix>.<column>`, for example `force_1.Cd` and `force_1.Cl`.
///
/// A column that holds a vector in any row is skipped: a coefficient is a scalar.
pub fn coefficient_series(file: &CoefficientFile, prefix: &str) -> Vec<Series> {
    file.columns
        .iter()
        .enumerate()
        .filter_map(|(at, column)| {
            let mut series = Series::new(format!("{prefix}.{}", series_suffix(column)));
            for row in &file.rows {
                match row.values.get(at)? {
                    Value::Scalar(v) => series.push(row.time, *v),
                    Value::Vector(_) => return None,
                }
            }
            Some(series)
        })
        .collect()
}
