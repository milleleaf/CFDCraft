//! The reference comparison as data: every station, per-component tolerance and verdict, and a
//! text rendering identical to the reference crate's own table.

use cfdcraft_backend_esi::Dialect;
use cfdcraft_bench_ghia::{GhiaTable, render_table};
use cfdcraft_ops::{
    GateReport,
    ghia::{compare_results, load_table, profile_from_raw},
    prepare,
};
use std::path::Path;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

/// Sampled centrelines of a 0.1 m cavity at a 0.1 m/s lid, written the way the solver writes
/// them, with every velocity moved by `shift` lid velocities from the reference.
fn write_samples(dir: &Path, shift: f64) {
    let table = GhiaTable::load().unwrap();
    let out = dir.join("postProcessing/centreline/5000");
    std::fs::create_dir_all(&out).unwrap();
    for (file, profile, component) in [
        ("vertical_U.xy", &table.u, 1),
        ("horizontal_U.xy", &table.v, 2),
    ] {
        let mut rows: Vec<_> = profile.coords.iter().zip(&profile.values).collect();
        rows.sort_by(|a, b| a.0.total_cmp(b.0));
        let text: String = rows
            .iter()
            .map(|(c, v)| {
                let mut cols = [0.0; 3];
                cols[component - 1] = (**v + shift) * 0.1;
                format!(
                    "{:.12} {:.12} {:.12} {:.12}\n",
                    **c * 0.1,
                    cols[0],
                    cols[1],
                    cols[2]
                )
            })
            .collect();
        std::fs::write(out.join(file), text).unwrap();
    }
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ops-ghia-tests")
        .join(format!("{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_gate_carries_every_station_and_renders_the_reference_tables_text() {
    let resolved = prepare(CAVITY, Dialect::EsiV2512).unwrap().resolved;
    let table = load_table(None).unwrap();
    let dir = scratch("stations");
    write_samples(&dir, 0.004);
    let gate = compare_results(&dir, &resolved, &table);

    assert_eq!(gate.name, "ghia");
    assert!(gate.reason.is_none());
    let names: Vec<_> = gate.metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["u", "v"]);
    assert_eq!(gate.metrics[0].rows.len(), table.u.coords.len());
    assert_eq!(gate.metrics[1].rows.len(), table.v.coords.len());
    assert_eq!(gate.metrics[0].tolerance, table.tolerances.u_max_abs);
    assert_eq!(gate.metrics[1].tolerance, table.tolerances.v_max_abs);
    for metric in &gate.metrics {
        assert!(metric.rows.iter().all(|r| (r.abs_dev - 0.004).abs() < 1e-9));
        assert!((metric.max_abs_dev - 0.004).abs() < 1e-9);
        assert_eq!(metric.passed, metric.max_abs_dev <= metric.tolerance);
    }
    assert_eq!(gate.passed, gate.metrics.iter().all(|m| m.passed));

    // The same samples through the reference crate directly give the same table, character for character.
    let sample = |file: &str, length: f64, component: usize| {
        let raw =
            std::fs::read_to_string(dir.join("postProcessing/centreline/5000").join(file)).unwrap();
        profile_from_raw(&raw, length, 0.1, component).unwrap()
    };
    let u = table.compare_u(&sample("vertical_U.xy", 0.1, 1)).unwrap();
    let v = table.compare_v(&sample("horizontal_U.xy", 0.1, 2)).unwrap();
    assert_eq!(gate.render_text(), render_table(&u, &v));

    // It survives the JSON a report is written as.
    let again: GateReport = serde_json::from_str(&serde_json::to_string(&gate).unwrap()).unwrap();
    assert_eq!(again.render_text(), gate.render_text());
}

#[test]
fn a_large_deviation_fails_the_gate_and_missing_output_fails_it_with_a_reason() {
    let resolved = prepare(CAVITY, Dialect::EsiV2512).unwrap().resolved;
    let table = load_table(None).unwrap();

    let dir = scratch("off");
    write_samples(&dir, 0.5);
    let gate = compare_results(&dir, &resolved, &table);
    assert!(!gate.passed && gate.reason.is_none() && gate.metrics.len() == 2);

    let empty = scratch("empty");
    let gate = compare_results(&empty, &resolved, &table);
    assert!(!gate.passed && gate.metrics.is_empty());
    assert!(gate.reason.as_deref().is_some_and(|r| !r.is_empty()));
    assert!(gate.render_text().contains("FAILED"));

    let corrupt = scratch("corrupt");
    write_samples(&corrupt, 0.0);
    std::fs::write(
        corrupt.join("postProcessing/centreline/5000/vertical_U.xy"),
        "0 NaN 0 0\n0.1 0 0 0\n",
    )
    .unwrap();
    let gate = compare_results(&corrupt, &resolved, &table);
    assert!(!gate.passed && gate.reason.is_some());
}
