//! A sweep, and its expansion into points.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::values::{Values, expand, tidy, value_count};

/// The most points one sweep may expand to.
pub const MAX_SWEEP_POINTS: usize = 200;
/// The most parameters one sweep may have.
pub const MAX_PARAMETERS: usize = 8;

/// The first thing wrong with a sweep.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SweepError {
    /// The sweep id is not a plain name of 1 to 36 lower-case letters, digits or hyphens.
    #[error(
        "the sweep id \"{sweep}\" is not a plain name of 1 to 36 lower-case letters, digits or hyphens"
    )]
    BadId { sweep: String },
    /// The sweep has no parameter.
    #[error("the sweep has no parameter")]
    NoParameters,
    /// The sweep has more parameters than the limit.
    #[error("the sweep has {count} parameters, but at most {max} are allowed")]
    TooManyParameters { count: usize, max: usize },
    /// A parameter name is not a plain name.
    #[error(
        "the parameter name \"{name}\" is not 1 to 24 lower-case letters, digits or underscores starting with a letter"
    )]
    BadName { name: String },
    /// Two parameters share a name.
    #[error("the parameter name \"{name}\" is used more than once")]
    DuplicateName { name: String },
    /// The values of a parameter cannot be used.
    #[error("the values of \"{name}\" cannot be used: {reason}")]
    BadValues { name: String, reason: String },
    /// A parameter has more values than the limit.
    #[error("\"{name}\" has {count} values, but at most {max} are allowed")]
    TooManyValues {
        name: String,
        count: usize,
        max: usize,
    },
    /// The sweep expands to more points than the limit.
    #[error("the sweep has {count} points, but at most {max} are allowed")]
    TooManyPoints { count: usize, max: usize },
    /// A value of a count parameter is not a whole number that is not negative.
    #[error("\"{name}\" is a count, but {value} is not a whole number from 0 up to 2^53")]
    NotACount { name: String, value: f64 },
    /// A setting pointer is not usable, or two parameters aim at one setting or at settings one of
    /// which lies below the other.
    #[error(
        "\"{name}\" aims at the setting \"{setting}\", which is not usable or overlaps a setting of another parameter"
    )]
    BadSetting { name: String, setting: String },
    /// A unit is empty, too long or holds a control character.
    #[error("the unit of \"{name}\" is empty, too long or holds a control character")]
    BadUnit { name: String },
}

impl SweepError {
    /// Fills in the parameter name on an error raised without one.
    pub(crate) fn named(self, who: &str) -> Self {
        match self {
            Self::BadValues { reason, .. } => Self::BadValues {
                name: who.to_owned(),
                reason,
            },
            Self::TooManyValues { count, max, .. } => Self::TooManyValues {
                name: who.to_owned(),
                count,
                max,
            },
            other => other,
        }
    }
}

/// What a parameter changes in the study.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    /// A quantity, written in a study as `{"value": <number>, "unit": <text>}`.
    Quantity { setting: String, unit: String },
    /// A whole number, such as a cell count.
    Count { setting: String },
    /// A plain number, such as a convergence limit.
    Number { setting: String },
    /// A vector setting turned by an angle. The values are angles in degrees, counter-clockwise
    /// about +z; the base vector is the one at angle zero. The two components are set as
    /// quantities in `unit`.
    Direction {
        setting_x: String,
        setting_y: String,
        unit: String,
        base_x: f64,
        base_y: f64,
    },
}

impl Target {
    /// The settings this target writes, in the order they are written.
    fn settings(&self) -> Vec<&str> {
        match self {
            Self::Quantity { setting, .. } | Self::Count { setting } | Self::Number { setting } => {
                vec![setting.as_str()]
            }
            Self::Direction {
                setting_x,
                setting_y,
                ..
            } => vec![setting_x.as_str(), setting_y.as_str()],
        }
    }
}

/// One thing a sweep varies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    pub target: Target,
    pub values: Values,
}

/// A sweep: the scenario it starts from and the parameters it varies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sweep {
    pub id: String,
    pub base: String,
    pub parameters: Vec<Parameter>,
}

/// One setting to change: a pointer into the study and the value to put there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub setting: String,
    pub value: Value,
}

/// One combination of the parameters' values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub index: u32,
    pub id: String,
    pub label: String,
    pub values: Vec<f64>,
    pub assignments: Vec<Assignment>,
}

