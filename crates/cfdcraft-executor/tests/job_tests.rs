//! `Job::begin` and `Job::execute` against a fake ssh and docker: the split,
//! cancellation, and per-job socket directories.
#![cfg(unix)]

mod support;

use cfdcraft_executor::{Job, RunConfig, RunError, docker::RunSpec, tunnel::Tunnel};
use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use support::*;

const GROUP: &str = "executor-job-tests";

fn scenario(tag: &str, mode: &str, which: &str, setup: impl FnOnce(&Path)) -> (PathBuf, String) {
    let root = new_root(GROUP, tag);
    setup(&root);
    let mut c = worker(&root, "job_worker", mode);
    c.env("CFC_TEST_SCENARIO", which);
    assert_worker_ok(&c.output().unwrap());
    let outcome = std::fs::read_to_string(root.join("outcome")).unwrap();
    (root, outcome)
}

fn config() -> RunConfig {
    RunConfig::from_cwd().unwrap()
}

/// Child side. Does nothing unless the parent set the root.
#[test]
fn job_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let which = std::env::var("CFC_TEST_SCENARIO").unwrap();
    let key = root.join("dummy-key");
    let spec = RunSpec::allrun(TEST_IMAGE).unwrap();
    let text = match which.as_str() {
        "complete" => {
            let job = Job::begin(&profile(), &key, config(), "split").unwrap();
            let id = job.run_id().to_string();
            assert_eq!(job.run_id(), id, "the run id is stable");
            // Nothing has run yet: begin only took the lock.
            assert!(events(&root).iter().all(|e| e != "run"));
            assert!(lock_dir(&root).exists());
            let out = job
                .execute(&root.join("case"), &root.join("results"), &spec)
                .unwrap();
            assert_eq!(out.container_name, id);
            format!("ok exit={:?} id={id}", out.exit_code)
        }
        "busy" => match Job::begin(&profile(), &key, config(), "split") {
            Err(RunError::Busy { holder, stale }) => {
                format!("busy holder={} stale={stale}", holder.is_some())
            }
            Err(e) => format!("other {e:?}"),
            Ok(_) => "started".into(),
        },
        "cancel" => {
            let cfg = config();
            let flag = cfg.cancel_flag();
            let watched = root.clone();
            // Cancel once the fake `docker run` is up and holding.
            std::thread::spawn(move || {
                let start = Instant::now();
                while !watched.join("running").exists() && start.elapsed() < Duration::from_secs(20)
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                flag.store(true, Ordering::SeqCst);
            });
            let job = Job::begin(&profile(), &key, cfg, "split").unwrap();
            match job.execute(&root.join("case"), &root.join("results"), &spec) {
                Err(RunError::Cancelled) => "cancelled".into(),
                Err(e) => format!("other {e:?}"),
                Ok(_) => "completed".into(),
            }
        }
        "dropped" => {
            drop(Job::begin(&profile(), &key, config(), "split").unwrap());
            "dropped".into()
        }
        "tunnels" => {
            let p = profile();
            let base = config().socket_dir;
            let a = Tunnel::open(&p, &key, &base, "aaaa").unwrap();
            let b = Tunnel::open(&p, &key, &base, "bbbb").unwrap();
            let (da, db) = (
                a.socket_path.parent().unwrap().to_path_buf(),
                b.socket_path.parent().unwrap().to_path_buf(),
            );
            assert_ne!(da, db);
            assert!(da.is_dir() && db.is_dir());
            drop(a);
            assert!(!da.exists(), "a tunnel removes its own directory");
            assert!(db.is_dir(), "and leaves the other one alone");
            drop(b);
            assert!(!db.exists());
            "tunnels".into()
        }
        other => panic!("unknown scenario {other}"),
    };
    std::fs::write(root.join("outcome"), text).unwrap();
}

#[test]
fn begin_then_execute_runs_and_names_the_container_after_the_run_id() {
    let (root, outcome) = scenario("complete", "ok", "complete", |_| {});
    assert!(
        outcome.starts_with("ok exit=Some(0) id=cfdcraft-split-"),
        "{outcome}"
    );
    let id = outcome.rsplit("id=").next().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("names")).unwrap().trim(),
        id
    );
    assert!(!lock_dir(&root).exists());
    assert_eq!(events(&root).iter().filter(|e| *e == "run").count(), 1);
}

#[test]
fn begin_reports_busy_before_any_docker_run() {
    let (root, outcome) = scenario("busy", "ok", "busy", |root| {
        std::fs::create_dir(lock_dir(root)).unwrap();
    });
    assert_eq!(outcome, "busy holder=false stale=false");
    assert!(!events(&root).iter().any(|e| e == "run"));
    assert!(lock_dir(&root).exists(), "someone else's lock is untouched");
}

#[test]
fn dropping_a_job_that_never_executed_releases_the_lock() {
    let (root, outcome) = scenario("dropped", "ok", "dropped", |_| {});
    assert_eq!(outcome, "dropped");
    assert!(!lock_dir(&root).exists());
    assert!(!events(&root).iter().any(|e| e == "run"));
}

#[test]
fn cancel_stops_and_removes_the_container_before_the_lock_is_released() {
    let (root, outcome) = scenario("cancel", "hold", "cancel", |_| {});
    assert_eq!(outcome, "cancelled");
    let events = events(&root);
    let run = events.iter().position(|x| x == "run").unwrap();
    assert_eq!(&events[run..run + 4], &["run", "stop", "rm", "ps"]);
    assert_eq!(events.last().map(String::as_str), Some("unlock"));
    assert!(!root.join("running").exists());
    assert!(!lock_dir(&root).exists());
}

#[test]
fn tunnels_with_different_tags_use_their_own_directories() {
    let (_, outcome) = scenario("tunnels", "ok", "tunnels", |_| {});
    assert_eq!(outcome, "tunnels");
}

#[test]
fn an_overlong_socket_path_is_refused_without_naming_it() {
    let root = new_root(GROUP, "long");
    let base = root.join("a".repeat(90));
    let err = match Tunnel::open(&profile(), &root.join("dummy-key"), &base, "tag") {
        Err(e) => e,
        Ok(_) => panic!("the path is over the limit"),
    };
    assert!(matches!(err, RunError::SocketPathTooLong));
    let shown = err.to_string();
    assert!(
        shown.contains("too long") && shown.contains("100"),
        "{shown}"
    );
    assert!(!shown.contains(&"a".repeat(90)), "{shown}");
    let _ = std::fs::remove_dir_all(&root);
}
