//! Surface field value files: the header, the columns, and refusals.

mod support;

use cfdcraft_monitors::{MAX_ROWS, MonitorError, Value, parse_surface, surface_series};
use support::{close, surface_text};

#[test]
fn a_surface_file_gives_its_header_and_a_series_per_column() {
    let rows = vec![
        (1.0, vec![vec![-0.5]]),
        (2.0, vec![vec![-0.25]]),
        (3.0, vec![vec![-0.125]]),
    ];
    let text = surface_text("patch inlet", 128, 0.0125, &["sum(phi)"], &rows);
    let file = parse_surface(&text).unwrap();
    assert_eq!(file.region, "patch inlet");
    assert_eq!(file.faces, 128);
    close(file.area, 0.0125);
    assert_eq!(file.columns, vec!["sum(phi)".to_owned()]);
    assert_eq!(file.rows.len(), 3);

    let series = surface_series(&file, "balance_inlet", &[("sum(phi)", "flux")]);
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].name, "balance_inlet.flux");
    assert_eq!(series[0].times, vec![1.0, 2.0, 3.0]);
    close(series[0].values[0], -0.5);
    close(series[0].values[2], -0.125);
}

#[test]
fn two_columns_and_a_vector_column_are_read() {
    let rows = vec![
        (1.0, vec![vec![0.5], vec![1.0, 2.0, 0.0], vec![10.0]]),
        (2.0, vec![vec![0.75], vec![1.5, 2.5, 0.0], vec![20.0]]),
    ];
    let text = surface_text(
        "patch outlet",
        64,
        0.5,
        &["areaAverage(p)", "sum(U)", "areaIntegrate(p)"],
        &rows,
    );
    let file = parse_surface(&text).unwrap();
    assert_eq!(file.rows[0].values[1], Value::Vector([1.0, 2.0, 0.0]));

    let series = surface_series(
        &file,
        "balance_outlet",
        &[
            ("areaIntegrate(p)", "pressure_integral"),
            ("areaAverage(p)", "pressure_average"),
            ("sum(U)", "velocity"),
            ("absent(p)", "missing"),
        ],
    );
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "balance_outlet.pressure_integral",
            "balance_outlet.pressure_average"
        ]
    );
    close(series[0].values[1], 20.0);
    close(series[1].values[1], 0.75);
}

#[test]
fn a_missing_or_short_header_is_refused() {
    let no_time = "# Region type : patch inlet\n# Faces : 4\n# Area : 1.0\n";
    assert!(matches!(
        parse_surface(no_time).unwrap_err(),
        MonitorError::Malformed { .. }
    ));
    let no_area = "# Region type : patch inlet\n# Faces : 4\n# Time\tsum(phi)\n1.0\t2.0\n";
    assert!(matches!(
        parse_surface(no_area).unwrap_err(),
        MonitorError::Malformed { .. }
    ));
    let data_first = "# Region type : patch inlet\n1.0\t2.0\n# Time\tsum(phi)\n";
    assert!(matches!(
        parse_surface(data_first).unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
    let bad_faces = "# Region type : patch inlet\n# Faces : many\n";
    assert!(matches!(
        parse_surface(bad_faces).unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
    let bad_area = "# Region type : patch inlet\n# Faces : 4\n# Area : inf\n";
    assert_eq!(
        parse_surface(bad_area).unwrap_err(),
        MonitorError::NonFinite { line: 3 }
    );
}

#[test]
fn non_finite_and_wrong_width_rows_are_refused() {
    let head = "# Region type : patch inlet\n# Faces : 4\n# Area : 1.0\n# Time\tsum(phi)\n";
    assert_eq!(
        parse_surface(&format!("{head}1.0\tnan\n")).unwrap_err(),
        MonitorError::NonFinite { line: 5 }
    );
    assert_eq!(
        parse_surface(&format!("{head}1.0\t1.0\n-inf\t2.0\n")).unwrap_err(),
        MonitorError::NonFinite { line: 6 }
    );
    assert!(matches!(
        parse_surface(&format!("{head}1.0\t1.0\t2.0\n")).unwrap_err(),
        MonitorError::Malformed { line: 5, .. }
    ));
    assert!(matches!(
        parse_surface(&format!("{head}1.0\n")).unwrap_err(),
        MonitorError::Malformed { line: 5, .. }
    ));
    assert!(matches!(
        parse_surface(&format!("{head}1.0\tx\n")).unwrap_err(),
        MonitorError::Malformed { line: 5, .. }
    ));
}

#[test]
fn a_surface_file_over_the_limits_is_refused() {
    let head = "# Region type : patch inlet\n# Faces : 4\n# Area : 1.0\n# Time\tsum(phi)\n";
    let mut text = head.to_owned();
    for i in 0..=MAX_ROWS {
        text.push_str(&format!("{i}\t1\n"));
    }
    assert_eq!(
        parse_surface(&text).unwrap_err(),
        MonitorError::TooManyRows { max: MAX_ROWS }
    );
    let huge = format!("{head}{}", "#".repeat(8 * 1024 * 1024));
    assert!(matches!(
        parse_surface(&huge).unwrap_err(),
        MonitorError::TooLarge { .. }
    ));
}
