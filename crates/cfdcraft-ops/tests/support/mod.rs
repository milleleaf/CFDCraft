//! Shared fixtures for the tests that run a study against a fake ssh and a fake docker. Unix only:
//! the fakes are POSIX sh scripts shared with the executor's tests.
#![allow(dead_code)]

use cfdcraft_ops::{RunMode, RunRequest};
use std::{
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const FAKE_SSH: &str = include_str!("../../../cfdcraft-executor/tests/fakes/ssh");
const FAKE_DOCKER: &str = include_str!("../../../cfdcraft-executor/tests/fakes/docker");
const EXAMPLE_HOSTS: &str = include_str!("../../../cfdcraft-executor/tests/hosts.example.toml");

/// The ssh target written into every test hosts file. Leak tests look for exactly this text.
pub const SSH_TARGET: &str = "leakcheck@example-target";

pub const CAVITY: &str = include_str!("../../../cfdcraft-model/tests/fixtures/cavity_v1.json");
pub const CHANNEL: &str = include_str!("../../../cfdcraft-model/tests/fixtures/channel_v1.json");

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A fresh fake box: `bin/` with the two fakes, a stand-in key, an empty results archive, a hosts
/// file, and `box/`, the directory that plays the remote work directory.
pub fn new_root(group: &str, tag: &str) -> PathBuf {
    build_root(
        workspace_root()
            .join("target")
            .join(group)
            .join(format!("{}-{tag}", std::process::id())),
    )
}

/// The same fake box under a root named by the tag alone. A server that checks its workspace path
/// against the unix socket limit needs the shortest root there is, because the checkout path of
/// a CI runner is longer than a developer's. The tag must be unique within the test group.
pub fn new_short_root(group: &str, tag: &str) -> PathBuf {
    build_root(workspace_root().join("target").join(group).join(tag))
}

fn build_root(root: PathBuf) -> PathBuf {
    let _ = std::fs::remove_dir_all(&root);
    for d in ["bin", "empty", "box", "runs"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("dummy-key"), "not a credential").unwrap();
    std::fs::write(
        root.join("hosts.toml"),
        EXAMPLE_HOSTS.replace("USER@HOST", SSH_TARGET),
    )
    .unwrap();
    assert!(
        Command::new("tar")
            .arg("-C")
            .arg(root.join("empty"))
            .arg("-cf")
            .arg(root.join("empty.tar"))
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

pub fn lock_dir(root: &Path) -> PathBuf {
    root.join("box/.cfdcraft-job-lock")
}

/// A holder file for a lock somebody else took, so the box looks busy.
pub fn hold_lock(root: &Path, started_epoch: u64, cap_seconds: u64) {
    std::fs::create_dir_all(lock_dir(root)).unwrap();
    std::fs::write(
        lock_dir(root).join("holder.json"),
        format!(
            "{{\"run_id\":\"other-run-1\",\"container\":\"other-run-1\",\"started_epoch\":{started_epoch},\"cap_seconds\":{cap_seconds},\"client\":\"other-client\"}}\n"
        ),
    )
    .unwrap();
}

pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// PATH with the fakes first, for the child process only.
fn child_path(root: &Path) -> OsString {
    std::env::join_paths(
        std::iter::once(root.join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap()
}

/// A child that re-runs this test binary for one worker test, with the fakes on its PATH. The
/// parent's environment is never changed and no test can reach a real ssh or docker.
pub fn worker(root: &Path, worker_test: &str, docker_mode: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", worker_test, "--nocapture"])
        .env("PATH", child_path(root))
        .env("CFC_TEST_ROOT", root)
        .env("CFC_TEST_MODE", docker_mode)
        .current_dir(workspace_root());
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

fn lines(path: PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// The docker commands the fake saw.
pub fn events(root: &Path) -> Vec<String> {
    lines(root.join("events"))
}

/// The ssh calls the fake saw; an empty list means nothing reached the host.
pub fn ssh_log(root: &Path) -> Vec<String> {
    lines(root.join("ssh.log"))
}

/// A request against the fake box. The sockets go under `target/` like the command line's.
pub fn request(root: &Path, mode: RunMode) -> RunRequest {
    RunRequest::new(
        mode,
        root.join("dummy-key"),
        root.join("hosts.toml"),
        "box",
        root.join("runs"),
        workspace_root().join("target/cfdcraft-sockets"),
    )
}
