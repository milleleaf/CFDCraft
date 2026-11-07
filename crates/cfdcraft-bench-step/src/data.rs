//! The reference numbers of the turbulent step, embedded in the crate and checked on load.

use cfdcraft_gate::bound_override;
use serde::Deserialize;
use thiserror::Error;

const REFERENCE: &str = include_str!("../data/ds_reference.toml");

/// Why embedded or supplied reference data is refused.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum LoadError {
    #[error("invalid benchmark data {what}: {reason}")]
    Corrupt { what: String, reason: String },
}

pub(crate) fn corrupt(what: &str, reason: impl Into<String>) -> LoadError {
    LoadError::Corrupt {
        what: what.to_owned(),
        reason: reason.into(),
    }
}

/// The limits of the turbulent step comparison.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerances {
    /// The largest distance of the reattachment length from the published one, in step heights.
    pub x_over_h: f64,
    /// The largest deviation of a velocity profile, in reference velocities.
    pub profile_u_max_abs: f64,
    /// The height of the floor line above the floor, in step heights.
    pub floor_offset_over_h: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsSource {
    pub paper: String,
    pub doi: String,
    pub page_url: String,
    pub data_url: String,
    pub curators: String,
    pub terms: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsReattachment {
    pub x_over_h: f64,
    pub uncertainty: f64,
    pub method: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsGeometry {
    pub step_height_m: f64,
    pub re_h: f64,
    pub expansion_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsTolerance {
    pub x_over_h: f64,
    pub profile_u_max_abs: f64,
    pub floor_offset_over_h: f64,
    pub units: String,
    pub justification: String,
}

/// The published reattachment length, the reference conditions and the tolerances.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsReference {
    pub source: DsSource,
    pub reattachment: DsReattachment,
    pub geometry: DsGeometry,
    pub tolerance: DsTolerance,
}

fn positive(what: &str, name: &str, value: f64) -> Result<(), LoadError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(corrupt(what, format!("{name} must be a number above zero")))
    }
}

impl DsReference {
    /// The reference shipped in the crate.
    pub fn load() -> Result<Self, LoadError> {
        Self::parse(REFERENCE)
    }

    pub fn parse(text: &str) -> Result<Self, LoadError> {
        const WHAT: &str = "ds_reference.toml";
        let r: Self = toml::from_str(text).map_err(|e| corrupt(WHAT, e.to_string()))?;
        for (name, value) in [
            ("reattachment x_over_h", r.reattachment.x_over_h),
            ("reattachment uncertainty", r.reattachment.uncertainty),
            ("step_height_m", r.geometry.step_height_m),
            ("re_h", r.geometry.re_h),
            ("expansion_ratio", r.geometry.expansion_ratio),
            ("tolerance x_over_h", r.tolerance.x_over_h),
            ("tolerance profile_u_max_abs", r.tolerance.profile_u_max_abs),
            (
                "tolerance floor_offset_over_h",
                r.tolerance.floor_offset_over_h,
            ),
        ] {
            positive(WHAT, name, value)?;
        }
        if r.tolerance.units != "step_heights" {
            return Err(corrupt(WHAT, "tolerance units must be step_heights"));
        }
        for (name, text) in [
            ("tolerance justification", &r.tolerance.justification),
            ("source doi", &r.source.doi),
            ("source page_url", &r.source.page_url),
            ("source terms", &r.source.terms),
        ] {
            if text.trim().is_empty() {
                return Err(corrupt(WHAT, format!("{name} must not be empty")));
            }
        }
        Ok(r)
    }

