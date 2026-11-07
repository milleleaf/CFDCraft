//! The reference comparison for the lid-driven cavity, as data: every station with the computed
//! value, the reference value and the deviation, plus the tolerance and the verdict per component.

use cfdcraft_bench_ghia::{Comparison, GhiaTable, Profile, Row, render_table};
use cfdcraft_model::ResolvedStudy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GateRow {
    pub station: f64,
    pub computed: f64,
    pub reference: f64,
    pub abs_dev: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GateMetric {
    /// `u` for the vertical centreline, `v` for the horizontal one.
    pub name: String,
    pub max_abs_dev: f64,
    pub tolerance: f64,
    pub passed: bool,
    pub rows: Vec<GateRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GateReport {
    /// Always `ghia`.
    pub name: String,
    pub passed: bool,
    pub metrics: Vec<GateMetric>,
    /// Why the gate failed without a comparison: missing or corrupt sampled output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl GateMetric {
    fn from_comparison(name: &str, c: &Comparison) -> Self {
        Self {
            name: name.to_owned(),
            max_abs_dev: c.max_abs_dev,
            tolerance: c.tolerance,
            passed: c.passed,
            rows: c
                .rows
                .iter()
                .map(|r| GateRow {
                    station: r.station,
                    computed: r.computed,
                    reference: r.reference,
                    abs_dev: r.abs_dev,
                })
                .collect(),
        }
    }

    /// The reference crate's own value, for rendering.
    pub fn to_comparison(&self) -> Comparison {
        Comparison {
            max_abs_dev: self.max_abs_dev,
            tolerance: self.tolerance,
            passed: self.passed,
            rows: self
                .rows
                .iter()
                .map(|r| Row {
                    station: r.station,
                    computed: r.computed,
                    reference: r.reference,
                    abs_dev: r.abs_dev,
                })
                .collect(),
        }
    }
}

impl GateReport {
    /// A failed gate that never got as far as a comparison.
    pub fn failed(reason: impl Into<String>) -> Self {
        Self {
            name: "ghia".to_owned(),
            passed: false,
            metrics: Vec::new(),
            reason: Some(reason.into()),
        }
    }

    /// The reference crate's table when there is a comparison, else the reason.
    pub fn render_text(&self) -> String {
        if let Some(reason) = &self.reason {
            return format!("Ghia verdict: FAILED\n  {reason}\n");
        }
        let find = |name: &str| self.metrics.iter().find(|m| m.name == name);
        match (find("u"), find("v")) {
            (Some(u), Some(v)) => render_table(&u.to_comparison(), &v.to_comparison()),
            _ => "Ghia verdict: FAILED\n  the comparison is incomplete\n".to_owned(),
        }
    }
}

/// The shipped reference, with the caller's tolerance file when one is given.
pub fn load_table(tolerance: Option<&Path>) -> Result<GhiaTable, String> {
    let table = GhiaTable::load().map_err(|e| e.to_string())?;
    match tolerance {
        Some(path) => table.with_tolerance_file(path).map_err(|e| e.to_string()),
        None => Ok(table),
    }
}

/// One sampled centreline as a profile in lid-velocity units on the unit interval.
pub fn profile_from_raw(
    raw: &str,
    length: f64,
    lid: f64,
    component: usize,
) -> Result<Profile, String> {
    let mut coords = Vec::new();
    let mut values = Vec::new();
    for line in raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
    {
        let nums: Vec<f64> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|e: std::num::ParseFloatError| e.to_string())?;
        if nums.len() != 4 || nums.iter().any(|x| !x.is_finite()) || !(1..=2).contains(&component) {
            return Err("invalid centreline row: expected coordinate Ux Uy Uz".into());
        }
        coords.push(nums[0] / length);
        values.push(nums[component] / lid);
    }
    if coords.len() < 2 {
        return Err("centreline has fewer than two samples".into());
    }
    if coords.windows(2).any(|w| w[0] >= w[1]) {
        return Err("centreline coordinates must increase".into());
    }
    if coords.first().unwrap().abs() > 1e-8 || (coords.last().unwrap() - 1.0).abs() > 1e-8 {
        return Err("centreline must include both physical wall endpoints".into());
    }
    // OpenFOAM's decimal output rounds coordinates; canonicalize only endpoints
    // already proven to lie on the physical walls within output precision.
    coords[0] = 0.0;
    let last = coords.len() - 1;
    coords[last] = 1.0;
    Ok(Profile { coords, values })
}

fn compare(results: &Path, r: &ResolvedStudy, table: &GhiaTable) -> Result<GateReport, String> {
    let lid = r
        .lid_velocity()
        .ok_or("the comparison needs a lid-driven cavity")?;
    let root = results.join("postProcessing/centreline");
    let final_dir = std::fs::read_dir(&root)
        .map_err(|_| "missing sampled centreline output")?
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .parse::<f64>()
                .is_ok_and(|t| (t - r.solver.end_time).abs() < 1e-6)
        })
        .ok_or("missing final-time centreline output")?
        .path();
    let read = |name: &str| {
        std::fs::read_to_string(final_dir.join(name))
            .map_err(|_| format!("missing sampled file {name}"))
    };
    let u = profile_from_raw(&read("vertical_U.xy")?, r.domain.length_y, lid, 1)?;
    let v = profile_from_raw(&read("horizontal_U.xy")?, r.domain.length_x, lid, 2)?;
    let u = table.compare_u(&u).map_err(|e| e.to_string())?;
    let v = table.compare_v(&v).map_err(|e| e.to_string())?;
    Ok(GateReport {
        name: "ghia".to_owned(),
        passed: u.passed && v.passed,
        metrics: vec![
            GateMetric::from_comparison("u", &u),
            GateMetric::from_comparison("v", &v),
        ],
        reason: None,
    })
}

/// Compare the sampled centrelines of a finished run with the reference. Missing or corrupt
/// output is a failed gate with the reason, not an error.
pub fn compare_results(results: &Path, r: &ResolvedStudy, table: &GhiaTable) -> GateReport {
    compare(results, r, table).unwrap_or_else(|reason| {
        GateReport::failed(format!("invalid or incomplete sampled output: {reason}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize_sampled_coordinates_and_components() {
        let p = profile_from_raw(
            "# y U\n0 0 0 0\n0.05 -0.02 0.005 0\n0.1 0.1 0 0\n",
            0.1,
            0.1,
            1,
        )
        .unwrap();
        assert_eq!(p.coords, vec![0.0, 0.5, 1.0]);
        assert!((p.values[1] + 0.2).abs() < 1e-12);
    }
    #[test]
    fn reject_missing_walls_nonfinite_or_corrupt_samples() {
        for s in [
            "",
            "0.01 0 0 0\n0.1 1 0 0",
            "0 0 0 0\n0.1 NaN 0 0",
            "0 0 0 0\n0 1 0 0\n0.1 0 0 0",
            "0 0\n0.1 0",
        ] {
            assert!(profile_from_raw(s, 0.1, 0.1, 1).is_err(), "{s}");
        }
    }
    #[test]
    fn rounded_physical_wall_coordinates_are_canonicalized() {
        let p = profile_from_raw("0 0 0 0\n0.333333333333 0.1 0 0", 1.0 / 3.0, 0.1, 1).unwrap();
        assert_eq!(p.coords, vec![0.0, 1.0]);
    }
}
