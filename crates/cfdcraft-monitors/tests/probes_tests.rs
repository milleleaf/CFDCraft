//! Probes files: locations, velocity and pressure series, tolerance of layout noise, and refusals.

mod support;

use cfdcraft_monitors::{
    FieldKind, MAX_BYTES, MAX_PROBES, MAX_ROWS, MonitorError, Value, parse_probes, probe_series,
};
use support::{close, probes_text};

fn velocity_rows() -> Vec<(f64, Vec<Vec<f64>>)> {
    vec![
        (0.5, vec![vec![1.0, 2.0, 0.0], vec![3.0, 4.0, 0.0]]),
        (1.0, vec![vec![1.5, 2.5, 0.0], vec![3.5, 4.5, 0.0]]),
        (1.5, vec![vec![1.75, 2.75, 0.0], vec![3.75, 4.75, 0.0]]),
    ]
}

#[test]
fn a_velocity_probes_file_gives_ux_and_uy_series() {
    let text = probes_text(
        &[[0.05, 0.05, 0.005], [0.01, 0.02, 0.005]],
        &velocity_rows(),
    );
    let file = parse_probes(&text).unwrap();
    assert_eq!(file.probes.len(), 2);
    assert_eq!(file.probes[1].location, [0.01, 0.02, 0.005]);
    assert!(!file.probes[0].not_found);
    assert_eq!(file.rows.len(), 3);
    assert_eq!(file.rows[0].values.len(), 2);
    assert_eq!(file.rows[0].values[0], Value::Vector([1.0, 2.0, 0.0]));

    let series = probe_series(&file, "probe_1", 0, FieldKind::Velocity);
    assert_eq!(series.len(), 2);
    assert_eq!(series[0].name, "probe_1.Ux");
    assert_eq!(series[1].name, "probe_1.Uy");
    assert_eq!(series[0].times, vec![0.5, 1.0, 1.5]);
    for (got, want) in series[0].values.iter().zip([1.0, 1.5, 1.75]) {
        close(*got, want);
    }
    for (got, want) in series[1].values.iter().zip([2.0, 2.5, 2.75]) {
        close(*got, want);
    }
    let second = probe_series(&file, "probe_2", 1, FieldKind::Velocity);
    close(second[0].values[2], 3.75);
}

#[test]
fn a_pressure_probes_file_gives_a_p_series() {
    let rows = vec![
        (1.0, vec![vec![0.25]]),
        (2.0, vec![vec![0.5]]),
        (3.0, vec![vec![-0.125]]),
    ];
    let file = parse_probes(&probes_text(&[[0.0, 0.0, 0.0]], &rows)).unwrap();
    let series = probe_series(&file, "p_monitor", 0, FieldKind::Pressure);
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].name, "p_monitor.p");
    assert_eq!(series[0].times, vec![1.0, 2.0, 3.0]);
    close(series[0].values[2], -0.125);
}

#[test]
fn a_probe_outside_the_mesh_is_marked() {
    let text = "# Probe 0 (0.1 0.1 0.005)\n# Probe 1 (9 9 9) # Not Found\n# Time\n0.5 (1 2 0) (0 0 0)\n1.0 (1 2 0) (0 0 0)\n";
    let file = parse_probes(text).unwrap();
    assert!(!file.probes[0].not_found);
    assert!(file.probes[1].not_found);
    let missing = probe_series(&file, "far", 1, FieldKind::Velocity);
    assert_eq!(missing.len(), 2);
    assert!(
        missing
            .iter()
            .all(|s| s.times.is_empty() && s.values.is_empty())
    );
    let found = probe_series(&file, "near", 0, FieldKind::Velocity);
    assert_eq!(found[0].times.len(), 2);
    let unknown = probe_series(&file, "none", 7, FieldKind::Pressure);
    assert!(unknown[0].times.is_empty());
}