    pub fn tolerances(&self) -> Tolerances {
        Tolerances {
            x_over_h: self.tolerance.x_over_h,
            profile_u_max_abs: self.tolerance.profile_u_max_abs,
            floor_offset_over_h: self.tolerance.floor_offset_over_h,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    tolerance: OverrideTable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverrideTable {
    x_over_h: Option<f64>,
    profile_u_max_abs: Option<f64>,
    justification: Option<String>,
}

impl Tolerances {
    /// The tolerances shipped in the crate.
    pub fn default_embedded() -> Result<Self, LoadError> {
        Ok(DsReference::load()?.tolerances())
    }

    /// The shipped tolerances with the values a user file replaces. The file has a `[tolerance]`
    /// table with `x_over_h` and `profile_u_max_abs`, each optional, and a `justification` that
    /// must not be empty. A value must be above zero and at most three times the shipped one.
    pub fn from_toml(text: &str, default: Self) -> Result<Self, LoadError> {
        const WHAT: &str = "tolerance override";
        let file: Override = toml::from_str(text).map_err(|e| corrupt(WHAT, e.to_string()))?;
        let table = file.tolerance;
        if table
            .justification
            .as_deref()
            .is_none_or(|j| j.trim().is_empty())
        {
            return Err(corrupt(WHAT, "a justification is required"));
        }
        let mut out = default;
        if let Some(v) = table.x_over_h {
            out.x_over_h = bound_override("x_over_h", v, default.x_over_h, 3.0)
                .map_err(|e| corrupt(WHAT, e))?;
        }
        if let Some(v) = table.profile_u_max_abs {
            out.profile_u_max_abs =
                bound_override("profile_u_max_abs", v, default.profile_u_max_abs, 3.0)
                    .map_err(|e| corrupt(WHAT, e))?;
        }
        Ok(out)
    }
}

const PROFILES: &str = include_str!("../data/ds_profiles.csv");

/// The stations of the published profiles, in step heights from the step.
pub const STATIONS: [f64; 4] = [1.0, 4.0, 6.0, 10.0];

/// One published profile: the mean streamwise velocity over the height above the downstream
/// floor, with the height in step heights and the velocity in reference velocities.
#[derive(Debug, Clone, PartialEq)]
pub struct Station {
    pub x_over_h: f64,
    pub y_over_h: Vec<f64>,
    pub u_over_uref: Vec<f64>,
}

/// The published profiles at the four stations.
#[derive(Debug, Clone, PartialEq)]
pub struct DsProfiles {
    pub stations: Vec<Station>,
}

impl DsProfiles {
    /// The profiles shipped in the crate.
    pub fn load() -> Result<Self, LoadError> {
        Self::parse(PROFILES)
    }

    /// Read a table with the header `x_over_h,y_over_h,u_over_uref`. Every value must be finite,
    /// the rows of a station must follow each other with the height increasing, the velocity must
    /// lie between -0.5 and 1.2, and each of the four stations needs at least 10 rows.
    pub fn parse(text: &str) -> Result<Self, LoadError> {
        const WHAT: &str = "ds_profiles.csv";
        let mut rows = text.lines();
        if rows.next().map(str::trim) != Some("x_over_h,y_over_h,u_over_uref") {
            return Err(corrupt(
                WHAT,
                "the header must be x_over_h,y_over_h,u_over_uref",
            ));
        }
        let mut stations: Vec<Station> = Vec::new();
        for (at, raw) in rows.enumerate() {
            let line = at + 2;
            if raw.trim().is_empty() {
                continue;
            }
            let values = raw
                .split(',')
                .map(|w| w.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
                .collect::<Option<Vec<f64>>>()
                .ok_or_else(|| {
                    corrupt(
                        WHAT,
                        format!("line {line} holds a value that is not a finite number"),
                    )
                })?;
            let &[x, y, u] = values.as_slice() else {
                return Err(corrupt(WHAT, format!("line {line} must hold three values")));
            };
            if !(-0.5..=1.2).contains(&u) {
                return Err(corrupt(
                    WHAT,
                    format!("line {line}: the velocity is out of range"),
                ));
            }
            let same_station = stations.last().is_some_and(|s| s.x_over_h == x);
            if !same_station {
                if stations.iter().any(|s| s.x_over_h == x) {
                    return Err(corrupt(
                        WHAT,
                        format!("line {line}: the rows of a station must be together"),
                    ));
                }
                stations.push(Station {
                    x_over_h: x,
                    y_over_h: Vec::new(),
                    u_over_uref: Vec::new(),
                });
            }
            if let Some(station) = stations.last_mut() {
                if station.y_over_h.last().is_some_and(|&last| last >= y) {
                    return Err(corrupt(
                        WHAT,
                        format!("line {line}: the height must increase"),
                    ));
                }
                station.y_over_h.push(y);
                station.u_over_uref.push(u);
            }
        }
        if !stations.iter().map(|s| s.x_over_h).eq(STATIONS) {
            return Err(corrupt(
                WHAT,
                "the stations must be 1, 4, 6 and 10 step heights",
            ));
        }
        if stations.iter().any(|s| s.y_over_h.len() < 10) {
            return Err(corrupt(WHAT, "every station needs at least 10 rows"));
        }
        Ok(Self { stations })
    }

    /// The profile at a station, in step heights.
    pub fn station(&self, x_over_h: f64) -> Option<&Station> {
        self.stations.iter().find(|s| s.x_over_h == x_over_h)
    }
}

const ARMALY: &str = include_str!("../data/armaly.toml");
const ARMALY_CURVE: &str = include_str!("../data/armaly_reattachment.csv");

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmalySource {
    pub paper: String,
    pub doi: String,
    pub figure: String,
    pub page: String,
    pub cross_check: String,
    pub method: String,
    pub terms: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmalyGeometry {
    pub step_height_mm: f64,
    pub inlet_height_mm: f64,
    pub outlet_height_mm: f64,
    pub expansion_ratio: f64,
    pub aspect_ratio: f64,
    pub reynolds_definition: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmalyTolerance {
    /// The reading uncertainty of the digitised lengths, in step heights.
    pub reading_uncertainty_s: f64,
    /// The share of the reference length added to the limit for the experiment's
    /// three-dimensionality.
    pub relative_allowance: f64,
    pub justification: String,
}

/// The geometry, the method of the readings and the tolerances of the laminar step.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmalyReference {
    pub source: ArmalySource,
    pub geometry: ArmalyGeometry,
    pub tolerance: ArmalyTolerance,
}

impl ArmalyReference {
    /// The reference shipped in the crate.
    pub fn load() -> Result<Self, LoadError> {
        Self::parse(ARMALY)
    }

    pub fn parse(text: &str) -> Result<Self, LoadError> {
        const WHAT: &str = "armaly.toml";
        let r: Self = toml::from_str(text).map_err(|e| corrupt(WHAT, e.to_string()))?;
        for (name, value) in [
            ("step_height_mm", r.geometry.step_height_mm),
            ("inlet_height_mm", r.geometry.inlet_height_mm),
            ("outlet_height_mm", r.geometry.outlet_height_mm),
            ("expansion_ratio", r.geometry.expansion_ratio),
            ("aspect_ratio", r.geometry.aspect_ratio),
            ("reading_uncertainty_s", r.tolerance.reading_uncertainty_s),
            ("relative_allowance", r.tolerance.relative_allowance),
        ] {
            positive(WHAT, name, value)?;
        }
        for (name, value) in [
            ("source doi", &r.source.doi),
            ("source figure", &r.source.figure),
            ("source page", &r.source.page),
            ("source method", &r.source.method),
            ("source terms", &r.source.terms),
            ("reynolds_definition", &r.geometry.reynolds_definition),
            ("tolerance justification", &r.tolerance.justification),
        ] {
            if value.trim().is_empty() {
                return Err(corrupt(WHAT, format!("{name} must not be empty")));
            }
        }
        Ok(r)
    }
}

/// The digitised reattachment length of the laminar step against the Reynolds number.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmalyCurve {
    pub re: Vec<f64>,
    pub x1_over_s: Vec<f64>,
}

impl ArmalyCurve {
    /// The curve shipped in the crate.
    pub fn load() -> Result<Self, LoadError> {
        Self::parse(ARMALY_CURVE)
    }

    /// Read a table with the header `re,x1_over_s`: at least 7 rows from Re 100 to Re 400 in
    /// steps of at most 50, every length between 2 and 15 and not falling with the Reynolds
    /// number.
    pub fn parse(text: &str) -> Result<Self, LoadError> {
        const WHAT: &str = "armaly_reattachment.csv";
        let mut rows = text.lines();
        if rows.next().map(str::trim) != Some("re,x1_over_s") {
            return Err(corrupt(WHAT, "the header must be re,x1_over_s"));
        }
        let mut curve = Self {
            re: Vec::new(),
            x1_over_s: Vec::new(),
        };
        for (at, raw) in rows.enumerate() {
            let line = at + 2;
            if raw.trim().is_empty() {
                continue;
            }
            let values = raw
                .split(',')
                .map(|w| w.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
                .collect::<Option<Vec<f64>>>()
                .ok_or_else(|| {
                    corrupt(
                        WHAT,
                        format!("line {line} holds a value that is not a finite number"),
                    )
                })?;
            let &[re, x1] = values.as_slice() else {
                return Err(corrupt(WHAT, format!("line {line} must hold two values")));
            };
            if !(2.0..=15.0).contains(&x1) {
                return Err(corrupt(
                    WHAT,
                    format!("line {line}: the length must lie between 2 and 15"),
                ));
            }
            if let (Some(&last_re), Some(&last_x1)) = (curve.re.last(), curve.x1_over_s.last()) {
                if re <= last_re || re - last_re > 50.0 {
                    return Err(corrupt(
                        WHAT,
                        format!("line {line}: the Reynolds number must rise by at most 50"),
                    ));
                }
                if x1 < last_x1 {
                    return Err(corrupt(
                        WHAT,
                        format!("line {line}: the length must not fall"),
                    ));
                }
            }
            curve.re.push(re);
            curve.x1_over_s.push(x1);
        }
        if curve.re.len() < 7 || curve.re.first() != Some(&100.0) || curve.re.last() != Some(&400.0)
        {
            return Err(corrupt(
                WHAT,
                "at least 7 rows from Re 100 to Re 400 are needed",
            ));
        }
        Ok(curve)
    }

    /// The length in step heights at a Reynolds number, linear between the rows; `None` outside
    /// the table (by more than a rounding error) or when the number is not finite.
    pub fn reference_at(&self, re: f64) -> Option<f64> {
        let (&first, &last) = (self.re.first()?, self.re.last()?);
        // A Reynolds number computed from round inputs can miss an end of the table by rounding.
        let slack = 1e-9 * last.abs();
        if !(re.is_finite() && (first - slack..=last + slack).contains(&re)) {
            return None;
        }
        let re = re.max(first).min(last);
        let hi = self
            .re
            .partition_point(|&r| r <= re)
            .min(self.re.len().saturating_sub(1));
        let lo = hi.saturating_sub(1);
        let (re0, re1) = (self.re[lo], self.re[hi]);
        let (x0, x1) = (self.x1_over_s[lo], self.x1_over_s[hi]);
        if re1 == re0 {
            return Some(x0);
        }
        Some(x0 + (re - re0) / (re1 - re0) * (x1 - x0))
    }
}