fn is_sweep_id(id: &str) -> bool {
    let mut chars = id.chars();
    (1..=36).contains(&id.len())
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_parameter_name(name: &str) -> bool {
    name.len() <= 24
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// Whether two setting pointers are one or one lies below the other, so that a change to one would
/// replace or be replaced by a change to the other.
fn overlap(a: &str, b: &str) -> bool {
    let below = |outer: &str, inner: &str| {
        inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with('/'))
    };
    a == b || below(a, b) || below(b, a)
}

fn check_target(p: &Parameter, seen: &mut Vec<String>) -> Result<(), SweepError> {
    let bad_setting = |setting: &str| SweepError::BadSetting {
        name: p.name.clone(),
        setting: setting.to_owned(),
    };
    for setting in p.target.settings() {
        let len = setting.chars().count();
        if !setting.starts_with('/')
            || !(2..=200).contains(&len)
            || has_control(setting)
            || seen.iter().any(|s| overlap(s, setting))
        {
            return Err(bad_setting(setting));
        }
        seen.push(setting.to_owned());
    }
    let unit = match &p.target {
        Target::Quantity { unit, .. } | Target::Direction { unit, .. } => Some(unit),
        Target::Count { .. } | Target::Number { .. } => None,
    };
    if let Some(unit) = unit
        && (unit.is_empty() || unit.chars().count() > 40 || has_control(unit))
    {
        return Err(SweepError::BadUnit {
            name: p.name.clone(),
        });
    }
    if let Target::Direction { base_x, base_y, .. } = &p.target {
        let reason = if !base_x.is_finite() || !base_y.is_finite() {
            Some("the base vector is not made of finite numbers")
        } else if *base_x == 0.0 && *base_y == 0.0 {
            Some("the base vector is zero, so no angle could turn it")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(SweepError::BadValues {
                name: p.name.clone(),
                reason: reason.to_owned(),
            });
        }
    }
    Ok(())
}

/// Checks the sweep as a whole and returns how many points it has. Nothing larger than the limits
/// allow is built.
fn validate(sweep: &Sweep) -> Result<usize, SweepError> {
    if !is_sweep_id(&sweep.id) {
        return Err(SweepError::BadId {
            sweep: sweep.id.clone(),
        });
    }
    if sweep.parameters.is_empty() {
        return Err(SweepError::NoParameters);
    }
    if sweep.parameters.len() > MAX_PARAMETERS {
        return Err(SweepError::TooManyParameters {
            count: sweep.parameters.len(),
            max: MAX_PARAMETERS,
        });
    }
    let mut total: usize = 1;
    let mut settings = Vec::new();
    for (i, p) in sweep.parameters.iter().enumerate() {
        if !is_parameter_name(&p.name) {
            return Err(SweepError::BadName {
                name: p.name.clone(),
            });
        }
        if sweep.parameters[..i].iter().any(|q| q.name == p.name) {
            return Err(SweepError::DuplicateName {
                name: p.name.clone(),
            });
        }
        check_target(p, &mut settings)?;
        let n = value_count(&p.values).map_err(|e| e.named(&p.name))?;
        if matches!(p.target, Target::Count { .. }) {
            // At most MAX_VALUES values, so this is cheap.
            for v in expand(&p.values).map_err(|e| e.named(&p.name))? {
                count_value(&p.name, v)?;
            }
        }
        total = total.saturating_mul(n);
    }
    if total > MAX_SWEEP_POINTS {
        return Err(SweepError::TooManyPoints {
            count: total,
            max: MAX_SWEEP_POINTS,
        });
    }
    Ok(total)
}

/// Checks a sweep without expanding it.
pub fn check_sweep(sweep: &Sweep) -> Result<(), SweepError> {
    validate(sweep).map(|_| ())
}

/// How many points the sweep has. A sweep over the limits is refused before anything is built.
pub fn point_count(sweep: &Sweep) -> Result<usize, SweepError> {
    validate(sweep)
}

fn count_value(name: &str, v: f64) -> Result<Value, SweepError> {
    if v.fract() == 0.0 && (0.0..=9_007_199_254_740_992.0).contains(&v) {
        Ok(Value::from(v as u64))
    } else {
        Err(SweepError::NotACount {
            name: name.to_owned(),
            value: v,
        })
    }
}

/// The cosine and sine of an angle in degrees: exact at the four right angles.
fn cos_sin(angle: f64) -> (f64, f64) {
    let a = angle.rem_euclid(360.0);
    if a == 0.0 || a >= 360.0 {
        (1.0, 0.0)
    } else if a == 90.0 {
        (0.0, 1.0)
    } else if a == 180.0 {
        (-1.0, 0.0)
    } else if a == 270.0 {
        (0.0, -1.0)
    } else {
        let r = a.to_radians();
        (r.cos(), r.sin())
    }
}

fn assignments(p: &Parameter, v: f64) -> Result<Vec<Assignment>, SweepError> {
    let one = |setting: &str, value: Value| {
        vec![Assignment {
            setting: setting.to_owned(),
            value,
        }]
    };
    Ok(match &p.target {
        Target::Quantity { setting, unit } => one(setting, json!({ "value": v, "unit": unit })),
        Target::Count { setting } => one(setting, count_value(&p.name, v)?),
        Target::Number { setting } => one(setting, Value::from(v)),
        Target::Direction {
            setting_x,
            setting_y,
            unit,
            base_x,
            base_y,
        } => {
            let (c, s) = cos_sin(v);
            let x = tidy(base_x * c - base_y * s);
            let y = tidy(base_x * s + base_y * c);
            if !x.is_finite() || !y.is_finite() {
                return Err(SweepError::BadValues {
                    name: p.name.clone(),
                    reason: format!("the vector turned by {v} is too large to hold as a number"),
                });
            }
            let mut both = one(setting_x, json!({ "value": x, "unit": unit }));
            both.extend(one(setting_y, json!({ "value": y, "unit": unit })));
            both
        }
    })
}

/// Expands the sweep into its points: the full factorial of the parameters, the first one varying
/// slowest.
pub fn points(sweep: &Sweep) -> Result<Vec<Point>, SweepError> {
    let total = validate(sweep)?;
    let lists = sweep
        .parameters
        .iter()
        .map(|p| expand(&p.values).map_err(|e| e.named(&p.name)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(total);
    for index in 0..total {
        let mut rest = index;
        let mut values = vec![0.0; lists.len()];
        for (slot, list) in values.iter_mut().zip(&lists).rev() {
            *slot = tidy(list[rest % list.len()]);
            rest /= list.len();
        }
        let mut label = Vec::with_capacity(values.len());
        let mut assigned = Vec::new();
        for (p, v) in sweep.parameters.iter().zip(&values) {
            label.push(format!("{} {}", p.name, v));
            assigned.extend(assignments(p, *v)?);
        }
        out.push(Point {
            index: index as u32,
            id: format!("{}-{:03}", sweep.id, index + 1),
            label: label.join(", "),
            values,
            assignments: assigned,
        });
    }
    Ok(out)
}
