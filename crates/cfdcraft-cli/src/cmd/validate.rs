use super::Error;
use cfdcraft_bench_ghia::{GhiaTable, Profile, render_table};
use cfdcraft_model::ResolvedStudy;
use std::path::Path;

pub fn profile_from_raw(
    raw: &str,
    length: f64,
    lid: f64,
    component: usize,
) -> Result<Profile, Error> {
    let mut coords = Vec::new();
    let mut values = Vec::new();
    for line in raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
    {
        let nums: Vec<f64> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
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
pub fn compare_results(results: &Path, r: &ResolvedStudy, table: &GhiaTable) -> Result<i32, Error> {
    let root = results.join("postProcessing/centreline");
    let dirs: Vec<_> = std::fs::read_dir(&root)?.collect::<Result<_, _>>()?;
    let final_dir = dirs
        .iter()
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .parse::<f64>()
                .is_ok_and(|t| (t - r.solver.end_time).abs() < 1e-6)
        })
        .ok_or("missing final-time centreline output")?
        .path();
    let u = profile_from_raw(
        &std::fs::read_to_string(final_dir.join("vertical_U.xy"))?,
        r.domain.length_y,
        r.lid_velocity,
        1,
    )?;
    let v = profile_from_raw(
        &std::fs::read_to_string(final_dir.join("horizontal_U.xy"))?,
        r.domain.length_x,
        r.lid_velocity,
        2,
    )?;
    let u = table.compare_u(&u)?;
    let v = table.compare_v(&v)?;
    println!("{}", render_table(&u, &v));
    Ok(if u.passed && v.passed { 0 } else { 1 })
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
