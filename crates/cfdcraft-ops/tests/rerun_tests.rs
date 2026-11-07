//! Rerunning a recorded result from its provenance alone and comparing it with the original. A
//! rerun is recorded whatever it finds. No test needs a compute host or a container engine.

use std::path::{Path, PathBuf};

use cfdcraft_backend_esi::Dialect;
use cfdcraft_compare::{Tolerance, Verdict};
use cfdcraft_doc::open;
use cfdcraft_model::{Diagnostic, Severity};
use cfdcraft_ops::jobs::plan_job_with_tool;
use cfdcraft_ops::{
    ComparisonData, DocOutcome, ExitClass, JobAction, Report, Repro, document_status, list_runs,
    plan_job, save_document, solve_document, tool_identity,
};
use cfdcraft_runs::{ToolIdentity, latest_result};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

fn comparison_of(report: &Report) -> &ComparisonData {
    match &report.repro {
        Some(Repro::Comparison(data)) => data,
        other => panic!("expected a comparison, found {other:?}"),
    }
}

/// The only finding of a report, checked to have this code.
fn sole<'a>(report: &'a Report, code: &str) -> &'a Diagnostic {
    assert_eq!(report.diagnostics.len(), 1, "{}", report.render_text());
    let found = &report.diagnostics[0];
    assert_eq!(found.code, code, "{}", report.render_text());
    found
}

/// Plan and carry out a rerun of `run`.
fn rerun(
    path: &Path,
    run: &str,
    runner: &mut FixtureRunner,
    tolerance: Tolerance,
    allow_tool_change: bool,
) -> DocOutcome {
    let action = JobAction::Rerun {
        run: run.to_owned(),
        tolerance,
        allow_tool_change,
    };
    match plan_job(path, action) {
        Ok(job) => job.run_with(runner),
        Err(outcome) => *outcome,
    }
}

fn plain_rerun(path: &Path, run: &str, runner: &mut FixtureRunner) -> DocOutcome {
    rerun(path, run, runner, Tolerance::same_image(), false)
}

/// A document solved once; the shifts are those of the runner's calls in turn.
fn solved(tag: &str, shifts: Vec<f64>) -> (PathBuf, FixtureRunner) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = shifts;
    passed(&solve_document(&path, &mut runner).report);
    (path, runner)
}

fn failing_report() -> Report {
    let mut failing = Report::new("run");
    failing.set_class(ExitClass::Infrastructure);
    failing.diagnostics.push(Diagnostic::error(
        "CFD-INFRA-001",
        "compute host",
        "",
        "The compute host could not be reached.",
    ));
    failing
}

#[test]
fn a_rerun_that_reproduces_the_original_is_recorded_with_its_comparison() {
    let (path, mut runner) = solved("rerun-same", Vec::new());
    let first = open(&path).unwrap().document.artifacts.runs[0].clone();

    let done = plain_rerun(&path, "run-1", &mut runner);
    passed(&done.report);
    assert_eq!(done.report.operation, "rerun");
    let line = done.line.as_ref().expect("a sentence for a person");
    assert_eq!(
        say(line),
        "run-2 reproduced run-1: every checked quantity is identical."
    );

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0], first);
    let second = &runs[1];
    assert_eq!(second.id, "run-2");
    assert_eq!(second.rerun_of.as_deref(), Some("run-1"));
    assert!(second.mesh.is_none());
    let (original, repeated) = (
        first.result.as_ref().unwrap(),
        second.result.as_ref().unwrap(),
    );
    assert_eq!(repeated.key, original.key);
    assert_eq!(second.provenance.mesh_hash, first.provenance.mesh_hash);
    assert_eq!(second.provenance.settings, first.provenance.settings);
    let comparison = second.comparison.as_ref().expect("the comparison is kept");
    assert_eq!(comparison.verdict, Verdict::Reproduced);

    let (owner, _) = latest_result(&runs, original.key).expect("the key is still owned");
    assert_eq!(owner.id, "run-1");

    let data = comparison_of(&done.report);
    assert_eq!(data.original, "run-1");
    assert_eq!(data.other, "run-2");
    assert_eq!(&data.comparison, comparison);
}

