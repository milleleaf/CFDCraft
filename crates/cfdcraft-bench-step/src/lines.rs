//! Reading the sampled lines of a finished run, and the study facts both step comparers need.
//!
//! A line monitor writes `postProcessing/line_<n>/<time folder>/line_U.xy`, where `n` counts from
//! 1 in label order. The newest time folder is the one with the largest number.

use cfdcraft_gate::{GateMetric, Mismatch, ResultFiles};
use cfdcraft_model::{BoundaryKind, ResolvedBoundary, ResolvedShape, ResolvedStudy};
use cfdcraft_monitors::parse_xy;
use thiserror::Error;

/// Why a run cannot be judged. Every text starts with the code of a missing or unusable result.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum Refusal {
    #[error("CFD-BENCH-002 the results lack {file}")]
    Missing { file: String },
    #[error("CFD-BENCH-002 {file} cannot be used: {reason}")]
    Unusable { file: String, reason: String },
}

/// The label of the line along the downstream floor.
pub const FLOOR: &str = "floor";

/// The name of a sampled line of velocity: the set is called `line` and the field is `U`.
const LINE_FILE: &str = "line_U.xy";

/// The step height and the x of the step, in metres, or `None` when the study has no step.
pub(crate) fn step_geometry(r: &ResolvedStudy) -> Option<(f64, f64)> {
    match r.domain.shape {
        Some(ResolvedShape::BackwardStep {
            step_height,
            upstream_length,
            ..
        }) => Some((step_height, upstream_length)),
        _ => None,
    }
}

/// The velocity inlet of the study.
pub(crate) fn inlet(r: &ResolvedStudy) -> Option<&ResolvedBoundary> {
    r.boundary_conditions
        .values()
        .find(|b| b.kind == BoundaryKind::VelocityInlet)
}

/// The newest time folder below `dir`.
fn latest_folder(files: &dyn ResultFiles, dir: &str) -> Result<String, Refusal> {
    files
        .list(dir)
        .into_iter()
        .filter_map(|name| {
            let time: f64 = name.parse().ok()?;
            time.is_finite().then_some((name, time))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(name, _)| name)
        .ok_or_else(|| Refusal::Missing {
            file: dir.to_owned(),
        })
}

/// The newest sample file of the line with this label, as (coordinate along `axis`, Ux) pairs in
/// metres and metres per second. `axis` is 0 for a line along x and 1 for a line along y. A file
/// of 4 columns (distance, Ux, Uy, Uz) is placed from the line's start; one of 6 columns
/// (x, y, z, Ux, Uy, Uz) carries its own coordinates. The coordinates must increase.
pub(crate) fn line_samples(
    files: &dyn ResultFiles,
    r: &ResolvedStudy,
    label: &str,
    axis: usize,
) -> Result<Vec<(f64, f64)>, Refusal> {
    let (at, line) = r
        .monitors
        .lines
        .iter()
        .enumerate()
        .find(|(_, l)| l.label == label)
        .ok_or_else(|| Refusal::Unusable {
            file: label.to_owned(),
            reason: "the study has no line with this label".to_owned(),
        })?;
    let dir = format!("postProcessing/line_{}", at + 1);
    let folder = latest_folder(files, &dir)?;
    let rel = format!("{dir}/{folder}/{LINE_FILE}");
    let unusable = |reason: String| Refusal::Unusable {
        file: rel.clone(),
        reason,
    };
    let text = files
        .read(&rel)
        .ok_or_else(|| Refusal::Missing { file: rel.clone() })?;
    let file = parse_xy(&text).map_err(|e| unusable(e.to_string()))?;
    let (start, end) = if axis == 0 {
        (line.x0, line.x1)
    } else {
        (line.y0, line.y1)
    };
    let direction = if end >= start { 1.0 } else { -1.0 };
    let points = file
        .rows
        .iter()
        .map(|row| match (file.columns, row.as_slice()) {
            (4, [distance, ux, ..]) => Some((start + direction * *distance, *ux)),
            (6, [x, y, _, ux, ..]) => Some((if axis == 0 { *x } else { *y }, *ux)),
            _ => None,
        })
        .collect::<Option<Vec<(f64, f64)>>>()
        .ok_or_else(|| unusable("expected rows of 4 or 6 numbers".to_owned()))?;
    if points.len() < 2 {
        return Err(unusable("fewer than two samples".to_owned()));
    }
    if points.windows(2).any(|w| w[0].0 >= w[1].0) {
        return Err(unusable("the coordinates do not increase".to_owned()));
    }
    Ok(points)
}

/// Piecewise-linear interpolation of increasing-abscissa samples, clamped to the end values.
pub(crate) fn interpolate(points: &[(f64, f64)], x: f64) -> f64 {
    let above = points.partition_point(|p| p.0 <= x);
    match (above.checked_sub(1).map(|i| points[i]), points.get(above)) {
        (Some(a), Some(b)) => a.1 + (x - a.0) / (b.0 - a.0) * (b.1 - a.1),
        (Some(a), None) => a.1,
        (None, Some(b)) => b.1,
        (None, None) => f64::NAN,
    }
}

/// One number against a published value with its uncertainty: the value passes when it lies within
/// `tolerance` of `nominal`. The published band (`nominal` plus or minus `half_band`) is shown as
/// the reference range and is not added to the tolerance.
pub(crate) fn nominal_scalar(
    name: &str,
    computed: f64,
    nominal: f64,
    half_band: f64,
    tolerance: f64,
    unit: &str,
) -> GateMetric {
    let mut metric = GateMetric::scalar(
        name,
        computed,
        [nominal - half_band, nominal + half_band],
        tolerance,
        Some(unit),
    );
    let deviation = if computed.is_finite() {
        (computed - nominal).abs()
    } else {
        f64::MAX
    };
    metric.max_abs_dev = deviation;
    metric.passed = computed.is_finite() && deviation <= tolerance;
    metric
}

/// What is wrong with the study's floor line, or `None` when it is as the comparison needs: the
/// line exists, is level and lies above the floor by more than nothing and at most twice `offset`
/// step heights.
pub(crate) fn floor_line_mismatch(
    r: &ResolvedStudy,
    step_height: f64,
    offset: f64,
) -> Option<Mismatch> {
    let Some(line) = r.monitors.lines.iter().find(|l| l.label == FLOOR) else {
        return Some(Mismatch {
            what: "line floor".to_owned(),
            found: "missing".to_owned(),
            expected: "a sampled line of velocity".to_owned(),
        });
    };
    let height = line.y0 / step_height;
    let level = (line.y1 - line.y0).abs() <= 1e-9 * step_height;
    if level && height > 0.0 && height <= 2.0 * offset {
        None
    } else {
        Some(Mismatch {
            what: "floor line height".to_owned(),
            found: format!("{height:.3} step heights"),
            expected: format!("about {offset} step heights"),
        })
    }
}

/// How far a study's geometry and flow may differ from the reference conditions, relative.
const CONDITION_TOLERANCE: f64 = 0.02;

/// Whether `found` is within the condition tolerance of `expected`.
pub(crate) fn near(found: f64, expected: f64) -> bool {
    (found - expected).abs() <= CONDITION_TOLERANCE * expected.abs()
}

pub(crate) fn mismatch(
    what: &str,
    found: impl Into<String>,
    expected: impl Into<String>,
) -> Mismatch {
    Mismatch {
        what: what.to_owned(),
        found: found.into(),
        expected: expected.into(),
    }
}
