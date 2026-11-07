//! Comparing two recorded runs on their key numbers under a stated tolerance. Comparing never
//! writes.

use cfdcraft_compare::{Tolerance, Verdict};
use cfdcraft_ops::{ComparisonData, ExitClass, Report, Repro, compare_runs};
use serde_json::Value;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

fn data_of(report: &Report) -> &ComparisonData {
    match &report.repro {
        Some(Repro::Comparison(data)) => data,
        other => panic!("expected a comparison, found {other:?}"),
    }
}

#[test]
fn identical_numbers_are_reproduced() {
    // The second run is a different run (the lid speed changed) whose samples are the same.
    let (path, _) = two_runs("compare-same", 0.2, vec![0.0]);

    let done = compare_runs(&path, "run-1", "run-2", Tolerance::same_image());
    passed(&done.report);
    assert_eq!(done.report.operation, "compare");
    assert!(
        done.report.diagnostics.is_empty(),
        "{}",
        done.report.render_text()
    );
    let data = data_of(&done.report);
    assert_eq!(data.original, "run-1");
    assert_eq!(data.other, "run-2");
    assert_eq!(data.comparison.verdict, Verdict::Reproduced);
    assert!(data.comparison.differing.is_empty());
    assert!(data.comparison.compared > 128);
    assert_eq!(
        done.line.as_ref().map(say).as_deref(),
        Some("run-2 reproduced run-1: every checked quantity is identical.")
    );

    let report: Value = serde_json::from_str(&done.report.to_json()).unwrap();
    assert_eq!(report["repro"]["kind"], "comparison");
    assert_eq!(report["repro"]["comparison"]["verdict"], "reproduced");
}

#[test]
fn a_tiny_difference_is_within_tolerance_and_warns() {
    // The samples are written with twelve decimals, so the smallest shift that survives the
    // writing is 1e-12; it is inside the default tolerance.
    let (path, _) = two_runs("compare-tiny", 0.2, vec![0.0, 1e-12]);

    let done = compare_runs(&path, "run-1", "run-2", Tolerance::same_image());
    passed(&done.report);
    let data = data_of(&done.report);
    assert_eq!(
        data.comparison.verdict,
        Verdict::WithinTolerance,
        "{:?}",
        data.comparison
    );
    assert!(!data.comparison.differing.is_empty());
    assert!(data.comparison.differing.iter().all(|d| d.within));
    assert_eq!(done.report.diagnostics.len(), 1);
    let warning = &done.report.diagnostics[0];
    assert_eq!(warning.code, "CFD-REPRO-010");
    assert!(
        warning
            .message
            .starts_with("run-2 is within tolerance of run-1; the largest difference is"),
        "{}",
        warning.message
    );
    let line = done.line.as_ref().map(say).unwrap();
    assert_eq!(line, warning.message);
}

#[test]
fn a_large_difference_differs_with_class_1() {
    let (path, _) = two_runs("compare-large", 0.2, vec![0.0, 0.1]);

    let done = compare_runs(&path, "run-1", "run-2", Tolerance::same_image());
    assert_eq!(
        done.report.class(),
        ExitClass::GateFailed,
        "{}",
        done.report.render_text()
    );
    let data = data_of(&done.report);
    assert_eq!(data.comparison.verdict, Verdict::Differs);
    let outside = data.comparison.differing.iter().filter(|d| !d.within);
    assert!(outside.clone().count() > 0);
    assert!(
        data.comparison
            .differing
            .iter()
            .any(|d| d.name.starts_with("centreline."))
    );

    assert_eq!(done.report.diagnostics.len(), 1);
    let found = &done.report.diagnostics[0];
    assert_eq!(found.code, "CFD-REPRO-007");
    assert_eq!(
        found.message,
        format!(
            "run-2 differs from run-1; quantities outside the tolerance: {}.",
            outside.count()
        )
    );
    assert!(done.line.is_none());
}

#[test]
fn a_looser_tolerance_can_make_a_difference_acceptable() {
    let (path, _) = two_runs("compare-loose", 0.2, vec![0.0, 0.1]);

    let loose = Tolerance::new(0.0, 1.0).unwrap();
    let done = compare_runs(&path, "run-1", "run-2", loose);
    passed(&done.report);
    let data = data_of(&done.report);
    assert_ne!(data.comparison.verdict, Verdict::Differs);
    assert_eq!(data.comparison.tolerance, loose);
}

#[test]
fn both_runs_must_exist_and_have_a_result() {
    let (path, _) = two_runs("compare-missing", 0.2, vec![0.0]);
    add_mesh_only_run(&path);

    for (original, other, code) in [
        ("run-1", "run-9", "CFD-REPRO-004"),
        ("run-9", "run-1", "CFD-REPRO-004"),
        ("../x", "run-1", "CFD-REPRO-004"),
        ("run-1", "RUN-2", "CFD-REPRO-004"),
        ("run-1", "run-3", "CFD-REPRO-005"),
        ("run-3", "run-1", "CFD-REPRO-005"),
    ] {
        let done = compare_runs(&path, original, other, Tolerance::same_image());
        assert_eq!(
            done.report.class(),
            ExitClass::StudyInvalid,
            "{original} {other}"
        );
        assert_eq!(done.report.diagnostics.len(), 1, "{original} {other}");
        assert_eq!(done.report.diagnostics[0].code, code, "{original} {other}");
        assert!(done.report.repro.is_none());
    }
    let done = compare_runs(&path, "run-1", "run-3", Tolerance::same_image());
    assert_eq!(
        done.report.diagnostics[0].message,
        "run-3 recorded no result, so there is nothing to rerun or compare."
    );
}

#[test]
fn comparing_a_run_with_itself_is_reproduced() {
    let (path, _) = two_runs("compare-itself", 0.2, Vec::new());

    let done = compare_runs(&path, "run-2", "run-2", Tolerance::same_image());
    passed(&done.report);
    assert_eq!(
        data_of(&done.report).comparison.verdict,
        Verdict::Reproduced
    );
    assert_eq!(
        done.line.as_ref().map(say).as_deref(),
        Some("run-2 reproduced run-2: every checked quantity is identical.")
    );
}

#[test]
fn compare_never_writes() {
    let (path, _) = two_runs("compare-never-writes", 0.2, vec![0.0, 0.1]);
    add_mesh_only_run(&path);
    let before = snapshot(&path);

    let tolerances = [Tolerance::same_image(), Tolerance::new(0.0, 1.0).unwrap()];
    for tolerance in tolerances {
        compare_runs(&path, "run-1", "run-2", tolerance);
        compare_runs(&path, "run-1", "run-1", tolerance);
        compare_runs(&path, "run-1", "run-3", tolerance);
        compare_runs(&path, "run-1", "run-9", tolerance);
    }
    assert_eq!(snapshot(&path), before);
}

#[test]
fn an_inconsistent_log_is_a_class_2_failure() {
    let (path, _) = two_runs("compare-inconsistent", 0.2, vec![0.0]);
    tamper_with_index(&path, "\"x\": 128,", "\"x\": 129,");

    let done = compare_runs(&path, "run-1", "run-2", Tolerance::same_image());
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    assert_eq!(done.report.diagnostics[0].code, "CFD-REPRO-003");
}
