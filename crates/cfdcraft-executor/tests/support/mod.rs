//! Shared fixtures for the tests that run a job against a fake ssh and a
//! fake docker. Unix only: the fakes are POSIX sh scripts.
#![allow(dead_code)]

use cfdcraft_executor::{
    HostProfile, Job, RunConfig, RunError, RunOutcome, Started, docker::RunSpec,
};
use std::{
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub const FAKE_SSH: &str = include_str!("../fakes/ssh");
pub const FAKE_DOCKER: &str = include_str!("../fakes/docker");

/// A well-formed digest reference that names no real image.
pub const TEST_IMAGE: &str =
    "example/openfoam@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn profile() -> HostProfile {
    HostProfile::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hosts.example.toml"),
        "box",
    )
    .unwrap()
}

/// A fresh fake box: `bin/` with the two fakes, an empty case, a stand-in key,
/// an empty results archive and `box/`, the directory that plays the remote
/// work directory (so the lock is a real directory inside it).
pub fn new_root(group: &str, tag: &str) -> PathBuf {
    let root = workspace_root()
        .join("target")
        .join(group)
        .join(format!("{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in ["bin", "case", "empty", "box"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("dummy-key"), "not a credential").unwrap();
    let archive = root.join("empty.tar");
    assert!(
        Command::new("tar")
            .arg("-C")
            .arg(root.join("empty"))
            .arg("-cf")
            .arg(&archive)
            .arg(".")
            .status()
            .unwrap()
            .success()
    );
    for (name, script) in [("ssh", FAKE_SSH), ("docker", FAKE_DOCKER)] {
        let path = root.join("bin").join(name);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    root
}

/// Like `new_root`, and the fake ssh really makes folders and unpacks uploads in
/// `box/` (a marker file that `worker` turns into `CFC_TEST_BOX_REAL=1`).
pub fn new_real_root(group: &str, tag: &str) -> PathBuf {
    let root = new_root(group, tag);
    std::fs::write(root.join("box-real"), "1").unwrap();
    root
}

pub fn lock_dir(root: &Path) -> PathBuf {
    root.join("box/.cfdcraft-job-lock")
}

/// PATH with the fakes first, for the child process only.
pub fn child_path(root: &Path) -> OsString {
    std::env::join_paths(
        std::iter::once(root.join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap()
}

/// A child that re-runs this test binary for one worker test, with the fakes on
/// its PATH. Concurrent tests never mutate the parent's environment or reach
/// real ssh or docker.
pub fn worker(root: &Path, worker_test: &str, mode: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", worker_test, "--nocapture"])
        .env("PATH", child_path(root))
        .env("CFC_TEST_ROOT", root)
        .env("CFC_TEST_MODE", mode)
        .current_dir(workspace_root());
    if root.join("box-real").exists() {
        c.env("CFC_TEST_BOX_REAL", "1");
    }
    c
}

pub fn assert_worker_ok(output: &Output) {
    assert!(
        output.status.success(),
        "worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn events(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join("events"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Begin a job on the fake box and execute it with the test image. Errors come
/// from `begin` (busy, ssh, preflight) or from `execute` (cleanup unconfirmed).
pub fn run_job(root: &Path, label: &str, results: &Path) -> Result<RunOutcome, RunError> {
    Job::begin(
        &profile(),
        &root.join("dummy-key"),
        RunConfig::from_cwd().unwrap(),
        label,
    )?
    .execute(
        &root.join("case"),
        results,
        &RunSpec::allrun(TEST_IMAGE).unwrap(),
    )
}

/// One line saying how a run ended, for a parent test to assert on.
pub fn describe(r: &Result<RunOutcome, RunError>) -> String {
    match r {
        Ok(o) => format!("ok exit={:?}", o.exit_code),
        Err(RunError::Busy { holder, stale }) => format!(
            "busy holder={} stale={stale}",
            holder.as_ref().map_or("none".into(), |h| h.to_json())
        ),
        Err(RunError::Ssh { detail }) => format!("ssh {detail}"),
        Err(e) if e.is_busy() => format!("busy-class {e}"),
        Err(RunError::Setup(m)) => format!("setup {m}"),
        Err(e) => format!("other {e}"),
    }
}

/// The state, exit status and out-of-memory flag the fake docker holds for a
/// detached container, or `None` when there is no such container.
pub fn container_state(root: &Path, name: &str) -> Option<(String, i32, bool)> {
    let dir = root.join("containers").join(name);
    let read = |f: &str| {
        std::fs::read_to_string(dir.join(f))
            .ok()
            .map(|t| t.trim().to_string())
    };
    Some((
        read("state")?,
        read("exit")?.parse().ok()?,
        read("oom")? == "true",
    ))
}

/// The `key=value` labels the fake docker recorded for a detached container.
pub fn container_labels(root: &Path, name: &str) -> Vec<String> {
    std::fs::read_to_string(root.join("containers").join(name).join("labels"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Begin a job under the chosen run id and start it detached, with one process.
pub fn start_detached_job(root: &Path, id: &str, doc: &str) -> Result<Started, RunError> {
    Job::begin_as(
        &profile(),
        &root.join("dummy-key"),
        RunConfig::from_cwd().unwrap(),
        id,
    )?
    .start_detached(
        &root.join("case"),
        &root.join("scratch"),
        &RunSpec::allrun_timed(TEST_IMAGE, 600).unwrap(),
        doc,
        1,
    )
}
