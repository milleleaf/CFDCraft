//! Continuing a run that was stopped on request: the case is rebuilt from the settings the run
//! recorded, seeded with its saved state, and recorded as a new run that names its parent. No test
//! needs a compute host or a container engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_backend_esi::{CaseFile, Dialect};
use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::jobs::plan_job_with_tool;
use cfdcraft_ops::queue_ops::{AddRequest, queue_add};
use cfdcraft_ops::{
    ExitClass, JobAction, JobOptions, QueueData, Report, Repro, plan_job, plan_scenario_job_with,
    prepare, save_document, solve_document, tool_identity,
};
use cfdcraft_runs::{ContinuedFrom, Outcome, StageRecipes, ToolIdentity, latest_result};
use cfdcraft_store::Store;
use cfdcraft_text::Line;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

/// A document whose `run-1` was stopped at 1200, and a runner that goes to the end from now on.
fn stopped(tag: &str) -> (PathBuf, FixtureRunner) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.stopped_at = Some("1200".to_owned());
    passed(&solve_document(&path, &mut runner).report);
    runner.stopped_at = None;
    (path, runner)
}

fn go_on(run: &str) -> JobAction {
    JobAction::Continue {
        run: run.to_owned(),
    }
}

fn text_of<'a>(tree: &'a cfdcraft_backend_esi::CaseTree, rel: &str) -> &'a [u8] {
    &tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(rel))
        .unwrap_or_else(|| panic!("the case has {rel}"))
        .bytes
}

#[test]
fn a_stopped_run_can_be_continued() {
    let (path, mut runner) = stopped("cont-plan");
    let job = plan_scenario_job_with(&path, None, go_on("run-1"), &JobOptions::default())
        .expect("a stopped run can be continued");

    let store = Store::open(store_dir(&path)).unwrap();
    let seed = job.continue_seed(&store).expect("the saved state is read");
    let names: Vec<&Path> = seed.iter().map(|f| f.rel_path.as_path()).collect();
    assert_eq!(names, [Path::new("1200/U"), Path::new("1200/p")]);
    assert_eq!(seed[0].bytes, b"field U\n");
    assert_eq!(seed[1].bytes, b"field p\n");

    // The result is the only stage it makes.
    passed(&job.run_with(&mut runner).report);
    let runs = open(&path).unwrap().document.artifacts.runs;
    assert!(runs[1].mesh.is_none());
    assert!(runs[1].result.is_some());
}

#[test]
fn the_continued_case_has_start_from_latest_time_and_the_seed() {
    let (path, _runner) = stopped("cont-case");
    let job = plan_job(&path, go_on("run-1")).unwrap();
    let store = Store::open(store_dir(&path)).unwrap();
    let seed = job.continue_seed(&store).unwrap();

    let tree = job
        .case_tree(1, &seed)
        .expect("the continued case is written");
    let control = String::from_utf8(text_of(&tree, "system/controlDict").to_vec()).unwrap();
    let start = control
        .lines()
        .find(|line| line.trim_start().starts_with("startFrom"))
        .expect("the control dictionary says where to start");
    assert!(start.contains("latestTime"), "{start}");
    assert_eq!(text_of(&tree, "1200/U"), b"field U\n");
    assert_eq!(text_of(&tree, "1200/p"), b"field p\n");
    let allrun = String::from_utf8(text_of(&tree, "Allrun").to_vec()).unwrap();
    assert!(allrun.contains("simpleFoam"), "{allrun}");

    // Without a seed it is the ordinary export of the study.
    let plain = job.case_tree(1, &[]).unwrap();
    let expected = prepare(job.study_json(), Dialect::EsiV2512)
        .unwrap()
        .export_tree;
    assert_eq!(plain, expected);
}

#[test]
fn a_bad_seed_is_refused_by_name() {
    let (path, _runner) = stopped("cont-seed");
    let job = plan_job(&path, go_on("run-1")).unwrap();
    let store = Store::open(store_dir(&path)).unwrap();
    let seed = job.continue_seed(&store).unwrap();

    // Only one of the two fields the case needs.
    let report: Report = *job.case_tree(1, &seed[..1]).unwrap_err();
    assert_eq!(codes(&report), ["CFD-CONT-001"]);

    // A file that is not a field of the saved time.
    let stray = CaseFile {
        rel_path: "1200/../evil".into(),
        bytes: b"x".to_vec(),
    };
    let mut worse = seed.clone();
    worse.push(stray);
    let report: Report = *job.case_tree(1, &worse).unwrap_err();
    assert_eq!(codes(&report), ["CFD-CONT-001"]);
}

#[test]
fn finishing_a_continue_records_a_complete_run_naming_its_parent() {
    let (path, mut runner) = stopped("cont-finish");
    let job = plan_job(&path, go_on("run-1")).unwrap();
    let done = job.run_with(&mut runner);
    passed(&done.report);
    assert_eq!(done.report.operation, "continue");

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    let second = &runs[1];
    assert_eq!(second.id, "run-2");
    assert_eq!(
        second.continued_from,
        Some(ContinuedFrom {
            run: "run-1".to_owned(),
            time: "1200".to_owned()
        })
    );
    assert!(second.result.is_some());
    assert!(second.verdict.is_some());
    assert_eq!(second.outcome, Outcome::Complete);
    assert!(second.checkpoint.is_none());
}

