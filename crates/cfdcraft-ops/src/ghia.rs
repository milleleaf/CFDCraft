//! The reference comparison for the lid-driven cavity, as data: every station with the computed
//! value, the reference value and the deviation, plus the tolerance and the verdict per component.

use crate::benchmarks::DirFiles;
use cfdcraft_bench_ghia::{GhiaTable, Profile};
use cfdcraft_gate::ResultFiles;
pub use cfdcraft_gate::{GateMetric, GateReport, GateRow};
use cfdcraft_model::ResolvedStudy;
use std::path::Path;

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

/// The two sampled centrelines of a finished run, in lid-velocity units on the unit interval:
/// the vertical one (streamwise velocity) and the horizontal one (cross velocity).
pub(crate) fn centrelines(
    files: &dyn ResultFiles,
    r: &ResolvedStudy,
) -> Result<(Profile, Profile), String> {
    let lid = r
        .lid_velocity()
        .ok_or("the comparison needs a lid-driven cavity")?;
    let root = "postProcessing/centreline";
    let times = files.list(root);
    if times.is_empty() {
        return Err("missing sampled centreline output".into());
    }
    let final_dir = times
        .iter()
        .find(|name| {
            name.parse::<f64>()
                .is_ok_and(|t| (t - r.solver.end_time).abs() < 1e-6)
        })
        .ok_or("missing final-time centreline output")?;
    let read = |name: &str| {
        files
            .read(&format!("{root}/{final_dir}/{name}"))
            .ok_or_else(|| format!("missing sampled file {name}"))
    };
    let u = profile_from_raw(&read("vertical_U.xy")?, r.domain.length_y, lid, 1)?;
    let v = profile_from_raw(&read("horizontal_U.xy")?, r.domain.length_x, lid, 2)?;
    Ok((u, v))
}

fn compare(
    files: &dyn ResultFiles,
    r: &ResolvedStudy,
    table: &GhiaTable,
) -> Result<GateReport, String> {
    let (u, v) = centrelines(files, r)?;
    let u = table.compare_u(&u).map_err(|e| e.to_string())?;
    let v = table.compare_v(&v).map_err(|e| e.to_string())?;
    Ok(GateReport::from_metrics(
        "ghia",
        vec![
            GateMetric::from_comparison("u", &u),
            GateMetric::from_comparison("v", &v),
        ],
    ))
}

/// Compare the sampled centrelines of a finished run, read through `files`, with the reference.
/// Missing or corrupt output is a failed gate with the reason, not an error.
pub(crate) fn compare_files(
    files: &dyn ResultFiles,
    r: &ResolvedStudy,
    table: &GhiaTable,
) -> GateReport {
    compare(files, r, table).unwrap_or_else(|reason| {
        GateReport::failed_named(
            "ghia",
            format!("invalid or incomplete sampled output: {reason}"),
        )
    })
}

/// Compare the sampled centrelines of a finished run with the reference. Missing or corrupt
/// output is a failed gate with the reason, not an error.
pub fn compare_results(results: &Path, r: &ResolvedStudy, table: &GhiaTable) -> GateReport {
    let files = DirFiles {
        root: results.to_path_buf(),
    };
    compare_files(&files, r, table)
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
