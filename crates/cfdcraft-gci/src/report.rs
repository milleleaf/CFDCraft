//! The convergence report for three meshes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::solve::observed_order;

/// The safety factor of the index for three or more meshes.
pub const SAFETY_FACTOR: f64 = 1.25;

/// One mesh: its cell count, the area (2D) it covers and the value of the quantity on it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Level {
    pub cells: u64,
    pub area: f64,
    pub value: f64,
}

/// Settings of one analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Options {
    /// The safety factor (1.25 for three meshes).
    pub safety: f64,
    /// The quantity is sampled from a transient run.
    pub transient: bool,
    /// The runs use wall functions, so the wall distance in wall units changes with the mesh.
    pub wall_function: bool,
    /// The size of the noise in the quantity, when known.
    pub noise: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            safety: SAFETY_FACTOR,
            transient: false,
            wall_function: false,
            noise: None,
        }
    }
}

/// How the three values approach a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Convergence {
    /// The changes shrink in one direction and an order exists.
    Monotone,
    /// The changes alternate in sign and an order exists.
    Oscillatory,
    /// The changes go one way but do not shrink fast enough for an order.
    NoPositiveOrder,
    /// The changes alternate in sign and grow.
    Divergent,
    /// Two neighbouring values are equal, so there is no order.
    Degenerate,
}

/// Something worth saying that does not stop the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Warning {
    Oscillatory,
    OrderOutOfRange,
    AsymptoticOutOfRange,
    Transient,
    WallFunction,
    NoiseLimited,
    NotRefined,
}

/// What the three meshes say about the quantity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GciReport {
    pub convergence: Convergence,
    /// Refinement ratios, fine to medium and medium to coarse.
    pub ratios: [f64; 2],
    /// The values, fine, medium, coarse.
    pub values: [f64; 3],
    pub order: Option<f64>,
    pub extrapolated: Option<f64>,
    pub relative_error_fine: Option<f64>,
    pub extrapolated_error_fine: Option<f64>,
    pub gci_fine: Option<f64>,
    pub gci_coarse: Option<f64>,
    pub asymptotic_ratio: Option<f64>,
    pub warnings: Vec<Warning>,
}

/// Why no report can be made.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize, JsonSchema)]
pub enum GciError {
    #[error("at least three meshes are needed, found {found}")]
    TooFewLevels { found: usize },
    #[error("the meshes are not systematically refined (ratio {ratio})")]
    NotRefined { ratio: f64 },
    #[error("a value or a setting is not a finite number")]
    NonFinite,
    #[error("a mesh has no cells or a non-positive or non-finite area")]
    BadArea,
}

/// A representative cell size of a mesh.
fn spacing(level: &Level) -> Result<f64, GciError> {
    let h = (level.area / level.cells as f64).sqrt();
    if level.cells > 0 && level.area.is_finite() && h.is_finite() && h > 0.0 {
        Ok(h)
    } else {
        Err(GciError::BadArea)
    }
}

/// Keep a number only when it is finite (a zero value makes a relative error meaningless).
fn finite(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}

/// Analyse three meshes given in any order; they are sorted by cell count, finest first.
pub fn analyse(levels: &[Level; 3], options: &Options) -> Result<GciReport, GciError> {
    let mut sorted = *levels;
    sorted.sort_by_key(|level| std::cmp::Reverse(level.cells));
    let [l1, l2, l3] = sorted;
    let (h1, h2, h3) = (spacing(&l1)?, spacing(&l2)?, spacing(&l3)?);
    let values = [l1.value, l2.value, l3.value];
    if !(values.iter().all(|v| v.is_finite()) && options.safety.is_finite()) {
        return Err(GciError::NonFinite);
    }
    let (r21, r32) = (h2 / h1, h3 / h2);
    for ratio in [r21, r32] {
        if ratio.is_nan() || ratio <= 1.0 {
            return Err(GciError::NotRefined { ratio });
        }
    }
    let (e21, e32) = (l2.value - l1.value, l3.value - l2.value);
    let mut report = GciReport {
        convergence: Convergence::NoPositiveOrder,
        ratios: [r21, r32],
        values,
        order: None,
        extrapolated: None,
        relative_error_fine: None,
        extrapolated_error_fine: None,
        gci_fine: None,
        gci_coarse: None,
        asymptotic_ratio: None,
        warnings: Vec::new(),
    };
    if e21 == 0.0 || e32 == 0.0 {
        report.convergence = Convergence::Degenerate;
        report.warnings = warnings(&report, options);
        return Ok(report);
    }
    let s = (e32 / e21).signum();
    let Some(p) = observed_order(r21, r32, (e32 / e21).abs(), s) else {
        report.convergence = if s > 0.0 {
            Convergence::NoPositiveOrder
        } else {
            Convergence::Divergent
        };
        report.warnings = warnings(&report, options);
        return Ok(report);
    };
    let (q21, q32) = (r21.powf(p), r32.powf(p));
    let extrapolated = (q21 * l1.value - l2.value) / (q21 - 1.0);
    let e_a21 = ((l1.value - l2.value) / l1.value).abs();
    let e_a32 = ((l2.value - l3.value) / l2.value).abs();
    let gci_fine = options.safety * e_a21 / (q21 - 1.0);
    let gci_coarse = options.safety * e_a32 / (q32 - 1.0);
    report.convergence = if s > 0.0 {
        Convergence::Monotone
    } else {
        Convergence::Oscillatory
    };
    report.order = Some(p);
    report.extrapolated = finite(extrapolated);
    report.relative_error_fine = finite(e_a21);
    report.extrapolated_error_fine = finite(((extrapolated - l1.value) / extrapolated).abs());
    report.gci_fine = finite(gci_fine);
    report.gci_coarse = finite(gci_coarse);
    report.asymptotic_ratio = finite(gci_coarse / (q21 * gci_fine));
    report.warnings = warnings(&report, options);
    Ok(report)
}

/// The non-blocking remarks, in the order of the `Warning` variants.
fn warnings(report: &GciReport, options: &Options) -> Vec<Warning> {
    let mut out = Vec::new();
    if report.convergence == Convergence::Oscillatory {
        out.push(Warning::Oscillatory);
    }
    if report.order.is_some_and(|p| !(0.5..=2.5).contains(&p)) {
        out.push(Warning::OrderOutOfRange);
    }
    if report
        .asymptotic_ratio
        .is_some_and(|a| !(0.9..=1.1).contains(&a))
    {
        out.push(Warning::AsymptoticOutOfRange);
    }
    if options.transient {
        out.push(Warning::Transient);
    }
    if options.wall_function {
        out.push(Warning::WallFunction);
    }
    if options
        .noise
        .is_some_and(|n| n > 0.5 * (report.values[1] - report.values[0]).abs())
    {
        out.push(Warning::NoiseLimited);
    }
    let [r21, r32] = report.ratios;
    if (r21 - r32).abs() > 1e-6 * r21.max(r32) {
        out.push(Warning::NotRefined);
    }
    out
}

/// Analyse every consecutive triple of three or more meshes, coarsest triple first and the
/// finest triple last.
pub fn analyse_series(levels: &[Level], options: &Options) -> Result<Vec<GciReport>, GciError> {
    if levels.len() < 3 {
        return Err(GciError::TooFewLevels {
            found: levels.len(),
        });
    }
    let mut sorted = levels.to_vec();
    sorted.sort_by_key(|level| std::cmp::Reverse(level.cells));
    let mut reports = Vec::new();
    for triple in sorted.windows(3).rev() {
        reports.push(analyse(&[triple[0], triple[1], triple[2]], options)?);
    }
    Ok(reports)
}