#[test]
fn a_continued_run_is_not_a_rerun_and_does_not_replace_the_parent() {
    let (path, mut runner) = stopped("cont-parent");
    let first = open(&path).unwrap().document.artifacts.runs[0].clone();
    let job = plan_job(&path, go_on("run-1")).unwrap();
    passed(&job.run_with(&mut runner).report);

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs[0], first);
    assert!(runs[1].rerun_of.is_none());
    assert!(runs[1].comparison.is_none());
    let key = runs[1].result.as_ref().unwrap().key;
    let (owner, _) = latest_result(&runs, key).expect("the key has an owner");
    assert_eq!(owner.id, "run-2");
}

/// The report a refused plan answers with, checked to be the refusal of a continue.
fn refused_by_name(path: &Path, action: JobAction, fragment: &str) {
    let before = snapshot(path);
    let outcome = plan_job(path, action).unwrap_err();
    assert_eq!(codes(&outcome.report), ["CFD-CONT-001"]);
    assert_eq!(outcome.report.class(), ExitClass::StudyInvalid);
    let message = &outcome.report.diagnostics[0].message;
    assert!(message.contains(fragment), "{message}");
    assert!(
        message.starts_with("The run cannot be continued:"),
        "{message}"
    );
    assert_eq!(snapshot(path), before, "nothing was changed");
}

#[test]
fn continue_is_refused_by_name() {
    // A run that went to its end.
    let dir = scratch("cont-refused-complete");
    let complete = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&complete, &mut runner).report);
    refused_by_name(&complete, go_on("run-1"), "went to its end");

    // A run the document does not have, and a name that is not a run name at all.
    let (path, _runner) = stopped("cont-refused-unknown");
    refused_by_name(&path, go_on("run-9"), "no run called run-9");
    refused_by_name(&path, go_on("../x"), "no run called ../x");

    // A saved state that was removed from the store.
    let (gone, _runner) = stopped("cont-refused-gone");
    let saved = open(&gone).unwrap().document.artifacts.runs[0]
        .checkpoint
        .clone()
        .unwrap();
    Store::open(store_dir(&gone))
        .unwrap()
        .remove(&saved.blob)
        .unwrap();
    refused_by_name(&gone, go_on("run-1"), "no longer in the store");

    // A run made with other tools than this build's.
    let dir = scratch("cont-refused-tool");
    let other = new_document(&dir, "cavity", CAVITY);
    let current = tool_identity(Dialect::EsiV2512);
    let old = ToolIdentity::new(
        "sha256:old",
        current.dialect.clone(),
        current.sidecar_lock.clone(),
        current.writer_version.clone(),
    );
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.stopped_at = Some("1200".to_owned());
    let Ok(job) = plan_job_with_tool(&other, JobAction::Solve, old) else {
        panic!("the document is planned for solving");
    };
    passed(&job.run_with(&mut runner).report);
    refused_by_name(&other, go_on("run-1"), "other tools");
    assert_eq!(runner.calls, 1, "nothing ran");
}

#[test]
fn the_continue_uses_the_settings_the_run_recorded_not_the_current_study() {
    let (path, mut runner) = stopped("cont-recorded");
    let recorded = open(&path).unwrap().document.artifacts.runs[0]
        .provenance
        .settings
        .clone();
    passed(&save_document(&with_lid_speed(0.5), &path, true).report);
    assert_ne!(open(&path).unwrap().document.study, recorded);

    let job = plan_job(&path, go_on("run-1")).unwrap();
    let expected = serde_json::to_string_pretty(&recorded).unwrap();
    assert_eq!(job.study_json(), expected.as_str());
    passed(&job.run_with(&mut runner).report);
    assert_eq!(runner.last_study.as_deref(), Some(expected.as_str()));

    let keys = StageRecipes::from_resolved(
        &cfdcraft_model::resolve(&recorded).unwrap(),
        &tool_identity(Dialect::EsiV2512),
    )
    .keys();
    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs[1].result.as_ref().unwrap().key, keys.result);
    assert_eq!(runs[1].provenance.settings, recorded);
}

fn queue_of(report: &Report) -> &QueueData {
    match &report.repro {
        Some(Repro::Queue(data)) => data.as_ref(),
        other => panic!("expected the queue, found {other:?}"),
    }
}

fn queue_bytes(doc: &Path) -> Option<Vec<u8>> {
    fs::read(store_dir(doc).join("queue.json")).ok()
}

#[test]
fn queue_add_continue_enqueues_a_continue_job_and_refuses_what_the_planner_refuses() {
    let (path, _runner) = stopped("cont-queue");
    let request = AddRequest {
        continue_run: Some("run-1".to_owned()),
        ..AddRequest::default()
    };
    let added = queue_add(&path, &request);
    passed(&added.report);
    let jobs = &queue_of(&added.report).jobs;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, "continue");
    assert_eq!(jobs[0].scenario, None);
    assert_eq!(jobs[0].state, "queued");

    // Asked for again while the first waits: added once.
    let again = queue_add(&path, &request);
    assert_eq!(again.line, Some(Line::QueueNothingToAdd));
    assert_eq!(queue_of(&again.report).jobs.len(), 1);

    // What the planner refuses passes through with its own code, and the queue is not touched.
    let dir = scratch("cont-queue-complete");
    let complete = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&complete, &mut runner).report);
    let refused = queue_add(&complete, &request);
    assert_eq!(codes(&refused.report), ["CFD-CONT-001"]);
    assert_eq!(refused.report.class(), ExitClass::StudyInvalid);
    assert_eq!(queue_bytes(&complete), None);

    // A continue is not combined with a variant or a sweep.
    for both in [
        AddRequest {
            scenario: Some("fast-lid".to_owned()),
            ..request.clone()
        },
        AddRequest {
            sweep: Some("lids".to_owned()),
            ..request.clone()
        },
    ] {
        let out = queue_add(&path, &both);
        assert_eq!(codes(&out.report), ["CFD-QUEUE-001"]);
    }
}
