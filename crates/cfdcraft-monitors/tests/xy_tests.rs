//! Sampled lines: the plain numeric rows, and the mean over the points at each write time.

mod support;

use cfdcraft_monitors::{FieldKind, LineFile, MAX_ROWS, MonitorError, line_mean_series, parse_xy};
use support::{close, xy_text};

fn velocity_line(ux: [f64; 5], uy: [f64; 5]) -> LineFile {
    let rows: Vec<Vec<f64>> = (0..5)
        .map(|i| vec![0.1 * i as f64, 0.5, 0.005, ux[i], uy[i], 0.0])
        .collect();
    parse_xy(&xy_text(&rows)).unwrap()
}

#[test]
fn an_xy_file_reads_numeric_rows() {
    let rows = vec![
        vec![0.0, 0.5, 0.005, 1.0],
        vec![0.1, 0.5, 0.005, 2.0],
        vec![0.2, 0.5, 0.005, 3.0],
    ];
    let file = parse_xy(&xy_text(&rows)).unwrap();
    assert_eq!(file.columns, 4);
    assert_eq!(file.rows.len(), 3);
    close(file.rows[2][3], 3.0);

    let with_noise = parse_xy("# a comment\n\n0 0 0 1\n  0.1\t0\t0\t2  \n").unwrap();
    assert_eq!(with_noise.columns, 4);
    assert_eq!(with_noise.rows.len(), 2);

    assert!(matches!(
        parse_xy("0 0 0 1\n0 0 0\n").unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
    assert!(matches!(
        parse_xy("0 0 0 abc\n").unwrap_err(),
        MonitorError::Malformed { line: 1, .. }
    ));
    assert_eq!(
        parse_xy("0 0 0 1\n0 0 0 nan\n").unwrap_err(),
        MonitorError::NonFinite { line: 2 }
    );
    let nothing = parse_xy("").unwrap();
    assert_eq!((nothing.columns, nothing.rows.len()), (0, 0));
}

#[test]
fn an_xy_file_over_the_limits_is_refused() {
    let mut text = String::new();
    for i in 0..=MAX_ROWS {
        text.push_str(&format!("{i} 1\n"));
    }
    assert_eq!(
        parse_xy(&text).unwrap_err(),
        MonitorError::TooManyRows { max: MAX_ROWS }
    );
    let huge = "#".repeat(8 * 1024 * 1024 + 1);
    assert!(matches!(
        parse_xy(&huge).unwrap_err(),
        MonitorError::TooLarge { .. }
    ));
}

#[test]
fn line_mean_series_averages_each_component_per_write_time() {
    let files = vec![
        (
            100.0,
            velocity_line([1.0, 2.0, 3.0, 4.0, 5.0], [0.0, 0.0, 0.0, 0.0, 10.0]),
        ),
        (
            200.0,
            velocity_line([2.0, 2.0, 2.0, 2.0, 2.0], [1.0, 2.0, 3.0, 4.0, 5.0]),
        ),
        (
            300.0,
            velocity_line([0.0, 0.0, 0.0, 0.0, 10.0], [-1.0, -1.0, -1.0, -1.0, -1.0]),
        ),
    ];
    let series = line_mean_series(&files, "line_1", FieldKind::Velocity);
    assert_eq!(series.len(), 2);
    assert_eq!(series[0].name, "line_1.mean_Ux");
    assert_eq!(series[1].name, "line_1.mean_Uy");
    assert_eq!(series[0].times, vec![100.0, 200.0, 300.0]);
    for (got, want) in series[0].values.iter().zip([3.0, 2.0, 2.0]) {
        close(*got, want);
    }
    for (got, want) in series[1].values.iter().zip([2.0, 3.0, -1.0]) {
        close(*got, want);
    }

    let pressure_rows: Vec<Vec<f64>> = (0..4)
        .map(|i| vec![0.1 * i as f64, 0.5, 0.005, 10.0 * (i + 1) as f64])
        .collect();
    let pressure = parse_xy(&xy_text(&pressure_rows)).unwrap();
    let series = line_mean_series(&[(50.0, pressure)], "line_1", FieldKind::Pressure);
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].name, "line_1.mean_p");
    assert_eq!(series[0].times, vec![50.0]);
    close(series[0].values[0], 25.0);
}

#[test]
fn a_line_file_with_no_points_is_skipped_not_averaged() {
    let empty = parse_xy("# nothing sampled\n").unwrap();
    let full = velocity_line([1.0; 5], [2.0; 5]);
    let series = line_mean_series(
        &[(10.0, empty), (20.0, full)],
        "line_1",
        FieldKind::Velocity,
    );
    assert_eq!(series[0].times, vec![20.0]);
    close(series[0].values[0], 1.0);
    close(series[1].values[0], 2.0);

    // A file too narrow for the field adds no sample either.
    let narrow = parse_xy("0 0 0 1\n").unwrap();
    let series = line_mean_series(&[(10.0, narrow)], "line_1", FieldKind::Velocity);
    assert!(series.iter().all(|s| s.times.is_empty()));
}
