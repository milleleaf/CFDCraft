//! A detached start against a fake ssh and docker: the container and the lock
//! are still there when the starting client is gone.
#![cfg(unix)]

mod support;

use cfdcraft_executor::{Job, RunConfig, RunError, lock::parse_busy_output};
use std::path::{Path, PathBuf};
use support::*;

const GROUP: &str = "executor-detached-tests";
const ID: &str = "cfdcraft-demo-1-2";

/// Run one scenario in a child with the fakes on its PATH and return the root.
fn scenario(tag: &str, mode: &str, which: &str, envs: &[(&str, &str)]) -> PathBuf {
    let root = new_real_root(GROUP, tag);
    std::fs::write(root.join("case/Allrun"), "#!/bin/sh\n").unwrap();
    let mut c = worker(&root, "detached_worker", mode);
    c.env("CFC_TEST_SCENARIO", which);
    for (k, v) in envs {
        c.env(k, v);
    }
    assert_worker_ok(&c.output().unwrap());
    root
}

fn holder_run_id(root: &Path) -> String {
    let text = std::fs::read_to_string(lock_dir(root).join("holder.json")).unwrap();
    parse_busy_output(&text).unwrap().run_id
}

/// Child side. Does nothing unless the parent set the root.
#[test]
fn detached_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let which = std::env::var("CFC_TEST_SCENARIO").unwrap();
    let key = root.join("dummy-key");
    let config = || RunConfig::from_cwd().unwrap();
    match which.as_str() {
        "start" => {
            let started = start_detached_job(&root, ID, "demo").unwrap();
            assert_eq!(started.run_id, ID);
            assert_eq!(started.remote_dir, format!("/data/cfdcraft/work/{ID}"));
        }
        "busy" => {
            start_detached_job(&root, ID, "demo").unwrap();
            // The container is running: the host reports itself busy.
            match Job::begin(&profile(), &key, config(), "other") {
                Err(e) => assert!(e.is_busy(), "{e:?}"),
                Ok(_) => panic!("a second job started"),
            }
        }
        "orphan" => {
            start_detached_job(&root, ID, "demo").unwrap();
            // Take the container away and leave the lock: the holder is named.
            std::fs::remove_dir_all(root.join("containers").join(ID)).unwrap();
            match Job::begin(&profile(), &key, config(), "other") {
                Err(RunError::Busy { holder, .. }) => {
                    assert_eq!(holder.expect("a holder").run_id, ID);
                }
                Err(e) => panic!("{e:?}"),
                Ok(_) => panic!("a second job started"),
            }
        }
        "docker_fails" => match start_detached_job(&root, ID, "demo") {
            Err(RunError::Docker { .. }) => {}
            other => panic!("{other:?}"),
        },
        "ids" => {
            for bad in ["cfdcraft-a'b", "cfdcraft-a/b", "other-1", ""] {
                let r = Job::begin_as(&profile(), &key, config(), bad);
                assert!(matches!(r, Err(RunError::Setup(_))), "{bad:?}");
            }
            let own = Job::begin_as(&profile(), &key, config(), ID).unwrap();
            assert_eq!(own.run_id(), ID);
            drop(own);
            let made = Job::begin(&profile(), &key, config(), "demo").unwrap();
            assert!(
                made.run_id().starts_with("cfdcraft-demo-"),
                "{}",
                made.run_id()
            );
        }
        "attached" => {
            let out = run_job(&root, "demo", &root.join("results")).unwrap();
            assert_eq!(out.exit_code, Some(0));
        }
        other => panic!("unknown scenario {other}"),
    }
}

#[test]
fn a_detached_start_keeps_the_container_and_the_lock() {
    let root = scenario("start", "ok", "start", &[]);
    let (state, _, oom) = container_state(&root, ID).expect("the container exists");
    assert_eq!(state, "running");
    assert!(!oom);
    let labels = container_labels(&root, ID);
    assert!(labels.contains(&format!("cfdcraft.run={ID}")), "{labels:?}");
    assert!(
        labels.contains(&"cfdcraft.doc=demo".to_string()),
        "{labels:?}"
    );
    assert!(lock_dir(&root).is_dir());
    assert_eq!(holder_run_id(&root), ID);
    let events = events(&root);
    assert!(
        events.iter().all(|e| e != "stop" && e != "rm"),
        "{events:?}"
    );
    assert!(root.join("box").join(ID).join("Allrun").is_file());
    assert!(!root.join("scratch/input.tar").exists());
}

#[test]
fn while_a_detached_run_lives_a_second_job_is_busy() {
    let root = scenario("busy", "ok", "busy", &[]);
    let (state, _, _) = container_state(&root, ID).unwrap();
    assert_eq!(state, "running");
    assert_eq!(holder_run_id(&root), ID);
}

#[test]
fn a_held_lock_with_no_container_reports_the_holder() {
    let root = scenario("orphan", "ok", "orphan", &[]);
    assert!(container_state(&root, ID).is_none());
    assert_eq!(holder_run_id(&root), ID);
}

#[test]
fn a_failed_docker_run_gives_the_lock_back() {
    let root = scenario(
        "fails",
        "ok",
        "docker_fails",
        &[("CFC_TEST_DOCKER_FAIL", "run")],
    );
    assert!(!lock_dir(&root).exists());
    assert!(container_state(&root, ID).is_none());
}

#[test]
fn begin_as_takes_the_id_it_is_given_and_begin_still_makes_its_own() {
    scenario("ids", "ok", "ids", &[]);
}

#[test]
fn the_attached_path_still_removes_its_container() {
    let root = scenario("attached", "ok", "attached", &[]);
    assert!(!root.join("running").exists());
    assert!(
        !root.join("containers").exists()
            || std::fs::read_dir(root.join("containers")).unwrap().count() == 0
    );
    assert!(!lock_dir(&root).exists());
}
