//! A job against a fake ssh and docker: cleanup order, the lock, and the
//! difference between a busy host and a failed one.
#![cfg(unix)]

mod support;

use cfdcraft_executor::{Holder, RunError, hosts::PreflightError};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use support::*;

const GROUP: &str = "executor-failure-tests";

/// Run one job in a child process and return its root and the one-line
/// description of the outcome. `setup` prepares the fake box first.
fn scenario(
    tag: &str,
    mode: &str,
    fail_255: Option<&str>,
    setup: impl FnOnce(&Path),
) -> (PathBuf, String) {
    let root = new_root(GROUP, tag);
    setup(&root);
    let mut c = worker(&root, "fake_box_worker", mode);
    if let Some(f) = fail_255 {
        c.env("CFC_TEST_FAIL_255", f);
    }
    assert_worker_ok(&c.output().unwrap());
    let outcome = std::fs::read_to_string(root.join("outcome")).unwrap();
    (root, outcome)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn fake_box_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let outcome = run_job(&root, "regression", &root.join("results"));
    std::fs::write(root.join("outcome"), describe(&outcome)).unwrap();
}

#[test]
fn free_lock_is_taken_used_and_removed() {
    let (root, outcome) = scenario("free", "ok", None, |_| {});
    assert_eq!(outcome, "ok exit=Some(0)");
    assert!(!lock_dir(&root).exists());
    assert_eq!(events(&root).iter().filter(|e| *e == "run").count(), 1);
}

#[test]
fn nonzero_docker_client_stops_owned_container_before_unlock() {
    let (root, outcome) = scenario("cleanup-ok", "cleanup-succeeds", None, |_| {});
    assert_eq!(outcome, "ok exit=Some(1)");
    let events = events(&root);
    let run = events.iter().position(|x| x == "run").unwrap();
    assert_eq!(&events[run..run + 4], &["run", "stop", "rm", "ps"]);
    assert!(!root.join("running").exists());
    assert!(!lock_dir(&root).exists());
    assert_eq!(events.last().map(String::as_str), Some("unlock"));
}

#[test]
fn unconfirmed_container_cleanup_retains_project_lock() {
    let (root, outcome) = scenario("cleanup-fails", "cleanup-fails", None, |_| {});
    assert!(
        outcome.starts_with("setup ") && outcome.contains("retained project lock"),
        "{outcome}"
    );
    let events = events(&root);
    let run = events.iter().position(|x| x == "run").unwrap();
    assert_eq!(&events[run..run + 4], &["run", "stop", "rm", "ps"]);
    assert!(lock_dir(&root).exists());
    assert!(root.join("running").exists());
    assert!(!events.iter().any(|e| e == "unlock"));
}

#[test]
fn held_lock_with_a_holder_is_busy_and_untouched() {
    let holder = Holder::new(
        "cfdcraft-other-1-2",
        "cfdcraft-other-1-2",
        now(),
        600,
        "test-client",
    )
    .unwrap();
    let json = holder.to_json();
    let (root, outcome) = scenario("held", "ok", None, |root| {
        std::fs::create_dir(lock_dir(root)).unwrap();
        std::fs::write(lock_dir(root).join("holder.json"), format!("{json}\n")).unwrap();
    });
    assert_eq!(outcome, format!("busy holder={json} stale=false"));
    assert!(!events(&root).iter().any(|e| e == "run"));
    assert_eq!(
        std::fs::read_to_string(lock_dir(&root).join("holder.json")).unwrap(),
        format!("{json}\n")
    );
}

#[test]
fn held_lock_with_no_holder_is_busy_without_a_name() {
    let (root, outcome) = scenario("no-holder", "ok", None, |root| {
        std::fs::create_dir(lock_dir(root)).unwrap();
    });
    assert_eq!(outcome, "busy holder=none stale=false");
    assert!(!events(&root).iter().any(|e| e == "run"));
    assert!(lock_dir(&root).exists());
}

#[test]
fn an_old_holder_is_reported_stale_and_not_broken() {
    let holder = Holder::new(
        "cfdcraft-old-1-2",
        "cfdcraft-old-1-2",
        1_000,
        600,
        "test-client",
    )
    .unwrap();
    let json = holder.to_json();
    let (root, outcome) = scenario("stale", "ok", None, |root| {
        std::fs::create_dir(lock_dir(root)).unwrap();
        std::fs::write(lock_dir(root).join("holder.json"), format!("{json}\n")).unwrap();
    });
    assert_eq!(outcome, format!("busy holder={json} stale=true"));
    assert!(lock_dir(&root).join("holder.json").exists());
}

#[test]
fn ssh_status_255_on_the_lock_is_an_ssh_failure_not_busy() {
    let (root, outcome) = scenario("ssh-255", "ok", Some("job-lock"), |_| {});
    assert!(outcome.starts_with("ssh "), "{outcome}");
    assert!(outcome.contains("simulated connection failure"));
    assert!(!events(&root).iter().any(|e| e == "run"));
}

#[test]
fn a_running_cfdcraft_container_is_busy_class() {
    let (root, outcome) = scenario("container", "ok", None, |root| {
        std::fs::write(root.join("running"), "cfdcraft-other-1-2\n").unwrap();
    });
    assert!(outcome.starts_with("busy-class "), "{outcome}");
    assert!(!events(&root).iter().any(|e| e == "run"));
    assert!(!lock_dir(&root).exists());
}

#[test]
fn only_try_later_outcomes_are_busy() {
    assert!(
        RunError::Busy {
            holder: None,
            stale: false
        }
        .is_busy()
    );
    assert!(
        RunError::Preflight(PreflightError::ExistingCfdcraftContainers { names: vec![] }).is_busy()
    );
    assert!(
        RunError::Preflight(PreflightError::Busy {
            load_1min: 9.0,
            threshold: 8.0
        })
        .is_busy()
    );
    assert!(!RunError::Ssh { detail: "x".into() }.is_busy());
    assert!(!RunError::Setup("x".into()).is_busy());
    assert!(!RunError::TimedOut.is_busy());
    assert!(!RunError::Preflight(PreflightError::InvalidLoad).is_busy());
}
