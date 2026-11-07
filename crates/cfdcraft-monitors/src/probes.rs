//! Probe files: a `# Probe <i> (<x> <y> <z>)` header per point, then a time and one value per point on each row.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::series::{MAX_PROBES, MAX_ROWS, MonitorError, Series, check_size, malformed, number};

/// Which quantity a monitor reads, and so which shape its values have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum FieldKind {
    Velocity,
    Pressure,
}

/// One value on a row: a scalar, or a vector written in parentheses.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum Value {
    Scalar(f64),
    Vector([f64; 3]),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProbeInfo {
    pub index: usize,
    pub location: [f64; 3],
    /// The point lies outside the mesh, so it has no usable samples.
    pub not_found: bool,
}

/// A time and one value per column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Row {
    pub time: f64,
    pub values: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProbeFile {
    pub probes: Vec<ProbeInfo>,
    pub rows: Vec<Row>,
}

/// The values in a piece of a row: scalars, and vectors as `(a b c)`.
pub(crate) fn read_values(line: usize, text: &str) -> Result<Vec<Value>, MonitorError> {
    let spaced = text.replace('(', " ( ").replace(')', " ) ");
    let mut words = spaced.split_whitespace();
    let mut values = Vec::new();
    while let Some(word) = words.next() {
        if word == "(" {
            let mut vector = [0.0; 3];
            for slot in &mut vector {
                let word = words
                    .next()
                    .ok_or_else(|| malformed(line, "a vector is cut short"))?;
                *slot = number(line, word)?;
            }
            if words.next() != Some(")") {
                return Err(malformed(line, "a vector does not hold three numbers"));
            }
            values.push(Value::Vector(vector));
        } else {
            values.push(Value::Scalar(number(line, word)?));
        }
    }
    Ok(values)
}

/// A data row: the time, then the values.
pub(crate) fn read_row(line: usize, text: &str) -> Result<Row, MonitorError> {
    let (time_word, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    Ok(Row {
        time: number(line, time_word)?,
        values: read_values(line, rest)?,
    })
}

fn probe_header(line: usize, expected: usize, rest: &str) -> Result<ProbeInfo, MonitorError> {
    let (index_text, after) = rest
        .split_once('(')
        .ok_or_else(|| malformed(line, "a probe header holds no location"))?;
    let index: usize = index_text
        .trim()
        .parse()
        .map_err(|_| malformed(line, "a probe index is not a whole number"))?;
    if index != expected {
        return Err(malformed(line, "probe indexes must count up from 0"));
    }
    let (coords, tail) = after
        .split_once(')')
        .ok_or_else(|| malformed(line, "a probe location is not closed"))?;
    let words: Vec<&str> = coords.split_whitespace().collect();
    if words.len() != 3 {
        return Err(malformed(line, "a probe location needs three numbers"));
    }
    let mut location = [0.0; 3];
    for (slot, word) in location.iter_mut().zip(words) {
        *slot = number(line, word)?;
    }
    Ok(ProbeInfo {
        index,
        location,
        not_found: tail.contains("Not Found"),
    })
}

/// Read a probes file. Comment lines other than the probe headers are ignored.
pub fn parse_probes(text: &str) -> Result<ProbeFile, MonitorError> {
    check_size(text)?;
    let mut probes: Vec<ProbeInfo> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    for (at, raw) in text.lines().enumerate() {
        let line = at + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(comment) = trimmed.strip_prefix('#') {
            // The index line `#  Probe  0  1` has no location and is an ordinary comment.
            if let Some(rest) = comment.trim_start().strip_prefix("Probe ")
                && rest.contains('(')
            {
                if !rows.is_empty() {
                    return Err(malformed(line, "a probe header follows data"));
                }
                if probes.len() == MAX_PROBES {
                    return Err(malformed(line, format!("more than {MAX_PROBES} probes")));
                }
                probes.push(probe_header(line, probes.len(), rest)?);
            }
            continue;
        }
        if rows.len() == MAX_ROWS {
            return Err(MonitorError::TooManyRows { max: MAX_ROWS });
        }
        let row = read_row(line, trimmed)?;
        if row.values.len() != probes.len() {
            return Err(malformed(
                line,
                format!(
                    "a row holds {} values for {} probes",
                    row.values.len(),
                    probes.len()
                ),
            ));
        }
        rows.push(row);
    }
    Ok(ProbeFile { probes, rows })
}

/// The series of one probe: `<prefix>.Ux` and `<prefix>.Uy` for a velocity, `<prefix>.p` for a pressure.
///
/// The series are returned empty for an unknown or not-found probe, and a row whose value has the
/// wrong shape for `field` adds no sample.
pub fn probe_series(
    file: &ProbeFile,
    prefix: &str,
    probe_index: usize,
    field: FieldKind,
) -> Vec<Series> {
    let usable = file.probes.get(probe_index).is_some_and(|p| !p.not_found);
    let values = file
        .rows
        .iter()
        .filter(|_| usable)
        .filter_map(|row| Some((row.time, *row.values.get(probe_index)?)));
    match field {
        FieldKind::Velocity => {
            let mut ux = Series::new(format!("{prefix}.Ux"));
            let mut uy = Series::new(format!("{prefix}.Uy"));
            for (time, value) in values {
                if let Value::Vector([x, y, _]) = value {
                    ux.push(time, x);
                    uy.push(time, y);
                }
            }
            vec![ux, uy]
        }
        FieldKind::Pressure => {
            let mut p = Series::new(format!("{prefix}.p"));
            for (time, value) in values {
                if let Value::Scalar(v) = value {
                    p.push(time, v);
                }
            }
            vec![p]
        }
    }
}
