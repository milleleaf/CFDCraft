//! Table V of the differentially heated cavity bench mark, the Rayleigh number of a study and the
//! scalar judges that put one computed number beside one published number.
//!
//! The velocities of the table are in the paper's units: lengths in units of the cavity side L
//! and velocities in units of kappa / L, so `u* = u L Pr / nu`.

use cfdcraft_gate::{GateMetric, bound_override};
use serde::Deserialize;
use thiserror::Error;

const TABLE: &str = include_str!("../data/dvd_table5.toml");

/// The Prandtl number of the bench mark fluid.
pub const PRANDTL: f64 = 0.71;
/// The Rayleigh number from which the looser allowances apply.
pub const TOP_RA: f64 = 1.0e6;
/// How far a Rayleigh number may be from a tabulated one and still be that case.
pub const RA_MATCH: f64 = 0.005;
/// The most an override may widen a shipped allowance, as a factor.
pub const OVERRIDE_FACTOR: f64 = 3.0;

#[derive(Debug, Error, PartialEq)]
pub enum LoadError {
    #[error("the heated-cavity reference data is not usable: {reason}")]
    Invalid { reason: String },
}

fn invalid(reason: impl Into<String>) -> LoadError {
    LoadError::Invalid {
        reason: reason.into(),
    }
}

/// Where the published numbers come from.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Source {
    pub paper: String,
    pub doi: String,
    pub table: String,
    pub page: u32,
    pub prandtl: f64,
    pub terms: String,
    /// The paper's own statement of its accuracy, printed with every report.
    pub accuracy: String,
}

/// One column of Table V.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Row {
    pub ra: f64,
    pub u_max: f64,
    pub u_max_z: f64,
    pub w_max: f64,
    pub w_max_x: f64,
    pub nu_avg: f64,
    pub nu_0: f64,
    pub nu_max: f64,
    pub nu_max_z: f64,
    pub nu_min: f64,
    pub nu_min_z: f64,
}

impl Row {
    fn values(&self) -> [f64; 11] {
        [
            self.ra,
            self.u_max,
            self.u_max_z,
            self.w_max,
            self.w_max_x,
            self.nu_avg,
            self.nu_0,
            self.nu_max,
            self.nu_max_z,
            self.nu_min,
            self.nu_min_z,
        ]
    }
}

/// The allowances of the gate: fractions of the published value for Nu and the velocities, an
/// absolute distance (in units of the cavity side) for the positions.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Tolerances {
    pub nu_rel: f64,
    pub nu_rel_top: f64,
    pub velocity_rel: f64,
    pub velocity_rel_top: f64,
    pub position_abs: f64,
    pub justification: String,
}

/// A user file that changes some allowances. A member that is left out keeps its shipped value.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverrideFile {
    tolerance: OverrideBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverrideBody {
    nu_rel: Option<f64>,
    nu_rel_top: Option<f64>,
    velocity_rel: Option<f64>,
    velocity_rel_top: Option<f64>,
    position_abs: Option<f64>,
    justification: Option<String>,
}

impl Tolerances {
    /// The allowance for Nu at the row's Rayleigh number.
    pub fn nu_allowance(&self, row: &Row) -> f64 {
        if row.ra >= TOP_RA {
            self.nu_rel_top
        } else {
            self.nu_rel
        }
    }

    /// The allowance for the two velocity maxima at the row's Rayleigh number.
    pub fn velocity_allowance(&self, row: &Row) -> f64 {
        if row.ra >= TOP_RA {
            self.velocity_rel_top
        } else {
            self.velocity_rel
        }
    }

