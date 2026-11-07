//! The run registry against a fake ssh and docker: start returns at once, one active job, polling,
//! and stopping the job's own container on shutdown.
#![cfg(unix)]

mod support;

use cfdcraft_ops::{ExitClass, Report, ResultOutcome, RunMode, RunRegistry, StartOutcome, Status};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use support::*;

const GROUP: &str = "ops-registry-tests";

fn release(root: &Path) {
    std::fs::write(root.join("release"), "").unwrap();
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

/// Child side. Does nothing unless the parent set the root. Each scenario asserts for itself.
#[test]
fn registry_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let reg = RunRegistry::new();
    let req = || request(&root, RunMode::Run);
    match std::env::var("CFC_TEST_SCENARIO").unwrap().as_str() {
        "poll" => {
            let t = Instant::now();
            let id = started(reg.start(CAVITY, req()));
            assert!(
                t.elapsed() < Duration::from_secs(15),
                "start waited for the job"
            );
            assert!(
                id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "{id}"
            );
            let first = match reg.status(&id) {
                Status::Running {
                    mode,
                    elapsed_seconds,
                } => {
                    assert_eq!(mode, RunMode::Run);
                    elapsed_seconds
                }
                other => panic!("{other:?}"),
            };
            assert!(matches!(reg.result(&id), ResultOutcome::Running));
            std::thread::sleep(Duration::from_millis(1300));
            match reg.status(&id) {
                Status::Running {
                    elapsed_seconds, ..
                } => assert!(elapsed_seconds > first),
                other => panic!("{other:?}"),
            }
            release(&root);
            let report = finished(&reg, &id);
            assert_eq!(report.class(), ExitClass::GateFailed);
            assert_eq!(report.run.as_ref().unwrap().run_id, id);
            assert!(matches!(
                reg.status(&id),
                Status::Finished { exit_class: 1, .. }
            ));
        }
        "race" => {
            let id = started(reg.start(CAVITY, req()));
            match reg.start(CAVITY, req()) {
                StartOutcome::Busy {
                    holder,
                    retry_after_seconds,
                    ..
                } => {
                    assert_eq!(holder.unwrap().run_id, id);
                    assert!((30..=600).contains(&retry_after_seconds));
                }
                other => panic!("expected Busy, got {other:?}"),
            }
            release(&root);
            finished(&reg, &id);
            // The guard is gone once the job is: a new start is accepted.
            std::fs::remove_file(root.join("release")).unwrap();
            let again = started(reg.start(CAVITY, req()));
            release(&root);
            finished(&reg, &again);
        }
        "foreign" => match reg.start(CAVITY, req()) {
            StartOutcome::Busy {
                holder,
                retry_after_seconds,
                stale,
            } => {
                let expect = std::env::var("CFC_TEST_EXPECT").unwrap();
                match expect.as_str() {
                    "live" => {
                        assert_eq!(holder.unwrap().run_id, "other-run-1");
                        assert!((30..=600).contains(&retry_after_seconds));
                        assert!(!stale);
                    }
                    "stale" => {
                        assert_eq!(holder.unwrap().run_id, "other-run-1");
                        assert_eq!(retry_after_seconds, 600);
                        assert!(stale);
                    }
                    _ => {
                        assert!(holder.is_none());
                        assert_eq!(retry_after_seconds, 60);
                    }
                }
            }
            other => panic!("expected Busy, got {other:?}"),
        },
        "refused" => {
            match reg.start("{\"schema_version\": 1}", req()) {
                StartOutcome::Refused(report) => {
                    assert_eq!(report.class(), ExitClass::StudyInvalid);
                }
                other => panic!("expected Refused, got {other:?}"),
            }
            let id = started(reg.start(CAVITY, req()));
            release(&root);
            finished(&reg, &id);
        }
        "shutdown" => {
            let id = started(reg.start(CAVITY, req()));
            wait_for("the container to start", || {
                events(&root).iter().any(|e| e == "run")
            });
            assert!(reg.shutdown(Duration::from_secs(20)));
            let report = finished(&reg, &id);
            assert_eq!(report.class(), ExitClass::Infrastructure);
            assert_eq!(report.infra.as_ref().unwrap().code, "CFD-INFRA-006");
        }
        "unknown" => {
            assert_eq!(reg.status("cfdcraft-nothing-1"), Status::NotFound);
            assert!(matches!(
                reg.result("cfdcraft-nothing-1"),
                ResultOutcome::NotFound
            ));
            assert_eq!(reg.status("../../etc"), Status::NotFound);
            assert!(matches!(reg.result(""), ResultOutcome::NotFound));
        }
        other => panic!("unknown scenario {other}"),
    }
}

fn scenario(tag: &str, which: &str, expect: &str, setup: impl FnOnce(&Path)) -> PathBuf {
    let root = new_root(GROUP, tag);
    setup(&root);
    let mut c = worker(&root, "registry_worker", "hold");
    c.env("CFC_TEST_SCENARIO", which)
        .env("CFC_TEST_EXPECT", expect);
    assert_worker_ok(&c.output().unwrap());
    root
}

#[test]
fn start_returns_at_once_and_the_job_can_be_polled() {
    let root = scenario("poll", "poll", "", |_| {});
    assert!(!lock_dir(&root).exists());
    assert!(!root.join("running").exists());
}

#[test]
fn a_second_start_in_one_process_is_busy_before_any_tunnel_opens() {
    let root = scenario("race", "race", "", |_| {});
    let log = ssh_log(&root);
    let takes = log.iter().filter(|l| l.contains("mkdir \"$d\"")).count();
    // Two accepted starts took the lock; the refused one never reached the host.
    assert_eq!(takes, 2, "{log:?}");
    assert_eq!(log.iter().filter(|l| *l == "tunnel").count(), 2, "{log:?}");
    assert_eq!(events(&root).iter().filter(|e| *e == "run").count(), 2);
}

#[test]
fn a_lock_held_elsewhere_is_busy_with_the_holder_and_is_never_removed() {
    let root = scenario("live", "foreign", "live", |r| {
        hold_lock(r, now_epoch(), 3600)
    });
    assert!(lock_dir(&root).join("holder.json").exists());

    let root = scenario("stale", "foreign", "stale", |r| {
        hold_lock(r, now_epoch() - 10_000, 100)
    });
    assert!(
        lock_dir(&root).join("holder.json").exists(),
        "a stale lock is left alone"
    );

    let root = scenario("nohold", "foreign", "none", |r| {
        std::fs::create_dir_all(lock_dir(r)).unwrap()
    });
    assert!(lock_dir(&root).exists());
}

#[test]
fn a_refused_study_releases_the_in_process_guard() {
    let root = scenario("refused", "refused", "", |_| {});
    assert!(!lock_dir(&root).exists());
}

#[test]
fn shutdown_stops_the_owned_container_and_releases_the_lock() {
    let root = scenario("shutdown", "shutdown", "", |_| {});
    let events = events(&root);
    assert!(events.iter().any(|e| e == "stop"), "{events:?}");
    assert!(events.iter().any(|e| e == "rm"), "{events:?}");
    assert!(!lock_dir(&root).exists(), "no lock directory left");
    assert!(!root.join("running").exists(), "no container left");
}

#[test]
fn unknown_run_ids_are_not_found() {
    scenario("unknown", "unknown", "", |_| {});
}