#[test]
fn comments_blank_lines_and_extra_spaces_are_tolerated() {
    let text = "\n# a comment\n# Probe 0 (  0.5   0.5 0.005 )\n\n#  Probe          0\n# Time\n\n   0.5\t(  1   2    0 )  \n# a late comment\n1.0   ( 3 4 0)\n\n";
    let file = parse_probes(text).unwrap();
    assert_eq!(file.probes.len(), 1);
    assert_eq!(file.probes[0].location, [0.5, 0.5, 0.005]);
    assert_eq!(file.rows.len(), 2);
    assert_eq!(file.rows[1].values[0], Value::Vector([3.0, 4.0, 0.0]));
}

#[test]
fn a_non_finite_number_is_refused() {
    for bad in ["nan", "inf", "-inf"] {
        let text = format!("# Probe 0 (0 0 0)\n# Time\n0.5 1.0\n1.0 {bad}\n");
        assert_eq!(
            parse_probes(&text).unwrap_err(),
            MonitorError::NonFinite { line: 4 },
            "{bad}"
        );
    }
    let in_vector = "# Probe 0 (0 0 0)\n0.5 (1 nan 0)\n";
    assert_eq!(
        parse_probes(in_vector).unwrap_err(),
        MonitorError::NonFinite { line: 2 }
    );
    let time = "# Probe 0 (0 0 0)\ninf 1.0\n";
    assert_eq!(
        parse_probes(time).unwrap_err(),
        MonitorError::NonFinite { line: 2 }
    );
    let words = "# Probe 0 (0 0 0)\nsoon 1.0\n";
    assert!(matches!(
        parse_probes(words).unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
}

#[test]
fn a_row_with_the_wrong_number_of_values_is_refused() {
    let short = "# Probe 0 (0 0 0)\n# Probe 1 (1 1 1)\n0.5 1.0\n";
    assert!(matches!(
        parse_probes(short).unwrap_err(),
        MonitorError::Malformed { line: 3, .. }
    ));
    let cut_vector = "# Probe 0 (0 0 0)\n0.5 (1 2)\n";
    assert!(matches!(
        parse_probes(cut_vector).unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
    let unclosed = "# Probe 0 (0 0 0)\n0.5 (1 2 3\n";
    assert!(matches!(
        parse_probes(unclosed).unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
    let skipped_index = "# Probe 0 (0 0 0)\n# Probe 2 (1 1 1)\n";
    assert!(matches!(
        parse_probes(skipped_index).unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
    let late_header = "# Probe 0 (0 0 0)\n0.5 1.0\n# Probe 1 (1 1 1)\n";
    assert!(matches!(
        parse_probes(late_header).unwrap_err(),
        MonitorError::Malformed { line: 3, .. }
    ));
}

#[test]
fn limits_are_enforced() {
    assert_eq!(MAX_PROBES, 64);
    assert_eq!(MAX_ROWS, 200_000);
    assert_eq!(MAX_BYTES, 8 * 1024 * 1024);

    let many_probes: String = (0..=MAX_PROBES)
        .map(|i| format!("# Probe {i} (0 0 0)\n"))
        .collect();
    assert!(matches!(
        parse_probes(&many_probes).unwrap_err(),
        MonitorError::Malformed { .. }
    ));
    let exactly: String = (0..MAX_PROBES)
        .map(|i| format!("# Probe {i} (0 0 0)\n"))
        .collect();
    assert_eq!(parse_probes(&exactly).unwrap().probes.len(), MAX_PROBES);

    let huge = "#".repeat(MAX_BYTES + 1);
    assert!(matches!(
        parse_probes(&huge).unwrap_err(),
        MonitorError::TooLarge { .. }
    ));

    let rows = |count: usize| -> String {
        let mut text = "# Probe 0 (0 0 0)\n".to_owned();
        for i in 0..count {
            text.push_str(&format!("{i} 1\n"));
        }
        text
    };
    assert_eq!(parse_probes(&rows(MAX_ROWS)).unwrap().rows.len(), MAX_ROWS);
    assert_eq!(
        parse_probes(&rows(MAX_ROWS + 1)).unwrap_err(),
        MonitorError::TooManyRows { max: MAX_ROWS }
    );
}
