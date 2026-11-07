//! The published intervals of the cylinder benchmark, the allowances this project widens them by,
//! and the judgement of one number against an interval.

use cfdcraft_gate::{GateMetric, GateReport, bound_override};
use serde::Deserialize;
use thiserror::Error;

const EMBEDDED: &str = include_str!("../data/st_2d2.toml");

/// The name the report carries.
pub const ID: &str = "cylinder";

/// An override may widen an allowance to this many times the shipped value, no further.
const OVERRIDE_FACTOR: f64 = 3.0;
/// The longest justification an override may give, in characters.
const MAX_JUSTIFICATION: usize = 500;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("the benchmark data is not usable: {reason}")]
    Corrupt { reason: String },
    #[error("the tolerance override is refused: {reason}")]
    Override { reason: String },
}

fn corrupt(reason: impl Into<String>) -> LoadError {
    LoadError::Corrupt {
        reason: reason.into(),
    }
}

fn refused(reason: impl Into<String>) -> LoadError {
    LoadError::Override {
        reason: reason.into(),
    }
}

/// Where the numbers come from.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub paper: String,
    pub doi: String,
    pub table: String,
    pub page: u32,
    pub statement: String,
    pub terms: String,
}

/// How the paper defines the quantities.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub reynolds: String,
    pub drag: String,
    pub lift: String,
    pub strouhal: String,
    pub pressure_difference: String,
    /// The first pressure point, (x, y) in metres.
    pub front_point: [f64; 2],
    /// The second pressure point, (x, y) in metres.
    pub rear_point: [f64; 2],
}

/// A published interval.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interval {
    pub low: f64,
    pub high: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Intervals {
    cd_max: Interval,
    cl_max: Interval,
    st: Interval,
    delta_p: Interval,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTolerance {
    cd_max: f64,
    cl_max: f64,
    st: f64,
    delta_p: f64,
    window_cycles: u64,
    min_cycles: u64,
    units: String,
    justification: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    source: Source,
    definition: Definition,
    reference: Intervals,
    tolerance: RawTolerance,
}

fn load(text: &str) -> Result<File, LoadError> {
    toml::from_str(text).map_err(|e: toml::de::Error| corrupt(e.to_string()))
}

/// The paper's numbers: where they are from, how the quantities are defined, and the four
/// intervals.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub source: Source,
    pub definition: Definition,
    pub cd_max: Interval,
    pub cl_max: Interval,
    pub st: Interval,
    pub delta_p: Interval,
}

fn sound(interval: &Interval, name: &str) -> Result<(), LoadError> {
    if interval.low.is_finite() && interval.high.is_finite() && interval.low <= interval.high {
        Ok(())
    } else {
        Err(corrupt(format!(
            "the interval of {name} is not low <= high"
        )))
    }
}

impl Reference {
    /// The numbers shipped with the crate.
    pub fn embedded() -> Result<Self, LoadError> {
        Self::parse(EMBEDDED)
    }

    /// The numbers of a data file in the shipped layout. A missing member, an unknown member or
    /// an interval with its low above its high is refused.
    pub fn parse(text: &str) -> Result<Self, LoadError> {
        let file = load(text)?;
        let Intervals {
            cd_max,
            cl_max,
            st,
            delta_p,
        } = file.reference;
        for (interval, name) in [
            (&cd_max, "cd_max"),
            (&cl_max, "cl_max"),
            (&st, "st"),
            (&delta_p, "delta_p"),
        ] {
            sound(interval, name)?;
        }
        Ok(Self {
            source: file.source,
            definition: file.definition,
            cd_max,
            cl_max,
            st,
            delta_p,
        })
    }
}

/// The allowances around the intervals, as fractions of an interval's midpoint, and the shedding
/// window. The window cannot be overridden: fewer periods would turn a refusal into a verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct Tolerances {
    pub cd_max: f64,
    pub cl_max: f64,
    pub st: f64,
    pub delta_p: f64,
    /// The last this many whole periods are measured.
    pub window_cycles: u64,
    /// At least this many whole periods must exist before a run is judged.
    pub min_cycles: u64,
    /// Why the allowances are what they are; for an override, the user's reason.
    pub justification: String,
    /// True when the allowances came from an override.
    pub overridden: bool,
}

