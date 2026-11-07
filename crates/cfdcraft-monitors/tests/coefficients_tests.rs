//! Force coefficient files: the directions, the columns, the series and refusals.

use cfdcraft_monitors::{
    MAX_ROWS, MonitorError, Value, coefficient_series, join_series, parse_coefficients,
};

const HEAD: &str = "# Force coefficients\n# dragDir : (1 0 0)\n# liftDir : (0 1 0)\n";

fn file_text() -> String {
    format!(
        "{HEAD}# Time Cm Cd Cl Cl(f) Cl(r)\n\
         0.1 0.01 1.50 0.20 0.10 0.10\n\
         0.2 0.02 1.55 0.25 0.12 0.13\n\
         0.3 0.03 1.60 -0.30 -0.15 -0.15\n"
    )
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} differs from {b}");
}

#[test]
fn a_coefficient_file_is_read() {
    let file = parse_coefficients(&file_text()).unwrap();
    assert_eq!(file.columns, vec!["Cm", "Cd", "Cl", "Cl(f)", "Cl(r)"]);
    assert_eq!(file.lift_dir, Some([0.0, 1.0, 0.0]));
    assert_eq!(file.drag_dir, Some([1.0, 0.0, 0.0]));
    assert_eq!(file.rows.len(), 3);
    assert!(file.rows.iter().all(|r| r.values.len() == 5));
    assert_eq!(file.rows[1].values[2], Value::Scalar(0.25));
    close(file.rows[2].time, 0.3);
}

#[test]
fn the_series_are_named_by_column() {
    let file = parse_coefficients(&file_text()).unwrap();
    let series = coefficient_series(&file, "force_1");
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "force_1.Cm",
            "force_1.Cd",
            "force_1.Cl",
            "force_1.Cl_f",
            "force_1.Cl_r"
        ]
    );
    let cl = &series[2];
    assert_eq!(cl.times, vec![0.1, 0.2, 0.3]);
    assert_eq!(cl.values, vec![0.20, 0.25, -0.30]);
}

#[test]
fn a_missing_lift_direction_is_none() {
    let text = "# Force coefficients\n# dragDir: (1 0 0)\n# Time Cd Cl\n1 1.0 0.0\n";
    let file = parse_coefficients(text).unwrap();
    assert_eq!(file.lift_dir, None);
    assert_eq!(file.drag_dir, Some([1.0, 0.0, 0.0]));

    let two = "# liftDir : (0 1)\n# Time Cd\n1 1.0\n";
    assert!(matches!(
        parse_coefficients(two).unwrap_err(),
        MonitorError::Malformed { line: 1, .. }
    ));
    let words = "# Time Cd\n# liftDir : (0 up 0)\n";
    assert!(matches!(
        parse_coefficients(words).unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
    let not_finite = "# liftDir : (0 nan 0)\n# Time Cd\n";
    assert!(matches!(
        parse_coefficients(not_finite).unwrap_err(),
        MonitorError::Malformed { line: 1, .. }
    ));
}

#[test]
fn bad_files_are_refused() {
    let head = "# Time Cd Cl\n";
    assert_eq!(
        parse_coefficients(&format!("{head}1 nan 0.0\n")).unwrap_err(),
        MonitorError::NonFinite { line: 2 }
    );
    assert!(matches!(
        parse_coefficients(&format!("{head}1 1.0\n")).unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
    assert!(matches!(
        parse_coefficients(&format!("{head}1 1.0 0.0 5.0\n")).unwrap_err(),
        MonitorError::Malformed { line: 2, .. }
    ));
    assert!(matches!(
        parse_coefficients("1 1.0 0.0\n# Time Cd Cl\n").unwrap_err(),
        MonitorError::Malformed { line: 1, .. }
    ));
    assert!(matches!(
        parse_coefficients(&format!("{head}1 1.0 0.0\n# Time Cd Cl\n")).unwrap_err(),
        MonitorError::Malformed { line: 3, .. }
    ));
    assert!(matches!(
        parse_coefficients("# dragDir : (1 0 0)\n").unwrap_err(),
        MonitorError::Malformed { line: 0, .. }
    ));
    let mut long = String::from("# Time Cd\n");
    for i in 0..=MAX_ROWS {
        long.push_str(&format!("{i} 1\n"));
    }
    assert_eq!(
        parse_coefficients(&long).unwrap_err(),
        MonitorError::TooManyRows { max: MAX_ROWS }
    );
    let huge = format!("# Time Cd\n{}", "#".repeat(8 * 1024 * 1024));
    assert!(matches!(
        parse_coefficients(&huge).unwrap_err(),
        MonitorError::TooLarge { .. }
    ));
}

#[test]
fn restarted_files_join() {
    let first = "# Time Cd\n1 1.0\n2 2.0\n3 3.0\n";
    let second = "# Time Cd\n2 20.0\n3 30.0\n4 40.0\n";
    let parts = vec![
        coefficient_series(&parse_coefficients(first).unwrap(), "force_1").remove(0),
        coefficient_series(&parse_coefficients(second).unwrap(), "force_1").remove(0),
    ];
    let joined = join_series(parts);
    assert_eq!(joined.name, "force_1.Cd");
    assert_eq!(joined.times, vec![1.0, 2.0, 3.0, 4.0]);
    assert_eq!(joined.values, vec![1.0, 20.0, 30.0, 40.0]);
}
