//! A run stopped on request is recorded with its mesh and its saved state at the latest time it
//! wrote, and with no result, so the result stays out of date. No test needs a compute host or a
//! container engine.

use std::path::PathBuf;

use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::{
    ExitClass, JobAction, Repro, document_status, list_runs, plan_job, solve_document,
};
use cfdcraft_runs::{Outcome, verify_runs};
use cfdcraft_store::Store;
use cfdcraft_text::Line;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

/// A document made from the cavity, and a runner that reports every run as stopped at 1200.
fn setup(tag: &str) -> (PathBuf, FixtureRunner) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.stopped_at = Some("1200".to_owned());
    runner.monitors = true;
    (path, runner)
}

#[test]
fn a_stopped_run_is_recorded_without_a_result() {
    let (path, mut runner) = setup("stopped-record");
    runner.np = Some(2);
    let done = solve_document(&path, &mut runner);
    passed(&done.report);

    let document = open(&path).unwrap().document;
    assert_eq!(document.artifacts.runs.len(), 1);
    let run = &document.artifacts.runs[0];
    assert_eq!(run.id, "run-1");
    assert_eq!(
        run.outcome,
        Outcome::Stopped {
            at_time: "1200".to_owned()
        }
    );
    let saved = run
        .checkpoint
        .as_ref()
        .expect("the saved state is recorded");
    assert_eq!(saved.time, "1200");
    let store = Store::open(store_dir(&path)).unwrap();
    assert!(store.contains(&saved.blob));
    assert_eq!(
        std::fs::metadata(store.blob_path(&saved.blob))
            .unwrap()
            .len(),
        saved.size
    );
    assert!(run.mesh.is_some());
    assert!(run.result.is_none());
    assert!(run.provenance.result_key.is_none());
    assert!(run.verdict.is_none());
    assert_eq!(run.np, Some(2));
    assert!(run.continued_from.is_none());
}

#[test]
fn a_stopped_run_leaves_the_result_out_of_date() {
    let (path, mut runner) = setup("stopped-status");
    passed(&solve_document(&path, &mut runner).report);

    let status = document_status(&path);
    let Some(Repro::Status(data)) = &status.report.repro else {
        panic!("expected the status, found {:?}", status.report.repro);
    };
    assert_eq!(data.mesh.state, "fresh");
    assert_eq!(data.result.state, "never_made");
    // A new solve plans the result again.
    assert!(plan_job(&path, JobAction::Solve).is_ok());
}

#[test]
fn the_report_says_the_run_is_stopped_and_can_be_continued() {
    let (path, mut runner) = setup("stopped-report");
    let done = solve_document(&path, &mut runner);
    assert_eq!(done.report.class(), ExitClass::Passed);

    let line = done.line.as_ref().expect("a sentence for a person");
    assert_eq!(
        line,
        &Line::StoppedRunNote {
            run: "run-1".to_owned(),
            time: "1200".to_owned()
        }
    );
    assert_eq!(
        say(line),
        "run-1 is a stopped run (latest time 1200); it holds no finished result."
    );
    let recorded = data(&done.report);
    assert_eq!(recorded.run.as_deref(), Some("run-1"));
    assert_eq!(recorded.recorded, ["mesh"]);
    assert!(recorded.reused.is_empty());
}

#[test]
fn a_stopped_run_is_judged_by_the_partial_rules_in_the_checks() {
    let (path, mut runner) = setup("stopped-checks");
    passed(&solve_document(&path, &mut runner).report);

    // The same run with a check that failed is refused as a normal run would be.
    let (other, mut failing) = setup("stopped-failing");
    failing.failing_check = Some("evidence".to_owned());
    let before = snapshot(&other);
    let done = solve_document(&other, &mut failing);
    assert_eq!(done.report.class(), ExitClass::GateFailed);
    assert!(codes(&done.report).contains(&"CFD-REPRO-009"));
    assert!(open(&other).unwrap().document.artifacts.runs.is_empty());
    assert_eq!(snapshot(&other), before);
}

#[test]
fn a_checkpoint_without_the_fields_is_not_recorded() {
    let (path, mut runner) = setup("stopped-missing");
    runner.omit = Some("1200/p");
    let before = snapshot(&path);
    let done = solve_document(&path, &mut runner);

    assert_eq!(done.report.class(), ExitClass::GateFailed);
    let found = done
        .report
        .diagnostics
        .iter()
        .find(|d| d.code == "CFD-REPRO-009")
        .expect("the reason is told");
    assert!(found.message.contains("1200/p"), "{}", found.message);
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_stopped_run_with_the_mesh_already_stored_records_no_new_mesh() {
    let (path, mut runner) = setup("stopped-twice");
    passed(&solve_document(&path, &mut runner).report);
    runner.stopped_at = Some("1300".to_owned());
    let second = solve_document(&path, &mut runner);
    passed(&second.report);
    assert!(data(&second.report).recorded.is_empty());
    assert_eq!(data(&second.report).reused, ["mesh"]);

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    assert!(runs[1].mesh.is_none());
    assert_eq!(
        runs[1].provenance.mesh_hash,
        runs[0].mesh.as_ref().unwrap().blob
    );
    assert_eq!(runs[1].checkpoint.as_ref().unwrap().time, "1300");
}

#[test]
fn the_document_with_a_stopped_run_still_opens_and_verifies() {
    let (path, mut runner) = setup("stopped-opens");
    passed(&solve_document(&path, &mut runner).report);

    let document = open(&path).unwrap().document;
    verify_runs(&document.artifacts.runs).expect("the log holds together");
    let listed = list_runs(&path);
    assert_eq!(listed.report.class(), ExitClass::Passed);
    let Some(Repro::Runs(runs)) = &listed.report.repro else {
        panic!("expected the runs, found {:?}", listed.report.repro);
    };
    assert_eq!(runs.runs[0].recorded, ["mesh"]);
    assert_eq!(runs.runs[0].result_key, None);
}