fn positive(name: &str, value: f64) -> Result<(), LoadError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(corrupt(format!("the allowance {name} is not above zero")))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverrideTable {
    cd_max: Option<f64>,
    cl_max: Option<f64>,
    st: Option<f64>,
    delta_p: Option<f64>,
    justification: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverrideFile {
    tolerance: OverrideTable,
}

impl Tolerances {
    /// The allowances shipped with the crate.
    pub fn default_embedded() -> Result<Self, LoadError> {
        Self::parse(EMBEDDED)
    }

    /// The allowances of a data file in the shipped layout. A missing or unknown member, an
    /// allowance that is not above zero, or a window that does not fit is refused.
    pub fn parse(text: &str) -> Result<Self, LoadError> {
        let t = load(text)?.tolerance;
        if t.units != "fraction_of_midpoint" {
            return Err(corrupt("the allowance units must be fraction_of_midpoint"));
        }
        for (name, value) in [
            ("cd_max", t.cd_max),
            ("cl_max", t.cl_max),
            ("st", t.st),
            ("delta_p", t.delta_p),
        ] {
            positive(name, value)?;
        }
        if t.window_cycles == 0 || t.min_cycles < t.window_cycles {
            return Err(corrupt(
                "the window must be at least one period and fit in the minimum",
            ));
        }
        if t.justification.trim().is_empty() {
            return Err(corrupt("the allowances have no justification"));
        }
        Ok(Self {
            cd_max: t.cd_max,
            cl_max: t.cl_max,
            st: t.st,
            delta_p: t.delta_p,
            window_cycles: t.window_cycles,
            min_cycles: t.min_cycles,
            justification: t.justification,
            overridden: false,
        })
    }

    /// The shipped allowances with the ones a user's file names replaced. Each is above zero and
    /// at most 3 times the shipped value, and the file must say why.
    pub fn from_toml(text: &str) -> Result<Self, LoadError> {
        let given = toml::from_str::<OverrideFile>(text)
            .map_err(|e| refused(e.to_string()))?
            .tolerance;
        let why = given.justification.as_deref().map_or("", str::trim);
        if why.is_empty() {
            return Err(refused("a justification is required"));
        }
        if why.chars().count() > MAX_JUSTIFICATION || why.chars().any(char::is_control) {
            return Err(refused(format!(
                "the justification must be plain text of at most {MAX_JUSTIFICATION} characters"
            )));
        }
        let base = Self::default_embedded()?;
        let pick = |name: &str, value: Option<f64>, shipped: f64| match value {
            Some(v) => bound_override(name, v, shipped, OVERRIDE_FACTOR).map_err(refused),
            None => Ok(shipped),
        };
        Ok(Self {
            cd_max: pick("cd_max", given.cd_max, base.cd_max)?,
            cl_max: pick("cl_max", given.cl_max, base.cl_max)?,
            st: pick("st", given.st, base.st)?,
            delta_p: pick("delta_p", given.delta_p, base.delta_p)?,
            justification: why.to_owned(),
            overridden: true,
            ..base
        })
    }
}

/// One number against a published interval: it passes when it lies inside the interval widened
/// on both sides by `allowance_rel` times the interval's midpoint. The deviation is the distance
/// outside the interval, 0 inside it.
pub fn judge_scalar(name: &str, value: f64, low: f64, high: f64, allowance_rel: f64) -> GateMetric {
    let midpoint = (low + high) / 2.0;
    GateMetric::scalar(
        name,
        value,
        [low, high],
        allowance_rel * midpoint,
        Some("1"),
    )
}

/// The four numbers of the benchmark against their intervals. The report passes only when all
/// four pass; an override is named in its notes.
pub fn judge_all(
    reference: &Reference,
    tolerances: &Tolerances,
    cd_max: f64,
    cl_max: f64,
    st: f64,
    delta_p: f64,
) -> GateReport {
    let one = |name, value, interval: &Interval, allowance| {
        judge_scalar(name, value, interval.low, interval.high, allowance)
    };
    let metrics = vec![
        one("cDmax", cd_max, &reference.cd_max, tolerances.cd_max),
        one("cLmax", cl_max, &reference.cl_max, tolerances.cl_max),
        one("St", st, &reference.st, tolerances.st),
        one("DeltaP", delta_p, &reference.delta_p, tolerances.delta_p),
    ];
    let mut report = GateReport::from_metrics(ID, metrics);
    if tolerances.overridden {
        report.note_override(&format!(
            "allowances cDmax {}, cLmax {}, St {}, DeltaP {} of the interval midpoint; reason: {}",
            tolerances.cd_max,
            tolerances.cl_max,
            tolerances.st,
            tolerances.delta_p,
            tolerances.justification
        ));
    }
    report
}
