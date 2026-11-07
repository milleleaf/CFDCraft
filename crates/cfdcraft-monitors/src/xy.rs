//! Sampled lines in the plain numeric layout: one row of numbers per point, no header.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::series::{MAX_ROWS, MonitorError, check_size, malformed, number};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineFile {
    /// The width every row has.
    pub columns: usize,
    pub rows: Vec<Vec<f64>>,
}

/// Read a sampled line. Blank lines and `#` comments are ignored; every row must have the width of the first.
pub fn parse_xy(text: &str) -> Result<LineFile, MonitorError> {
    check_size(text)?;
    let mut columns = 0;
    let mut rows: Vec<Vec<f64>> = Vec::new();
    for (at, raw) in text.lines().enumerate() {
        let line = at + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if rows.len() == MAX_ROWS {
            return Err(MonitorError::TooManyRows { max: MAX_ROWS });
        }
        let row = trimmed
            .split_whitespace()
            .map(|word| number(line, word))
            .collect::<Result<Vec<f64>, _>>()?;
        if rows.is_empty() {
            columns = row.len();
        } else if row.len() != columns {
            return Err(malformed(
                line,
                format!(
                    "a row holds {} numbers, the first held {columns}",
                    row.len()
                ),
            ));
        }
        rows.push(row);
    }
    Ok(LineFile { columns, rows })
}
