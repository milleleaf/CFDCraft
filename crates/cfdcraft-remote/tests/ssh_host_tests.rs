//! The real host against a fake ssh and a fake docker: the exact command strings
//! run for real in a folder that plays the compute host's work directory.
#![cfg(unix)]

mod support;

use cfdcraft_remote::{ContainerState, Host, HostError, ReleaseOutcome, RemoteFile, SshHost};
use std::path::{Path, PathBuf};
use support::*;

const GROUP: &str = "remote-host-tests";
const ID: &str = "cfdcraft-demo-1-2";
const OTHER: &str = "cfdcraft-demo-3-4";

/// Run one scenario in a child with the fakes on its PATH.
fn scenario(tag: &str, which: &str, envs: &[(&str, &str)]) {
    let root = new_root(GROUP, tag);
    let mut c = worker(&root, "host_worker", "ok");
    c.env("CFC_TEST_SCENARIO", which);
    for (k, v) in envs {
        c.env(k, v);
    }
    assert_worker_ok(&c.output().unwrap());
}

fn connect(root: &Path) -> SshHost {
    SshHost::connect(
        profile(),
        root.join("dummy-key"),
        sockets(),
        root.join("scratch"),
        TEST_IMAGE,
    )
    .unwrap()
}

/// A host with a run started on it.
fn started(root: &Path) -> SshHost {
    let mut h = connect(root);
    h.start(ID, &root.join("case"), "demo", 1).unwrap();
    h
}

/// A run folder on the fake box that no start made, for the tests that only read
/// and write files.
fn run_folder(root: &Path) -> PathBuf {
    let dir = box_dir(root, ID);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Child side. Does nothing unless the parent set the root.
#[test]
fn host_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    match std::env::var("CFC_TEST_SCENARIO").unwrap().as_str() {
        "start" => {
            let mut h = started(&root);
            assert_eq!(h.inspect(ID).unwrap(), ContainerState::Running);
            assert!(box_dir(&root, ID).join("Allrun").is_file());
            assert!(box_dir(&root, ID).join("system/controlDict").is_file());
            assert_eq!(h.holder().unwrap().unwrap().run_id, ID);
        }
        "tail" => {
            let mut h = connect(&root);
            write(&run_folder(&root).join("log.simpleFoam"), "0123456789");
            assert_eq!(h.tail(ID, "log.simpleFoam", 4, 100).unwrap(), b"456789");
            assert_eq!(h.tail(ID, "log.simpleFoam", 0, 3).unwrap(), b"012");
            assert_eq!(h.tail(ID, "log.simpleFoam", 10, 100).unwrap(), b"");
            assert_eq!(h.tail(ID, "missing", 0, 100).unwrap(), b"");
            assert!(matches!(
                h.tail(ID, "../x", 0, 1),
                Err(HostError::Refused { .. })
            ));
        }
        "files" => {
            let mut h = connect(&root);
            let run = run_folder(&root);
            write(&run.join("postProcessing/probes/0/p"), "12345");
            write(&run.join("postProcessing/other"), "1");
            write(&run.join("0/U"), &"x".repeat(3000));
            write(&run.join("0.5/U"), &"x".repeat(3000));
            write(&run.join("processor0/0.5/U"), "x");
            write(&run.join("constant/g"), "x");
            let mut files = h.files(ID, "postProcessing").unwrap();
            files.sort_by(|a, b| a.rel.cmp(&b.rel));
            assert_eq!(
                files,
                [
                    RemoteFile {
                        rel: "postProcessing/other".into(),
                        size: 1
                    },
                    RemoteFile {
                        rel: "postProcessing/probes/0/p".into(),
                        size: 5
                    },
                ]
            );
            assert!(h.files(ID, "nothing").unwrap().is_empty());
            let mut dirs = h.time_dirs(ID).unwrap();
            dirs.sort_by(|a, b| a.name.cmp(&b.name));
            let names: Vec<_> = dirs.iter().map(|d| d.name.as_str()).collect();
            assert_eq!(names, ["0", "0.5"]);
            assert!(dirs.iter().all(|d| d.kb >= 1));
        }
        "free_kb" => {
            assert_eq!(connect(&root).free_kb().unwrap(), 123456);
        }
        "write" => {
            let mut h = connect(&root);
            let run = run_folder(&root);
            std::fs::create_dir_all(run.join("system")).unwrap();
            h.write_text(ID, "system/controlDict", "stopAt writeNow;\n")
                .unwrap();
            assert_eq!(
                h.read_text(ID, "system/controlDict").unwrap(),
                "stopAt writeNow;\n"
            );
            h.write_text(ID, "system/controlDict", "second\n").unwrap();
            assert_eq!(h.read_text(ID, "system/controlDict").unwrap(), "second\n");
            let left: Vec<_> = std::fs::read_dir(run.join("system"))
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(left, ["controlDict"]);
            assert!(matches!(
                h.read_text(ID, "system/nothing"),
                Err(HostError::Refused { .. })
            ));
            assert!(matches!(
                h.write_text(ID, "../x", "t"),
                Err(HostError::Refused { .. })
            ));
        }
        "release" => {
            let mut h = started(&root);
            assert_eq!(h.release(OTHER).unwrap(), ReleaseOutcome::NotOurs);
            assert!(h.holder().unwrap().is_some());
            assert_eq!(h.release(ID).unwrap(), ReleaseOutcome::Released);
            assert_eq!(h.holder().unwrap(), None);
            assert_eq!(h.release(ID).unwrap(), ReleaseOutcome::Absent);
        }
        "unreachable" => {
            let mut h = connect(&root);
            assert_eq!(
                h.tail(ID, "log.simpleFoam", 0, 10),
                Err(HostError::Unreachable)
            );
        }
        "unreachable_start" => {
            let mut h = connect(&root);
            assert_eq!(
                h.start(ID, &root.join("case"), "demo", 1),
                Err(HostError::Unreachable)
            );
        }
        "stop_remove" => {
            let mut h = started(&root);
            // A container this tool did not label is left alone.
            let foreign = root.join("containers/cfdcraft-foreign-1");
            write(&foreign.join("state"), "running\n");
            write(&foreign.join("exit"), "0\n");
            write(&foreign.join("oom"), "false\n");
            write(&foreign.join("labels"), "");
            assert!(matches!(
                h.stop_container("cfdcraft-foreign-1", 5),
                Err(HostError::Refused { .. })
            ));
            assert!(matches!(
                h.remove_container("cfdcraft-foreign-1"),
                Err(HostError::Refused { .. })
            ));
            assert_eq!(
                h.inspect("cfdcraft-foreign-1").unwrap(),
                ContainerState::Running
            );
            // Nothing to stop is not an error.
            h.stop_container(OTHER, 5).unwrap();

            h.stop_container(ID, 5).unwrap();
            assert!(matches!(
                h.inspect(ID).unwrap(),
                ContainerState::Exited {
                    oom_killed: false,
                    ..
                }
            ));
            h.remove_container(ID).unwrap();
            assert_eq!(h.inspect(ID).unwrap(), ContainerState::None);
        }
        "download" => {
            let mut h = connect(&root);
            let run = run_folder(&root);
            write(&run.join("0.5/U"), "u");
            write(&run.join("processor0/0.5/U"), "u");
            write(&run.join("log"), "l");
            let dest = root.join("out");
            h.download(ID, &dest).unwrap();
            assert!(dest.join("0.5/U").is_file());
            assert!(dest.join("log").is_file());
            assert!(!dest.join("processor0").exists());
            assert!(!dest.join("results.tar").exists());
        }
        "busy" => {
            let mut h = started(&root);
            assert!(matches!(
                h.start(OTHER, &root.join("case"), "demo", 1),
                Err(HostError::Busy { .. })
            ));
        }
        other => panic!("unknown scenario {other}"),
    }
}