    /// The shipped allowances changed by the `[tolerance]` table of a user file. Every value is
    /// bounded to [`OVERRIDE_FACTOR`] times the shipped one, and the file must say why.
    pub fn from_toml(text: &str, defaults: &Tolerances) -> Result<Tolerances, LoadError> {
        let file: OverrideFile = toml::from_str(text).map_err(|e| invalid(e.to_string()))?;
        let body = file.tolerance;
        let justification = body
            .justification
            .filter(|j| !j.trim().is_empty())
            .ok_or_else(|| invalid("an override needs a justification"))?;
        let bounded = |name: &str, given: Option<f64>, default: f64| match given {
            None => Ok(default),
            Some(value) => bound_override(name, value, default, OVERRIDE_FACTOR).map_err(invalid),
        };
        Ok(Tolerances {
            nu_rel: bounded("nu_rel", body.nu_rel, defaults.nu_rel)?,
            nu_rel_top: bounded("nu_rel_top", body.nu_rel_top, defaults.nu_rel_top)?,
            velocity_rel: bounded("velocity_rel", body.velocity_rel, defaults.velocity_rel)?,
            velocity_rel_top: bounded(
                "velocity_rel_top",
                body.velocity_rel_top,
                defaults.velocity_rel_top,
            )?,
            position_abs: bounded("position_abs", body.position_abs, defaults.position_abs)?,
            justification,
        })
    }
}

#[derive(Deserialize)]
struct Raw {
    source: Source,
    row: Vec<Row>,
    tolerance: Tolerances,
}

/// The published numbers with their source and the shipped allowances.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub source: Source,
    pub rows: Vec<Row>,
    pub tolerances: Tolerances,
}

impl Table {
    /// The table shipped with the crate.
    pub fn load() -> Result<Table, LoadError> {
        Self::parse(TABLE)
    }

    /// Read a table from TOML text. A missing field, a value that is not finite or above zero
    /// where it must be, a repeated Rayleigh number or an empty source text is refused.
    pub fn parse(text: &str) -> Result<Table, LoadError> {
        let raw: Raw = toml::from_str(text).map_err(|e| invalid(e.to_string()))?;
        if raw.row.is_empty() {
            return Err(invalid("the table has no rows"));
        }
        for row in &raw.row {
            if row.values().iter().any(|v| !v.is_finite()) || row.ra <= 0.0 {
                return Err(invalid(format!("a value for Ra {} is not usable", row.ra)));
            }
        }
        for (at, row) in raw.row.iter().enumerate() {
            if raw.row[..at].iter().any(|other| other.ra == row.ra) {
                return Err(invalid(format!("Ra {} appears twice", row.ra)));
            }
        }
        let s = &raw.source;
        if !(s.prandtl.is_finite() && s.prandtl > 0.0) {
            return Err(invalid("the Prandtl number is not usable"));
        }
        let texts = [&s.paper, &s.doi, &s.table, &s.terms, &s.accuracy];
        if texts.iter().any(|t| t.trim().is_empty()) {
            return Err(invalid("a source text is empty"));
        }
        let t = &raw.tolerance;
        let numbers = [
            t.nu_rel,
            t.nu_rel_top,
            t.velocity_rel,
            t.velocity_rel_top,
            t.position_abs,
        ];
        if numbers.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
            return Err(invalid("an allowance is not finite and above zero"));
        }
        if t.justification.trim().is_empty() {
            return Err(invalid("the allowances need a justification"));
        }
        Ok(Table {
            source: raw.source,
            rows: raw.row,
            tolerances: raw.tolerance,
        })
    }

    /// The column whose Rayleigh number the given one matches within [`RA_MATCH`].
    pub fn row_for_ra(&self, ra: f64) -> Option<&Row> {
        if !ra.is_finite() {
            return None;
        }
        self.rows
            .iter()
            .find(|row| (ra - row.ra).abs() <= RA_MATCH * row.ra)
    }
}

/// `Ra = g beta dT L^3 Pr / nu^2`, which is `g beta dT L^3 / (nu kappa)` with `kappa = nu / Pr`.
pub fn rayleigh(gravity: f64, beta: f64, delta_t: f64, length: f64, nu: f64, pr: f64) -> f64 {
    gravity * beta * delta_t * length.powi(3) * pr / (nu * nu)
}

/// A number against a published one, allowed to differ by `allowance` times the published value.
pub fn judge_relative(name: &str, value: f64, reference: f64, allowance: f64) -> GateMetric {
    GateMetric::scalar(
        name,
        value,
        [reference, reference],
        allowance * reference.abs(),
        None,
    )
}

/// A position against a published one, allowed to differ by `allowance` in units of the side.
pub fn judge_position(name: &str, value: f64, reference: f64, allowance: f64) -> GateMetric {
    GateMetric::scalar(name, value, [reference, reference], allowance, None)
}