#[test]
fn the_rerun_uses_the_recorded_settings_not_the_current_study() {
    let (path, mut runner) = solved("rerun-settings", Vec::new());
    let recorded = open(&path).unwrap().document.artifacts.runs[0]
        .provenance
        .settings
        .clone();
    passed(&save_document(&with_lid_speed(0.5), &path, true).report);
    let edited = open(&path).unwrap().document.study;
    assert_ne!(edited, recorded);

    let done = plain_rerun(&path, "run-1", &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    assert_eq!(
        runner.last_study.as_deref(),
        Some(serde_json::to_string_pretty(&recorded).unwrap().as_str())
    );
    assert_ne!(
        runner.last_study.as_deref(),
        Some(serde_json::to_string_pretty(&edited).unwrap().as_str())
    );
    // The case the runner made follows the study it was given, so these are the first numbers.
    assert_eq!(
        comparison_of(&done.report).comparison.verdict,
        Verdict::Reproduced
    );

    let status = document_status(&path);
    match &status.report.repro {
        Some(Repro::Status(data)) => {
            assert_eq!(data.mesh.state, "fresh");
            assert_eq!(data.result.state, "stale");
            assert_eq!(data.result.since.as_deref(), Some("run-1"));
        }
        other => panic!("expected the status, found {other:?}"),
    }
}

#[test]
fn a_rerun_within_tolerance_warns_and_is_recorded() {
    // The samples are written with twelve decimals, so the smallest shift that survives the
    // writing is 1e-12; it is inside the default tolerance.
    let (path, mut runner) = solved("rerun-within", vec![0.0, 1e-12]);

    let done = plain_rerun(&path, "run-1", &mut runner);
    passed(&done.report);
    assert!(
        codes(&done.report).contains(&"CFD-REPRO-010"),
        "{}",
        done.report.render_text()
    );
    let data = comparison_of(&done.report);
    assert_eq!(data.comparison.verdict, Verdict::WithinTolerance);
    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    let kept = runs[1].comparison.as_ref().unwrap();
    assert_eq!(kept.verdict, Verdict::WithinTolerance);
}

#[test]
fn a_rerun_that_differs_is_class_1_and_still_recorded() {
    let (path, mut runner) = solved("rerun-differs", vec![0.0, 0.1]);

    let done = plain_rerun(&path, "run-1", &mut runner);
    assert_eq!(
        done.report.class(),
        ExitClass::GateFailed,
        "{}",
        done.report.render_text()
    );
    assert!(codes(&done.report).contains(&"CFD-REPRO-007"));
    assert_eq!(
        comparison_of(&done.report).comparison.verdict,
        Verdict::Differs
    );

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].id, "run-2");
    assert_eq!(runs[1].rerun_of.as_deref(), Some("run-1"));
    let kept = runs[1].comparison.as_ref().unwrap();
    assert_eq!(kept.verdict, Verdict::Differs);
    assert!(kept.differing.iter().any(|d| !d.within));
}

