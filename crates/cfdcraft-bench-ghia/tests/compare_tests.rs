use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use cfdcraft_bench_ghia::{GhiaTable, InterpError, LoadError, render_table};

const CSV: &str = include_str!("../data/ghia_re100.csv");
const TOML: &str = include_str!("../data/ghia_re100.toml");

fn test_dir() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ghia-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn vendored_table_has_all_stations_landmarks_and_provenance() {
    let table = GhiaTable::load().unwrap();
    assert_eq!(table.u.coords.len(), 17);
    assert_eq!(table.v.coords.len(), 17);
    let midpoint = |p: &cfdcraft_bench_ghia::Profile| {
        p.values[p.coords.iter().position(|&x| x == 0.5).unwrap()]
    };
    assert_eq!(midpoint(&table.u), -0.20581);
    assert_eq!(midpoint(&table.v), 0.05454);
    for text in [
        "10.1016/0021-9991(82)90058-4",
        "e5354b7e5eb8567ac812c204338dba2cf0ef540c",
        "triscale-innov/LidJul.jl",
        "MIT",
        "Copyright (c) 2020: TriScale innov.",
        "THE SOFTWARE IS PROVIDED",
    ] {
        assert!(CSV.contains(text), "missing provenance: {text}");
    }
    assert_eq!(table.tolerances.u_max_abs, 0.02);
    assert_eq!(table.tolerances.v_max_abs, 0.02);
}

#[test]
fn exact_reference_profiles_pass_with_zero_deviation() {
    let table = GhiaTable::load().unwrap();
    for comparison in [
        table.compare_u(&table.u).unwrap(),
        table.compare_v(&table.v).unwrap(),
    ] {
        assert!(comparison.passed);
        assert_eq!(comparison.max_abs_dev, 0.0);
        assert_eq!(comparison.rows.len(), 17);
    }
}

#[test]
fn over_tolerance_profiles_fail_and_report_deviation() {
    let table = GhiaTable::load().unwrap();
    let mut profile = table.u.clone();
    profile.values.iter_mut().for_each(|v| *v += 0.03);
    let comparison = table.compare_u(&profile).unwrap();
    assert!(!comparison.passed);
    assert!((comparison.max_abs_dev - 0.03).abs() < 1e-12);
    let text = render_table(&comparison, &table.compare_v(&table.v).unwrap());
    assert!(text.contains("Ghia u: FAIL"));
    assert!(text.contains("Ghia v: PASS"));
    assert_eq!(
        text.lines()
            .filter(|line| line.contains(" | ") && !line.starts_with("Ghia"))
            .count(),
        34
    );
}

#[test]
fn exact_tolerance_boundary_passes() {
    let mut table = GhiaTable::load().unwrap();
    let mut profile = table.u.clone();
    profile.values[0] = 0.01;
    table.tolerances.u_max_abs = 0.01;
    assert!(table.compare_u(&profile).unwrap().passed);
}

#[test]
fn runtime_tolerance_file_changes_verdict() {
    let dir = test_dir();
    let csv = dir.join("reference.csv");
    let tolerance = dir.join("tolerance.toml");
    std::fs::write(&csv, CSV).unwrap();
    std::fs::write(&tolerance, TOML).unwrap();
    let table = GhiaTable::load_from(&csv, &tolerance).unwrap();
    let mut profile = table.u.clone();
    profile.values[8] += 1e-4;
    assert!(table.compare_u(&profile).unwrap().passed);
    std::fs::write(
        &tolerance,
        TOML.replace("u_max_abs = 0.02", "u_max_abs = 1e-9"),
    )
    .unwrap();
    let strict = GhiaTable::load_from(&csv, &tolerance).unwrap();
    assert!(!strict.compare_u(&profile).unwrap().passed);
    assert!(
        !table
            .with_tolerance_file(&tolerance)
            .unwrap()
            .compare_u(&profile)
            .unwrap()
            .passed
    );
}

#[test]
fn missing_data_returns_path_in_error() {
    let missing = test_dir().join("missing-reference.csv");
    let error = GhiaTable::load_from(&missing, &missing).unwrap_err();
    assert!(matches!(error, LoadError::DataMissing { .. }));
    assert!(error.to_string().contains("missing-reference.csv"));
}

#[test]
fn corrupt_csv_headers_counts_bom_values_and_duplicates_fail() {
    for csv in [
        String::new(),
        format!("\u{feff}{CSV}"),
        CSV.replace("component,coordinate,velocity\n", "bad-header\n"),
        CSV.replace("u,1.0000,1.00000\n", ""),
        CSV.replace("v,0.5000,0.05454", "v,0.5000,NaN"),
        CSV.replace("v,0.5000,0.05454", "v,0.2344,0.05454"),
        CSV.replace("u,1.0000,1.00000", "w,1.0000,1.00000"),
        CSV.replace("u,1.0000,1.00000", "u,2.0000,1.00000"),
        CSV.replace("u,1.0000,1.00000", "u,1.0000,1.00000,extra"),
    ] {
        assert!(matches!(
            GhiaTable::from_text(&csv, TOML),
            Err(LoadError::DataCorrupt { .. })
        ));
    }
}

#[test]
fn corrupt_tolerance_data_is_rejected() {
    for toml in [
        String::new(),
        TOML.replace("u_max_abs = 0.02", "u_max_abs = nan"),
        TOML.replace("u_max_abs = 0.02", "u_max_abs = -0.01"),
        TOML.replace("u_max_abs = 0.02", "u_max_abs = 0"),
        TOML.replace("u_max_abs = 0.02", "u_max_abs = 0.051"),
        TOML.replace("units = \"lid_velocities\"", "units = \"m/s\""),
        TOML.lines()
            .filter(|line| !line.starts_with("justification ="))
            .collect::<Vec<_>>()
            .join("\n"),
    ] {
        assert!(matches!(
            GhiaTable::from_text(CSV, &toml),
            Err(LoadError::DataCorrupt { .. })
        ));
    }
}

#[test]
fn reference_input_can_be_sorted_defensively() {
    let mut lines: Vec<_> = CSV
        .lines()
        .filter(|line| line.starts_with("u,") || line.starts_with("v,"))
        .collect();
    lines.reverse();
    let csv = format!("component,coordinate,velocity\n{}\n", lines.join("\n"));
    let table = GhiaTable::from_text(&csv, TOML).unwrap();
    assert_eq!(table.u.coords.first(), Some(&0.0));
    assert_eq!(table.u.coords.last(), Some(&1.0));
}

#[test]
fn missing_wall_samples_and_nonfinite_velocities_cannot_pass_gate() {
    let table = GhiaTable::load().unwrap();
    let mut p = table.u.clone();
    p.coords.remove(0);
    p.values.remove(0);
    assert!(matches!(
        table.compare_u(&p),
        Err(InterpError::MissingWalls)
    ));
    let mut p = table.v.clone();
    p.values[5] = f64::NAN;
    assert!(matches!(table.compare_v(&p), Err(InterpError::NonFinite)));
}
