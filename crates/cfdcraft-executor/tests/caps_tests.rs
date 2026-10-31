use cfdcraft_executor::{
    HostProfile,
    docker::run_args,
    hosts::preflight,
    run::{RunError, run_id, wait_bounded},
};
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

fn profile() -> HostProfile {
    HostProfile::load(
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/hosts.example.toml"
        )),
        "box",
    )
    .unwrap()
}

#[test]
fn preflight_load_boundary_and_foreign_services() {
    let p = profile();
    assert!(
        preflight(
            &p,
            "up 14 days, load average: 7.99, 2.0, 1.0",
            "ai-pilot-api-1\ngqa-service"
        )
        .is_ok()
    );
    assert!(preflight(&p, "load average: 8.0, 1, 1", "").is_err());
    assert!(preflight(&p, "load average: 8.01, 1, 1", "").is_err());
    assert!(preflight(&p, "load average: 0.1, 1, 1", "cfdcraft-old Up 1 hour").is_err());
    assert!(preflight(&p, "load average: NaN, 1, 1", "").is_err());
    assert!(preflight(&p, "garbage", "").is_err());
}

#[test]
fn caps_and_ownership_are_always_on_run_argv() {
    let p = profile();
    let a = run_args(&p, "/data/cfdcraft/work/cfdcraft-test", "cfdcraft-test").unwrap();
    for (k, v) in [
        ("--cpus", "2"),
        ("--memory", "8g"),
        ("--user", "1000:1000"),
        ("--memory-swap", "8g"),
        ("--network", "none"),
    ] {
        let i = a.iter().position(|x| x == k).unwrap();
        assert_eq!(a[i + 1], v);
    }
    assert!(a.contains(&"--rm".into()));
    assert!(run_args(&p, "/data/cfdcraft/work/cfdcraft-test", "foreign").is_err());
    assert!(run_args(&p, "/data/foreign/case", "cfdcraft-test").is_err());
    assert!(run_id("cavity").unwrap().starts_with("cfdcraft-"));
    assert!(run_id("bad; touch pwned").is_err());
}

#[test]
fn profile_rejects_unsafe_caps_and_targets() {
    let mut p = profile();
    p.cpus = 0;
    assert!(p.validate().is_err());
    p = profile();
    p.concurrency = 2;
    assert!(p.validate().is_err());
    p = profile();
    p.work_dir = "/data/other".into();
    assert!(p.validate().is_err());
}

#[test]
fn wall_clock_cap_kills_and_reaps_child() {
    #[cfg(unix)]
    let mut c = {
        let mut c = Command::new("sleep");
        c.arg("5");
        c
    };
    #[cfg(windows)]
    let mut c = {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 5"]);
        c
    };
    let mut child = c.spawn().unwrap();
    let t = Instant::now();
    assert!(matches!(
        wait_bounded(&mut child, Duration::from_millis(100)),
        Err(RunError::TimedOut)
    ));
    assert!(t.elapsed() < Duration::from_secs(3));
    assert!(child.try_wait().unwrap().is_some());
}
