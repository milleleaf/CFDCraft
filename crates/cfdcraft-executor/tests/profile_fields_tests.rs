//! The host profile's core cap and disk floor: defaults, bounds, and the old
//! bounds staying where they were.

use cfdcraft_executor::HostProfile;
use std::path::PathBuf;

const EXAMPLE: &str = include_str!("hosts.example.toml");

/// The example profile with some keys replaced or added.
fn text(overrides: &[(&str, &str)]) -> String {
    let mut seen = Vec::new();
    let mut out = String::new();
    for line in EXAMPLE.lines() {
        let key = line.split(" = ").next().unwrap_or("");
        match overrides.iter().find(|(k, _)| *k == key) {
            Some((k, v)) => {
                seen.push(*k);
                out.push_str(&format!("{k} = {v}\n"));
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    for (k, v) in overrides {
        if !seen.contains(k) {
            out.push_str(&format!("{k} = {v}\n"));
        }
    }
    out
}

fn load(name: &str, overrides: &[(&str, &str)]) -> Result<HostProfile, String> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("profile-{}-{name}.toml", std::process::id()));
    std::fs::write(&file, text(overrides)).unwrap();
    let r = HostProfile::load(&file, "box").map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&file);
    r
}

#[test]
fn a_profile_without_the_new_fields_loads_with_one_core_and_twenty_gb() {
    let p = load("defaults", &[]).unwrap();
    assert_eq!(p.core_cap, 1);
    assert_eq!(p.min_free_gb, 20);
}

#[test]
fn the_new_fields_can_be_set_inside_the_bounds() {
    let p = load("set", &[("core_cap", "2"), ("min_free_gb", "50")]).unwrap();
    assert_eq!((p.core_cap, p.min_free_gb), (2, 50));
}

#[test]
fn a_core_cap_above_the_cpus_or_zero_is_refused() {
    assert!(load("above", &[("cpus", "2"), ("core_cap", "3")]).is_err());
    assert!(load("below", &[("cpus", "1"), ("core_cap", "2")]).is_err());
    assert!(load("zero", &[("core_cap", "0")]).is_err());
}

#[test]
fn a_floor_of_zero_or_over_two_thousand_is_refused() {
    assert!(load("floor-zero", &[("min_free_gb", "0")]).is_err());
    assert!(load("floor-high", &[("min_free_gb", "2001")]).is_err());
    assert!(load("floor-edge", &[("min_free_gb", "2000")]).is_ok());
}

#[test]
fn the_bounds_are_the_decided_ones() {
    assert!(load("cpus", &[("cpus", "9"), ("core_cap", "1")]).is_err());
    assert!(load("wall", &[("wall_clock_seconds", "86401")]).is_err());
    assert!(load("conc", &[("concurrency", "2")]).is_err());
    assert!(load("edge", &[("wall_clock_seconds", "86400")]).is_ok());
}

#[test]
fn an_unknown_field_is_still_refused() {
    assert!(load("unknown", &[("core_caps", "1")]).is_err());
}
