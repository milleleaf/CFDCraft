//! Every executor failure has a stable code and a public message that carries
//! nothing from the environment; the container command and image come only from
//! a fixed allowlist.
#![cfg(unix)]

mod support;

use cfdcraft_executor::{
    Holder, Job, RunConfig, RunError,
    docker::{RunSpec, run_args},
    hosts::PreflightError,
};
use std::path::PathBuf;
use support::*;

const GROUP: &str = "executor-error-tests";

/// Child side. Runs a whole job and writes the code, the public text and the
/// detail of the error it ended with.
#[test]
fn error_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let mut p = profile();
    if let Some(cap) = std::env::var("CFC_TEST_CAP")
        .ok()
        .and_then(|c| c.parse().ok())
    {
        p.wall_clock_seconds = cap;
    }
    let key = match std::env::var("CFC_TEST_SCENARIO").as_deref() {
        Ok("missing-key") => root.join("no-such-key"),
        _ => root.join("dummy-key"),
    };
    let result = Job::begin(&p, &key, RunConfig::from_cwd().unwrap(), "errors").and_then(|job| {
        job.execute(
            &root.join("case"),
            &root.join("results"),
            &RunSpec::allrun(TEST_IMAGE).unwrap(),
        )
    });
    let e = result.expect_err("every scenario here ends in an error");
    std::fs::write(root.join("code"), e.infra_code()).unwrap();
    std::fs::write(root.join("public"), e.public_message()).unwrap();
    std::fs::write(root.join("detail"), e.detail()).unwrap();
    std::fs::write(root.join("display"), e.to_string()).unwrap();
}

/// Run one failure and return its code; the public text and `Display` must not
/// carry the key path, the ssh target or the box directories.
fn code_of(tag: &str, mode: &str, scenario: &str, env: &[(&str, &str)]) -> String {
    let root = new_root(GROUP, tag);
    let mut c = worker(&root, "error_worker", mode);
    c.env("CFC_TEST_SCENARIO", scenario);
    for (k, v) in env {
        c.env(k, v);
    }
    assert_worker_ok(&c.output().unwrap());
    let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap();
    let public = read("public");
    assert_eq!(public, read("display"));
    for secret in [root.to_str().unwrap(), "USER@HOST", "/data/cfdcraft"] {
        assert!(!public.contains(secret), "{public}");
    }
    read("code")
}

#[test]
fn failures_get_stable_codes() {
    let ssh = code_of("ssh", "ok", "ssh", &[("CFC_TEST_FAIL_255", "uptime")]);
    let docker = code_of("docker", "ok", "docker", &[("CFC_TEST_DOCKER_FAIL", "ps")]);
    let timeout = code_of("timeout", "hold", "timeout", &[("CFC_TEST_CAP", "1")]);
    let setup = code_of("key", "ok", "missing-key", &[]);
    assert_eq!(
        [ssh, docker, timeout, setup],
        [
            "CFD-INFRA-002",
            "CFD-INFRA-003",
            "CFD-INFRA-004",
            "CFD-INFRA-005"
        ]
    );
}

#[test]
fn a_held_lock_and_a_running_container_are_busy() {
    let root = new_root(GROUP, "busy");
    std::fs::create_dir(lock_dir(&root)).unwrap();
    let mut c = worker(&root, "error_worker", "ok");
    c.env("CFC_TEST_SCENARIO", "busy");
    assert_worker_ok(&c.output().unwrap());
    assert_eq!(
        std::fs::read_to_string(root.join("code")).unwrap(),
        "CFD-INFRA-001"
    );
}

#[test]
fn classes_without_a_fixture_map_to_their_codes() {
    let busy_load = PreflightError::Busy {
        load_1min: 9.0,
        threshold: 8.0,
    };
    let existing = PreflightError::ExistingCfdcraftContainers { names: vec![] };
    for e in [
        RunError::Busy {
            holder: None,
            stale: false,
        },
        RunError::Preflight(busy_load),
        RunError::Preflight(existing),
    ] {
        assert_eq!(e.infra_code(), "CFD-INFRA-001");
    }
    assert_eq!(RunError::Cancelled.infra_code(), "CFD-INFRA-006");
    assert_eq!(RunError::SocketPathTooLong.infra_code(), "CFD-INFRA-005");
    assert_eq!(
        RunError::Preflight(PreflightError::InvalidLoad).infra_code(),
        "CFD-INFRA-005"
    );
}

#[test]
fn public_text_never_carries_the_environment() {
    let secret = "/home/someone/.ssh/id_key user@box.example /data/cfdcraft/work/cfdcraft-x-1-2";
    let holder = Holder::new("cfdcraft-x-1-2", "cfdcraft-x-1-2", 1, 600, "test-client").unwrap();
    let with_detail = [
        RunError::Io(std::io::Error::other(secret)),
        RunError::Preflight(PreflightError::InvalidProfile(secret.into())),
        RunError::Setup(secret.into()),
        RunError::Ssh {
            detail: secret.into(),
        },
        RunError::Docker {
            detail: secret.into(),
        },
    ];
    let without = [
        RunError::TimedOut,
        RunError::Cancelled,
        RunError::SocketPathTooLong,
        RunError::Busy {
            holder: Some(holder),
            stale: true,
        },
    ];
    for e in &with_detail {
        for shown in [
            e.public_message(),
            e.to_string(),
            format!("{e:?}").replace(secret, ""),
        ] {
            for part in ["id_key", "user@box", "/data/cfdcraft"] {
                assert!(!shown.contains(part), "{e:?} leaked {part}: {shown}");
            }
        }
        assert!(e.detail().contains(secret), "{e:?}");
    }
    for e in &without {
        assert!(!e.public_message().contains("cfdcraft-x"), "{e:?}");
        assert!(!e.detail().is_empty());
    }
}

#[test]
fn the_image_must_be_a_lowercase_digest_reference() {
    let hex = "0123456789abcdef".repeat(4);
    assert!(RunSpec::allrun(&format!("example/openfoam@sha256:{hex}")).is_ok());
    for bad in [
        "example/openfoam:2606".to_string(),
        format!("example/openfoam@sha256:{}", &hex[..63]),
        format!("example/openfoam@sha256:{}", hex.to_uppercase()),
        format!("example/openfoam @sha256:{hex}"),
        format!("example/openfoam\"@sha256:{hex}"),
        format!("example/openfoam;touch x@sha256:{hex}"),
        format!("-v/etc:/x@sha256:{hex}"),
        format!("@sha256:{hex}"),
    ] {
        assert!(RunSpec::allrun(&bad).is_err(), "{bad}");
    }
}

#[test]
fn the_allrun_command_is_fixed_and_the_caps_stay_on() {
    let spec = RunSpec::allrun(TEST_IMAGE).unwrap();
    let a = run_args(
        &profile(),
        "/data/cfdcraft/work/cfdcraft-test",
        "cfdcraft-test",
        &spec,
    )
    .unwrap();
    let tail = &a[a.len() - 4..];
    assert_eq!(tail, [TEST_IMAGE, "bash", "-lc", "cd /w && sh ./Allrun"]);
    for flag in ["--rm", "--pull=never"] {
        assert!(a.iter().any(|x| x == flag), "{flag}");
    }
    for (k, v) in [
        ("--cpus", "2"),
        ("--memory", "8g"),
        ("--memory-swap", "8g"),
        ("--network", "none"),
        ("--user", "1000:1000"),
    ] {
        let i = a.iter().position(|x| x == k).unwrap();
        assert_eq!(a[i + 1], v, "{k}");
    }
}