#[test]
fn a_different_tool_is_refused_unless_allowed() {
    let dir = scratch("rerun-tool");
    let path = new_document(&dir, "cavity", CAVITY);
    let current = tool_identity(Dialect::EsiV2512);
    let old = ToolIdentity::new(
        "sha256:old",
        current.dialect.clone(),
        current.sidecar_lock.clone(),
        current.writer_version.clone(),
    );
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let Ok(job) = plan_job_with_tool(&path, JobAction::Solve, old) else {
        panic!("the document is planned for solving");
    };
    passed(&job.run_with(&mut runner).report);
    assert_eq!(runner.calls, 1);
    let before = snapshot(&path);

    let refused = plain_rerun(&path, "run-1", &mut runner);
    assert_eq!(refused.report.class(), ExitClass::StudyInvalid);
    let found = sole(&refused.report, "CFD-REPRO-006");
    assert!(found.message.contains("solver image"), "{}", found.message);
    assert_eq!(runner.calls, 1);
    assert_eq!(snapshot(&path), before);

    let allowed = rerun(&path, "run-1", &mut runner, Tolerance::same_image(), true);
    passed(&allowed.report);
    assert_eq!(runner.calls, 2);
    let warning = allowed
        .report
        .diagnostics
        .iter()
        .find(|d| d.code == "CFD-REPRO-006")
        .expect("the change of tool is told");
    assert_eq!(warning.severity, Severity::Warning);
    assert!(warning.message.contains("solver image"));

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    let (original, repeated) = (
        runs[0].result.as_ref().unwrap(),
        runs[1].result.as_ref().unwrap(),
    );
    assert_ne!(repeated.key, original.key);
    assert_eq!(runs[1].provenance.tool, current);
    assert_eq!(runs[1].rerun_of.as_deref(), Some("run-1"));
    assert!(runs[1].comparison.is_some());
    assert!(comparison_of(&allowed.report).comparison.compared > 0);
}

#[test]
fn an_unknown_run_or_a_run_without_a_result_is_refused() {
    let (path, mut runner) = solved("rerun-unknown", Vec::new());
    for name in ["run-9", "../x"] {
        let done = plain_rerun(&path, name, &mut runner);
        assert_eq!(done.report.class(), ExitClass::StudyInvalid, "{name}");
        sole(&done.report, "CFD-REPRO-004");
    }

    add_mesh_only_run(&path);
    let before = snapshot(&path);
    let done = plain_rerun(&path, "run-2", &mut runner);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    sole(&done.report, "CFD-REPRO-005");
    assert_eq!(runner.calls, 1);
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_record_whose_result_key_was_edited_is_refused() {
    let (path, mut runner) = solved("rerun-edited", Vec::new());
    let key = open(&path).unwrap().document.artifacts.runs[0]
        .provenance
        .result_key
        .unwrap()
        .to_string();
    let other = "ab".repeat(32);
    assert_ne!(key, other);
    tamper_with_index(
        &path,
        &format!("\"result_key\": \"{key}\""),
        &format!("\"result_key\": \"{other}\""),
    );
    let before = snapshot(&path);

    let done = plain_rerun(&path, "run-1", &mut runner);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    sole(&done.report, "CFD-REPRO-003");
    assert_eq!(runner.calls, 1);

    let solving = solve_document(&path, &mut runner);
    sole(&solving.report, "CFD-REPRO-003");
    sole(&document_status(&path).report, "CFD-REPRO-003");
    sole(&list_runs(&path).report, "CFD-REPRO-003");
    assert_eq!(runner.calls, 1);
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_failed_runner_records_nothing() {
    let (path, mut runner) = solved("rerun-failed", Vec::new());
    let before = snapshot(&path);
    let failing = failing_report();
    runner.fail_with = Some(failing.clone());

    let done = plain_rerun(&path, "run-1", &mut runner);
    assert_eq!(runner.calls, 2);
    assert_eq!(done.report.class(), ExitClass::Infrastructure);
    assert_eq!(done.report.operation, "rerun");
    assert_eq!(done.report.diagnostics, failing.diagnostics);
    assert!(done.line.is_none());
    assert_eq!(snapshot(&path), before);
}

#[test]
fn the_rerun_runs_the_case_exactly_once() {
    let (path, mut runner) = solved("rerun-once", Vec::new());
    assert_eq!(runner.calls, 1);
    passed(&plain_rerun(&path, "run-1", &mut runner).report);
    assert_eq!(runner.calls, 2);
    passed(&plain_rerun(&path, "run-1", &mut runner).report);
    assert_eq!(runner.calls, 3);
    assert_eq!(open(&path).unwrap().document.artifacts.runs.len(), 3);
}
