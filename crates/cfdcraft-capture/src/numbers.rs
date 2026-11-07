use std::path::Path;

use cfdcraft_compare::KeyNumbers;

use crate::bundle::read_regular;
use crate::result::centreline_folder;
use crate::{CaptureError, CapturePlan, FILE_CAP};

fn invalid(file: &str, why: &str) -> CaptureError {
    CaptureError::Invalid(format!("{file}: {why}"))
}

fn text_of(bytes: &[u8], file: &str) -> Result<String, CaptureError> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| invalid(file, "it is not text"))
}

fn finite(word: &str) -> Option<f64> {
    word.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The time of the last `Time = ` line and the first initial residual of each of `fields` after
/// it, in the order given. `file` is the log's name, for the refusal text.
fn last_iteration(
    log: &str,
    file: &str,
    fields: &[String],
) -> Result<(f64, Vec<f64>), CaptureError> {
    let lines: Vec<&str> = log.lines().collect();
    let (at, time_text) = lines
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, line)| line.strip_prefix("Time = ").map(|t| (i, t)))
        .ok_or_else(|| invalid(file, "it has no Time line"))?;
    let time = finite(time_text.trim().trim_end_matches('s'))
        .ok_or_else(|| invalid(file, "its last Time line holds no number"))?;
    let block = &lines[at + 1..];
    let mut residuals = Vec::with_capacity(fields.len());
    for name in fields {
        let marker = format!("Solving for {name},");
        let value = block
            .iter()
            .find_map(|line| {
                line.split_once(marker.as_str())?
                    .1
                    .split_once("Initial residual = ")?
                    .1
                    .split(',')
                    .next()
            })
            .and_then(finite)
            .ok_or_else(|| {
                invalid(
                    file,
                    &format!("its last iteration has no initial residual for {name}"),
                )
            })?;
        residuals.push(value);
    }
    Ok((time, residuals))
}

/// One column of a sample file: rows are `coordinate Ux Uy Uz`, `#` lines are comments.
fn sample_column(text: &str, file: &str, column: usize) -> Result<Vec<f64>, CaptureError> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let row = line
            .split_whitespace()
            .map(finite)
            .collect::<Option<Vec<f64>>>()
            .filter(|row| row.len() == 4)
            .ok_or_else(|| invalid(file, &format!("line {} is not four finite numbers", n + 1)))?;
        out.push(row[column]);
    }
    if out.is_empty() {
        return Err(invalid(file, "it has no sample rows"));
    }
    Ok(out)
}

/// The key numbers of a cavity run: `solver.iterations`, `residual.Ux`, `residual.Uy`,
/// `residual.p` (the first initial residuals of the last solver iteration) and every centreline
/// sample as `centreline.vertical.ux.NNNN` and `centreline.horizontal.uy.NNNN`, in file order.
pub fn key_numbers(results: &Path, end_time: f64) -> Result<KeyNumbers, CaptureError> {
    key_numbers_with(results, end_time, &CapturePlan::cavity())
}

/// [`key_numbers`] for the log and residual fields `plan` names; the centreline samples are read
/// only when the plan asks for them. A plan that is not in the OpenFOAM layout has no numbers
/// derived from its log: the set is empty.
pub fn key_numbers_with(
    results: &Path,
    end_time: f64,
    plan: &CapturePlan,
) -> Result<KeyNumbers, CaptureError> {
    if !plan.time_folder {
        return Ok(KeyNumbers::new());
    }
    let log = text_of(&read_regular(results, &plan.log, FILE_CAP)?, &plan.log)?;
    let (time, residuals) = last_iteration(&log, &plan.log, &plan.residual_fields)?;
    let mut numbers = KeyNumbers::new();
    numbers.insert("solver.iterations".to_owned(), time);
    for (name, value) in plan.residual_fields.iter().zip(residuals) {
        numbers.insert(format!("residual.{name}"), value);
    }
    if !plan.centreline {
        return Ok(numbers);
    }

    let centre = centreline_folder(results, end_time)?;
    for (file, column, prefix) in [
        ("vertical_U.xy", 1, "centreline.vertical.ux"),
        ("horizontal_U.xy", 2, "centreline.horizontal.uy"),
    ] {
        let rel = format!("postProcessing/centreline/{centre}/{file}");
        let text = text_of(&read_regular(results, &rel, FILE_CAP)?, &rel)?;
        for (index, value) in sample_column(&text, &rel, column)?.into_iter().enumerate() {
            numbers.insert(format!("{prefix}.{index:04}"), value);
        }
    }
    Ok(numbers)
}
