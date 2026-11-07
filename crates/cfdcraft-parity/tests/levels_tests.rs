use std::collections::BTreeMap;

use cfdcraft_compare::{KeyNumbers, Tolerance, Verdict};
use cfdcraft_parity::{
    GateResult, Level, Levels, ParityInput, ParityIssue, ParityReport, RecordedColumn, build,
};

const NU_HOT: &str = "bench.dvd.nu_hot";

fn column(backend: &str, value: f64) -> RecordedColumn {
    RecordedColumn {
        backend: backend.to_owned(),
        image: format!("sha256:{backend}"),
        run: "run-1".to_owned(),
        preview: false,
        gate_passed: true,
        converged: Some(true),
        numbers: KeyNumbers::from([(NU_HOT.to_owned(), value)]),
    }
}

/// The baseline holds 4.0 and the tolerance is 0.5; every column has its golden compile passed.
fn input(columns: Vec<RecordedColumn>) -> ParityInput {
    ParityInput {
        benchmark: "dvd".to_owned(),
        case: "ra1e5 medium".to_owned(),
        golden: columns
            .iter()
            .map(|c| (c.backend.clone(), Level::Pass))
            .collect(),
        baseline: column("esi-v2512", 4.0),
        columns,
        tolerances: BTreeMap::from([(NU_HOT.to_owned(), Tolerance { abs: 0.5, rel: 0.0 })]),
    }
}

fn all(level: Level) -> Levels {
    Levels {
        golden: level,
        oracle: level,
        parity: level,
    }
}

#[test]
fn a_column_that_failed_its_gate_is_shown_and_does_not_count() {
    let mut bad = column("su2-8.6.0", 4.25);
    bad.gate_passed = false;
    let (report, issues) = build(&input(vec![bad.clone()]));
    let shown = &report.columns[0];
    assert_eq!(shown.gate, GateResult::Fail);
    assert_eq!(shown.rows.len(), 1);
    assert_eq!(shown.levels.oracle, Level::Fail);
    assert_eq!(
        issues,
        vec![ParityIssue::GateFailed {
            backend: "su2-8.6.0".to_owned()
        }]
    );
    assert_eq!(report.levels, all(Level::NotRun));

    // Beside a column that counts, the failed one changes nothing in the aggregate.
    let (both, _) = build(&input(vec![column("esi-v2606", 4.25), bad]));
    assert_eq!(both.levels, all(Level::Pass));
    assert_eq!(both.columns.len(), 2);
}

#[test]
fn an_unconverged_column_is_shown_and_does_not_count() {
    for converged in [Some(false), None] {
        let mut col = column("esi-v2606", 4.0);
        col.converged = converged;
        let (report, issues) = build(&input(vec![col]));
        assert!(!report.columns[0].converged);
        assert_eq!(report.columns[0].rows.len(), 1);
        assert!(!report.converged["esi-v2606"]);
        assert_eq!(
            issues,
            vec![ParityIssue::NotConverged {
                backend: "esi-v2606".to_owned()
            }]
        );
        assert_eq!(report.levels, all(Level::NotRun));
        assert!(!report.admitted());
    }
}

#[test]
fn all_three_levels_must_pass_for_a_column_to_be_admitted() {
    let (ok, issues) = build(&input(vec![column("esi-v2606", 4.25)]));
    assert!(issues.is_empty());
    assert_eq!(ok.levels, all(Level::Pass));
    assert!(ok.admitted());

    // A golden result that did not run keeps the table from passing.
    let mut missing_golden = input(vec![column("esi-v2606", 4.25)]);
    missing_golden.golden.clear();
    let (report, _) = build(&missing_golden);
    assert_eq!(report.columns[0].levels.golden, Level::NotRun);
    assert_eq!(report.levels.golden, Level::NotRun);
    assert_eq!(report.levels.parity, Level::Pass);
    assert!(!report.admitted());

    // A quantity outside the tolerance fails parity for the column and the table.
    let (far, issues) = build(&input(vec![column("esi-v2606", 5.0)]));
    assert!(issues.is_empty());
    assert_eq!(far.columns[0].rows[0].verdict, Verdict::Differs);
    assert_eq!(far.columns[0].levels.parity, Level::Fail);
    assert_eq!(far.levels.parity, Level::Fail);
    assert!(!far.admitted());

    // A failed golden compile fails the table even when the numbers agree.
    let mut failed_golden = input(vec![column("esi-v2606", 4.0)]);
    failed_golden
        .golden
        .insert("esi-v2606".to_owned(), Level::Fail);
    assert_eq!(build(&failed_golden).0.levels.golden, Level::Fail);

    // One counted column that passes cannot hide another that does not.
    let (mixed, _) = build(&input(vec![
        column("esi-v2606", 4.25),
        column("su2-8.6.0", 5.0),
    ]));
    assert_eq!(mixed.levels.parity, Level::Fail);
    assert!(!mixed.admitted());

    // With no column at all nothing was run.
    let (empty, _) = build(&input(Vec::new()));
    assert_eq!(empty.levels, all(Level::NotRun));
}

#[test]
fn a_preview_column_is_labelled_and_never_the_baseline() {
    let mut preview = column("fieldcraft-lbm-0.7.5", 4.25);
    preview.preview = true;
    let (report, issues) = build(&input(vec![preview.clone()]));
    assert!(issues.is_empty());
    assert!(report.columns[0].preview);

    let mut as_baseline = input(vec![column("esi-v2606", 4.0)]);
    as_baseline.baseline = preview;
    let (report, issues) = build(&as_baseline);
    assert!(issues.contains(&ParityIssue::BaselineMissing));
    assert!(report.columns[0].rows.is_empty());
    assert_eq!(report.columns[0].levels.parity, Level::NotRun);
    assert!(!report.admitted());
}

#[test]
fn the_baseline_may_have_a_failing_gate_and_is_still_a_baseline() {
    let mut failing = input(vec![column("esi-v2606", 4.25)]);
    failing.baseline.gate_passed = false;
    let (report, issues) = build(&failing);
    assert!(issues.is_empty());
    assert_eq!(report.baseline.gate, GateResult::Fail);
    let row = &report.columns[0].rows[0];
    assert_eq!(row.verdict, Verdict::WithinTolerance);
    assert_eq!(row.tolerance, Tolerance { abs: 0.5, rel: 0.0 });
}

#[test]
fn the_report_round_trips_through_json() {
    let (report, _) = build(&input(vec![column("esi-v2606", 4.25)]));
    let text = serde_json::to_string(&report).expect("a report serialises");
    let back: ParityReport = serde_json::from_str(&text).expect("a report parses");
    assert_eq!(back, report);

    let value = serde_json::to_value(&report).expect("a report serialises");
    assert_eq!(value["baseline"]["backend"], "esi-v2512");
    assert_eq!(value["baseline"]["gate"], "PASS");
    assert_eq!(value["levels"]["golden"], "pass");
    assert_eq!(value["converged"]["esi-v2606"], true);
    let column = &value["columns"][0];
    assert_eq!(column["gate"], "PASS");
    assert_eq!(column["levels"]["parity"], "pass");
    let row = &column["rows"][0];
    for key in [
        "quantity",
        "value",
        "baseline",
        "abs_diff",
        "tolerance",
        "verdict",
    ] {
        assert!(row.get(key).is_some(), "the row has a {key} key");
    }
    assert_eq!(row["verdict"], "within_tolerance");
}
