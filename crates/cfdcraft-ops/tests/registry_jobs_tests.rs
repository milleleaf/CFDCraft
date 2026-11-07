//! The run registry's general start: a job is finished by a step that turns the finished run into
//! the report kept for polling. The compute host is a fake ssh and a fake docker that answers with
//! one prepared, passing results archive.
#![cfg(unix)]

mod support;

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_ops::{
    ExitClass, Finished, Report, ResultOutcome, RunMode, RunRegistry, StartOutcome, Status,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use support::*;

const GROUP: &str = "ops-registry-jobs";

/// What the step saw: the class of the finished run and the id of its run.
type Seen = Arc<Mutex<Option<(ExitClass, Option<String>)>>>;

/// A fake box whose results download is a good run of the cavity.
fn box_with_results(tag: &str) -> PathBuf {
    let root = new_root(GROUP, tag);
    let src = root.join("results-src");
    let spec = FixtureSpec {
        end_time: 5000,
        cells: 16384,
        samples: 64,
        shift: 0.0,
    };
    write_results(&src, &spec).unwrap();
    let made = Command::new("tar")
        .arg("-C")
        .arg(&src)
        .arg("-cf")
        .arg(root.join("results.tar"))
        .arg(".")
        .status()
        .unwrap();
    assert!(made.success());
    root
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn finished(reg: &RunRegistry, id: &str) -> Report {
    wait_for("the job to finish", || {
        matches!(reg.result(id), ResultOutcome::Finished(_))
    });
    match reg.result(id) {
        ResultOutcome::Finished(r) => *r,
        other => panic!("{other:?}"),
    }
}

fn started(outcome: StartOutcome) -> String {
    match outcome {
        StartOutcome::Started { run_id } => run_id,
        other => panic!("expected Started, got {other:?}"),
    }
}

fn marker() -> Report {
    Report::from_diagnostics("marker", Vec::new(), None)
}

/// Child side. Does nothing unless the parent set the root. Each scenario asserts for itself.
#[test]
fn jobs_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let reg = RunRegistry::new();
    let req = || request(&root, RunMode::Run);
    match std::env::var("CFC_TEST_SCENARIO").unwrap().as_str() {
        "step" => {
            let id = started(reg.start_job(CAVITY, req(), Box::new(|_| marker())));
            let report = finished(&reg, &id);
            assert_eq!(report.operation, "marker");
            assert_eq!(report.class(), ExitClass::Passed);
            assert!(report.run.is_none(), "the run's own report was replaced");
            assert!(matches!(
                reg.status(&id),
                Status::Finished { exit_class: 0, .. }
            ));
        }
        "sees" => {
            let seen: Seen = Arc::default();
            let slot = seen.clone();
            let id = started(reg.start_job(
                CAVITY,
                req(),
                Box::new(move |done: Finished| {
                    let run = done.report.run.as_ref().map(|r| r.run_id.clone());
                    *slot.lock().unwrap() = Some((done.report.class(), run));
                    marker()
                }),
            ));
            finished(&reg, &id);
            let seen = seen.lock().unwrap().take().expect("the step was called");
            assert_eq!(seen.0, ExitClass::Passed);
            assert_eq!(seen.1.as_deref(), Some(id.as_str()));
        }
        "panic" => {
            let id = started(reg.start_job(
                CAVITY,
                req(),
                Box::new(|_: Finished| -> Report { panic!("the step failed") }),
            ));
            let report = finished(&reg, &id);
            assert_eq!(report.class(), ExitClass::Infrastructure);
            assert_eq!(report.operation, "run");
            assert!(report.infra.is_some());
            // The guard is gone: a new start is accepted.
            let again = started(reg.start(CAVITY, req()));
            finished(&reg, &again);
        }
        "plain" => {
            let id = started(reg.start(CAVITY, req()));
            let report = finished(&reg, &id);
            assert_eq!(report.operation, "run");
            assert_eq!(report.class(), ExitClass::Passed);
            assert_eq!(report.run.as_ref().unwrap().run_id, id);
        }
        "not_called" => {
            let called = Arc::new(AtomicBool::new(false));
            let step = || {
                let flag = called.clone();
                Box::new(move |done: Finished| {
                    flag.store(true, Ordering::SeqCst);
                    done.report
                })
            };
            match reg.start_job("{\"schema_version\": 1}", req(), step()) {
                StartOutcome::Refused(report) => {
                    assert_eq!(report.class(), ExitClass::StudyInvalid);
                }
                other => panic!("expected Refused, got {other:?}"),
            }
            match reg.start_job(CAVITY, req(), step()) {
                StartOutcome::Busy { holder, .. } => {
                    assert_eq!(holder.unwrap().run_id, "other-run-1");
                }
                other => panic!("expected Busy, got {other:?}"),
            }
            assert!(!called.load(Ordering::SeqCst), "the step was never called");
        }
        other => panic!("unknown scenario {other}"),
    }
}

fn scenario(tag: &str, which: &str, setup: impl FnOnce(&Path)) -> PathBuf {
    let root = box_with_results(tag);
    setup(&root);
    let mut c = worker(&root, "jobs_worker", "ok");
    c.env("CFC_TEST_SCENARIO", which)
        .env("CFC_TEST_RESULTS_TAR", root.join("results.tar"));
    assert_worker_ok(&c.output().unwrap());
    root
}

#[test]
fn a_job_is_finished_by_its_step() {
    let root = scenario("step", "step", |_| {});
    assert!(!lock_dir(&root).exists());
}

#[test]
fn the_step_sees_the_finished_run() {
    scenario("sees", "sees", |_| {});
}

#[test]
fn a_step_that_panics_becomes_the_unexpected_stop_report() {
    let root = scenario("panic", "panic", |_| {});
    assert!(!lock_dir(&root).exists());
}

#[test]
fn start_is_start_job_with_the_identity_step() {
    scenario("plain", "plain", |_| {});
}

#[test]
fn the_step_does_not_run_when_the_host_is_busy_or_the_study_is_refused() {
    let root = scenario("notcalled", "not_called", |r| {
        hold_lock(r, now_epoch(), 3600)
    });
    assert!(lock_dir(&root).join("holder.json").exists());
}