#[test]
fn start_leaves_a_running_container_and_the_files_on_the_fake_box() {
    scenario("start", "start", &[]);
}

#[test]
fn tail_returns_bytes_after_the_offset_and_nothing_for_a_missing_file() {
    scenario("tail", "tail", &[]);
}

#[test]
fn files_and_time_dirs_report_sizes() {
    scenario("files", "files", &[]);
}

#[test]
fn free_kb_reads_the_df_override() {
    scenario("df", "free_kb", &[("CFC_TEST_DF_KB", "123456")]);
}

#[test]
fn write_text_replaces_the_file_through_a_temporary_name() {
    scenario("write", "write", &[]);
}

#[test]
fn release_is_checked_through_the_host() {
    scenario("release", "release", &[]);
}

#[test]
fn a_failed_connection_is_unreachable_not_failed() {
    scenario(
        "unreach",
        "unreachable",
        &[("CFC_TEST_FAIL_255", "log.simpleFoam")],
    );
    scenario(
        "unreach2",
        "unreachable_start",
        &[("CFC_TEST_FAIL_255", "uptime")],
    );
}

#[test]
fn stop_and_remove_a_container_by_name() {
    scenario("stop", "stop_remove", &[]);
}

#[test]
fn download_extracts_the_box_files_without_processor_folders() {
    scenario("download", "download", &[]);
}

#[test]
fn a_busy_lock_is_busy_not_unreachable() {
    scenario("busy", "busy", &[]);
}
