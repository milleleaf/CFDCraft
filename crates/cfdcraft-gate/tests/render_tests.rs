use cfdcraft_bench_ghia::{Comparison, Row};
use cfdcraft_gate::{GateMetric, GateReport, render_report};

fn profile(name: &str, dev: f64, tolerance: f64, passed: bool) -> GateMetric {
    let row = |station, computed, reference, abs_dev| Row {
        station,
        computed,
        reference,
        abs_dev,
    };
    GateMetric::from_comparison(
        name,
        &Comparison {
            max_abs_dev: dev,
            tolerance,
            passed,
            rows: vec![row(1.0, 0.5, 0.48, 0.02), row(4.0, 0.25, 0.28, 0.03)],
        },
    )
}

fn drag() -> GateMetric {
    GateMetric::scalar("cDmax", 3.2301, [3.22, 3.24], 0.0648, Some("1"))
}

#[test]
fn a_scalar_metric_shows_computed_range_tolerance_and_verdict() {
    let text = render_report(&GateReport::from_metrics("cylinder", vec![drag()]));
    let line = text.lines().nth(1).unwrap();
    for part in ["cDmax", "3.2301", "3.2200 to 3.2400", "0.0648", "PASS"] {
        assert!(line.contains(part), "{part} missing in {line}");
    }

    let off = GateMetric::scalar("cDmax", 3.5, [3.22, 3.24], 0.0648, Some("1"));
    let text = render_report(&GateReport::from_metrics("cylinder", vec![off]));
    assert!(text.starts_with("cylinder verdict: FAILED\n"));
    let line = text.lines().nth(1).unwrap();
    assert!(line.contains("3.5000") && line.ends_with("FAIL"), "{line}");
}

#[test]
fn a_profile_metric_shows_its_worst_station() {
    let report = GateReport::from_metrics("step", vec![profile("u", 0.15, 0.1, false)]);
    let text = render_report(&report);
    let head = text.lines().nth(1).unwrap();
    assert_eq!(
        head,
        "u  max deviation 0.150000  tolerance 0.100000  worst station 4.0000  FAIL"
    );
    let rows: Vec<&str> = text.lines().skip(2).collect();
    assert_eq!(rows.len(), 3, "{text}");
    assert!(rows[0].contains("station") && rows[0].contains("deviation"));
    assert!(rows[1].contains("1.0000") && rows[1].contains("0.480000"));
    assert!(rows[2].contains("4.0000") && rows[2].contains("0.030000"));

    let ok = GateReport::from_metrics("step", vec![profile("u", 0.03, 0.1, true)]);
    assert!(render_report(&ok).lines().nth(1).unwrap().ends_with("PASS"));
}

#[test]
fn a_non_gating_metric_is_marked() {
    let mut info = GateMetric::scalar("St", 0.9, [0.295, 0.305], 0.0091, None);
    info.gating = false;
    let report = GateReport::from_metrics("cylinder", vec![drag(), info]);
    assert!(report.passed);
    let text = render_report(&report);
    assert!(text.starts_with("cylinder verdict: PASS\n"));
    let line = text.lines().nth(2).unwrap();
    assert!(line.starts_with("St") && line.ends_with("info"), "{line}");
    assert!(!line.contains("PASS") && !line.contains("FAIL"));
}

#[test]
fn the_report_header_and_notes() {
    let mut report = GateReport::from_metrics("cylinder", vec![drag()]);
    report.note_override("cDmax tolerance 0.0648 instead of 0.0500: coarse mesh");
    let text = render_report(&report);
    assert!(text.starts_with("cylinder verdict: PASS\n"));
    assert!(
        text.lines()
            .last()
            .unwrap()
            .starts_with("tolerance override used:")
    );
    assert!(text.find("cDmax  ").unwrap() < text.find("tolerance override").unwrap());

    let failed = GateReport::failed_named("cylinder", "the force file is missing");
    assert_eq!(
        render_report(&failed),
        "cylinder verdict: FAILED\n  the force file is missing\n"
    );
    assert_eq!(failed.render_text(), render_report(&failed));
}

#[test]
fn text_is_stable_and_has_no_braces_or_tabs() {
    let mut info = GateMetric::scalar("St", 0.31, [0.295, 0.305], 0.0091, None);
    info.gating = false;
    let mut report = GateReport::from_metrics(
        "cylinder",
        vec![drag(), profile("u", 0.03, 0.1, true), info],
    );
    report.note_override("cDmax tolerance 0.0648 instead of 0.0500: coarse mesh");
    let expected = concat!(
        "cylinder verdict: PASS\n",
        "cDmax  3.2301  3.2200 to 3.2400  0.0648  1  PASS\n",
        "u  max deviation 0.030000  tolerance 0.100000  worst station 4.0000  PASS\n",
        "     station    computed   reference   deviation\n",
        "      1.0000    0.500000    0.480000    0.020000\n",
        "      4.0000    0.250000    0.280000    0.030000\n",
        "St  0.3100  0.2950 to 0.3050  0.0091  -  info\n",
        "tolerance override used: cDmax tolerance 0.0648 instead of 0.0500: coarse mesh\n",
    );
    let text = report.render_text();
    assert_eq!(text, expected);
    assert!(!text.contains('\t') && !text.contains('{') && !text.contains('}'));
}
