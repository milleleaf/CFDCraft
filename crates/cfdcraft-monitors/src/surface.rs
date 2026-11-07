//! Surface field value files: a region header, a `# Time` line naming the columns, then a time and one value per column.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::probes::{Row, Value, read_row};
use crate::series::{MAX_ROWS, MonitorError, Series, check_size, malformed, number};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceFile {
    /// The region as written, for example `patch inlet`.
    pub region: String,
    pub faces: u64,
    pub area: f64,
    /// The column names as written after `# Time`, for example `sum(phi)`.
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

/// The text after the colon of a `# Name : value` header line.
fn header_value<'a>(comment: &'a str, name: &str) -> Option<&'a str> {
    let (key, value) = comment.split_once(':')?;
    (key.trim() == name).then_some(value.trim())
}

/// Read a surface field value file. All of the region, face count, area and column headers are required.
pub fn parse_surface(text: &str) -> Result<SurfaceFile, MonitorError> {
    check_size(text)?;
    let mut region: Option<String> = None;
    let mut faces: Option<u64> = None;
    let mut area: Option<f64> = None;
    let mut columns: Option<Vec<String>> = None;
    let mut rows: Vec<Row> = Vec::new();
    for (at, raw) in text.lines().enumerate() {
        let line = at + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(comment) = trimmed.strip_prefix('#') {
            let comment = comment.trim_start();
            if let Some(names) = comment.strip_prefix("Time")
                && !names.starts_with(':')
            {
                columns = Some(names.split_whitespace().map(str::to_owned).collect());
            } else if let Some(value) = header_value(comment, "Region type") {
                region = Some(value.to_owned());
            } else if let Some(value) = header_value(comment, "Faces") {
                faces = Some(
                    value
                        .parse()
                        .map_err(|_| malformed(line, "the face count is not a whole number"))?,
                );
            } else if let Some(value) = header_value(comment, "Area") {
                area = Some(number(line, value)?);
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
    let missing = |what: &str| malformed(0, format!("the header has no {what}"));
    Ok(SurfaceFile {
        region: region.ok_or_else(|| missing("region type"))?,
        faces: faces.ok_or_else(|| missing("face count"))?,
        area: area.ok_or_else(|| missing("area"))?,
        columns: columns.ok_or_else(|| missing("# Time line"))?,
        rows,
    })
}

/// One series per mapped column, named `<prefix>.<suffix>`.
///
/// `mapping` pairs a column as written in the file with the suffix to give its series. A column the
/// file does not have is skipped, and so is a column of vectors: a surface monitor reads scalars.
pub fn surface_series(file: &SurfaceFile, prefix: &str, mapping: &[(&str, &str)]) -> Vec<Series> {
    mapping
        .iter()
        .filter_map(|(column, suffix)| {
            let at = file.columns.iter().position(|name| name == column)?;
            let mut series = Series::new(format!("{prefix}.{suffix}"));
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

/// One series per column of vectors, holding one component of them: `<prefix>.<column>.x`, `.y` or `.z`.
///
/// `component` is 0 for x, 1 for y, 2 for z; any other number gives nothing. A column that holds a
/// scalar in any row is skipped, and so is every column when the file has no rows, so the scalar
/// columns stay with `surface_series`. The column text is used as written.
pub fn surface_series_component(file: &SurfaceFile, prefix: &str, component: usize) -> Vec<Series> {
    let Some(axis) = ["x", "y", "z"].get(component) else {
        return Vec::new();
    };
    if file.rows.is_empty() {
        return Vec::new();
    }
    file.columns
        .iter()
        .enumerate()
        .filter_map(|(at, column)| {
            let mut series = Series::new(format!("{prefix}.{column}.{axis}"));
            for row in &file.rows {
                match row.values.get(at)? {
                    Value::Vector(v) => series.push(row.time, v[component]),
                    Value::Scalar(_) => return None,
                }
            }
            Some(series)
        })
        .collect()
}
