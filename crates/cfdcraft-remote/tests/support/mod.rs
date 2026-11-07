//! Fixtures for the tests that drive the real host against a fake ssh and a fake
//! docker. Unix only: the fakes are POSIX sh scripts. The two fakes are the ones
//! the executor tests use; they are included by relative path.
#![allow(dead_code)]

use cfdcraft_executor::HostProfile;
use std::{
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub const FAKE_SSH: &str = include_str!("../../../cfdcraft-executor/tests/fakes/ssh");
pub const FAKE_DOCKER: &str = include_str!("../../../cfdcraft-executor/tests/fakes/docker");

/// Put in front of the executor's fake ssh: a results download (`tar -C`) really
/// archives the box folder, so the exclusion of the per-process folders is run and
/// not only built. Everything else goes to the executor's fake unchanged.
pub const SSH_WRAPPER: &str = r##"#!/bin/sh
for last do :; done
case "$last" in
    'tar -C'*)
        if [ -z "${CFC_TEST_RESULTS_TAR:-}" ]; then
            printf '%s\n' "$last" >> "$CFC_TEST_ROOT/ssh.log"
            exec sh -c "$(printf '%s' "$last" | sed "s#/data/cfdcraft/work#$CFC_TEST_ROOT/box#g")"
        fi;;
esac
exec "$(dirname "$0")/ssh-base" "$@"
"##;

/// A well-formed digest reference that names no real image.
pub const TEST_IMAGE: &str =
    "example/openfoam@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn profile() -> HostProfile {
    HostProfile::load(
        &workspace_root().join("crates/cfdcraft-executor/tests/hosts.example.toml"),
        "box",
    )
    .unwrap()
}

/// A short folder for tunnel sockets (the whole socket path has a length limit).
pub fn sockets() -> PathBuf {
    workspace_root().join("target/cfdcraft-sockets")
}

/// A fresh fake box: `bin/` with the fakes, a case folder, a stand-in key and
/// `box/`, the folder that plays the remote work directory. The fake ssh really
/// makes folders and unpacks uploads in it.
pub fn new_root(group: &str, tag: &str) -> PathBuf {
    let root = workspace_root()
        .join("target")
        .join(group)
        .join(format!("{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in ["bin", "case/system", "empty", "box"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("dummy-key"), "not a credential").unwrap();
    std::fs::write(root.join("case/Allrun"), "#!/bin/sh\n").unwrap();
    std::fs::write(root.join("case/system/controlDict"), "stopAt endTime;\n").unwrap();
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
    for (name, script) in [
        ("ssh-base", FAKE_SSH),
        ("ssh", SSH_WRAPPER),
        ("docker", FAKE_DOCKER),
    ] {
        let path = root.join("bin").join(name);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(root.join("box-real"), "1").unwrap();
    root
}

/// The run folder on the fake box.
pub fn box_dir(root: &Path, run_id: &str) -> PathBuf {
    root.join("box").join(run_id)
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
/// its PATH, so the parent's environment is never changed and no test reaches a
/// real ssh or docker.
pub fn worker(root: &Path, worker_test: &str, mode: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", worker_test, "--nocapture"])
        .env("PATH", child_path(root))
        .env("CFC_TEST_ROOT", root)
        .env("CFC_TEST_MODE", mode)
        .env("CFC_TEST_BOX_REAL", "1")
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
