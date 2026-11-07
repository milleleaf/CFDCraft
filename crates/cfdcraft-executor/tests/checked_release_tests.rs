//! The checked release and the holder read: only the run that holds the lock can
//! give it back, and a failed connection is never mistaken for "not ours".
#![cfg(unix)]

mod support;

use cfdcraft_executor::{
    Holder, ReleaseOutcome, RunError,
    lock::{
        ABSENT_EXIT, NOT_HOLDER_EXIT, read_holder, release_checked, release_checked_command,
        release_command,
    },
};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use support::*;

const GROUP: &str = "executor-checked-release-tests";
const ID: &str = "cfdcraft-demo-1-2";
const LOCK: &str = "/data/cfdcraft/work/.cfdcraft-job-lock";

/// A lock held by `id`, made directly in the fake box folder.
fn plant(root: &Path, id: &str) -> PathBuf {
    std::fs::create_dir(lock_dir(root)).unwrap();
    let file = lock_dir(root).join("holder.json");
    let holder = Holder::new(id, id, 1_000, 600, "test-client").unwrap();
    std::fs::write(&file, format!("{}\n", holder.to_json())).unwrap();
    file
}

/// Run one scenario in a child with the fakes on its PATH; return its outcome.
fn scenario(root: &Path, which: &str, arg: &str, envs: &[(&str, &str)]) -> String {
    let mut c = worker(root, "release_worker", "ok");
    c.env("CFC_TEST_SCENARIO", which).env("CFC_TEST_ARG", arg);
    for (k, v) in envs {
        c.env(k, v);
    }
    assert_worker_ok(&c.output().unwrap());
    std::fs::read_to_string(root.join("outcome")).unwrap()
}

fn release_text(r: Result<ReleaseOutcome, RunError>) -> String {
    match r {
        Ok(ReleaseOutcome::Released) => "released".into(),
        Ok(ReleaseOutcome::NotOurs) => "not-ours".into(),
        Ok(ReleaseOutcome::Absent) => "absent".into(),
        Err(RunError::Ssh { .. }) => "ssh".into(),
        Err(RunError::Setup(_)) => "setup".into(),
        Err(e) => format!("other {e:?}"),
    }
}

/// Child side. Does nothing unless the parent set the root.
#[test]
fn release_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let which = std::env::var("CFC_TEST_SCENARIO").unwrap();
    let arg = std::env::var("CFC_TEST_ARG").unwrap_or_default();
    let key = root.join("dummy-key");
    let p = profile();
    let text = match which.as_str() {
        "release" => release_text(release_checked(&p, &key, LOCK, &arg)),
        "holder" => match read_holder(&p, &key, LOCK) {
            Ok(Some(h)) => h.run_id,
            Ok(None) => "none".into(),
            Err(RunError::Ssh { .. }) => "ssh".into(),
            Err(e) => format!("other {e:?}"),
        },
        "start_then_release" => {
            start_detached_job(&root, ID, "demo").unwrap();
            release_text(release_checked(&p, &key, LOCK, ID))
        }
        other => panic!("unknown scenario {other}"),
    };
    std::fs::write(root.join("outcome"), text).unwrap();
}

#[test]
fn the_holder_releases_its_own_lock() {
    let root = new_real_root(GROUP, "own");
    assert_eq!(scenario(&root, "start_then_release", "", &[]), "released");
    assert!(!lock_dir(&root).exists());
}

#[test]
fn another_id_leaves_the_lock_alone() {
    let root = new_root(GROUP, "other");
    let file = plant(&root, ID);
    let before = std::fs::read(&file).unwrap();
    assert_eq!(
        scenario(&root, "release", "cfdcraft-other-9-9", &[]),
        "not-ours"
    );
    assert!(lock_dir(&root).is_dir());
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

#[test]
fn no_lock_reports_absent() {
    let root = new_root(GROUP, "absent");
    assert_eq!(scenario(&root, "release", ID, &[]), "absent");
}

#[test]
fn an_id_with_a_quote_or_a_slash_is_refused_before_ssh() {
    let long = "x".repeat(129);
    for bad in ["a'b", "a/b", "", long.as_str()] {
        assert!(release_checked_command(LOCK, bad).is_err(), "{bad:?}");
    }
    assert!(release_checked_command("/a b/lock", ID).is_err());
    assert!(release_checked_command("/a'b", ID).is_err());
    let root = new_root(GROUP, "refused");
    plant(&root, ID);
    assert_eq!(scenario(&root, "release", "a'b", &[]), "setup");
    assert!(!root.join("ssh.log").exists(), "nothing reached the host");
    assert!(lock_dir(&root).is_dir());
}

#[test]
fn read_holder_returns_the_holder_or_none() {
    let root = new_root(GROUP, "read");
    assert_eq!(scenario(&root, "holder", "", &[]), "none");
    let file = plant(&root, ID);
    assert_eq!(scenario(&root, "holder", "", &[]), ID);
    std::fs::write(&file, "{ not a holder").unwrap();
    assert_eq!(scenario(&root, "holder", "", &[]), "none");
}

#[test]
fn a_failed_connection_is_an_ssh_error_not_not_ours() {
    let root = new_root(GROUP, "fail255");
    plant(&root, ID);
    let down = [("CFC_TEST_FAIL_255", "job-lock")];
    assert_eq!(scenario(&root, "release", ID, &down), "ssh");
    assert_eq!(scenario(&root, "holder", "", &down), "ssh");
    assert!(lock_dir(&root).is_dir(), "nothing was removed");
}

#[test]
fn the_unchecked_release_still_removes_any_lock() {
    let root = new_root(GROUP, "unchecked");
    plant(&root, ID);
    let path = format!("{}/.cfdcraft-job-lock", root.join("box").display());
    assert_eq!(
        release_command(&path).unwrap(),
        format!("rm -f '{path}/holder.json'; rmdir '{path}'")
    );
    let status = Command::new("sh")
        .args(["-c", &release_command(&path).unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(!lock_dir(&root).exists());
}

#[test]
fn an_id_that_is_a_prefix_of_the_holders_is_not_the_holder() {
    let root = new_root(GROUP, "prefix");
    let longer = format!("{ID}2");
    let file = plant(&root, &longer);
    let path = format!("{}/.cfdcraft-job-lock", root.join("box").display());
    let run = |id: &str| {
        Command::new("sh")
            .args(["-c", &release_checked_command(&path, id).unwrap()])
            .status()
            .unwrap()
            .code()
    };
    assert_eq!(run(ID), Some(NOT_HOLDER_EXIT));
    assert!(file.is_file(), "the longer id still holds the lock");
    assert_eq!(run(&longer), Some(0));
    assert!(!lock_dir(&root).exists());
    assert_eq!(run(&longer), Some(ABSENT_EXIT));
}
