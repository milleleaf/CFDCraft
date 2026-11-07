//! The largest values a host profile may name, and the checks that did not change.

use cfdcraft_executor::{
    HostProfile,
    hosts::{MAX_CPUS, MAX_MEMORY_GB, MAX_WALL_CLOCK_SECONDS},
};
use std::path::Path;

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
fn the_constants_are_the_decided_values() {
    assert_eq!(MAX_CPUS, 8);
    assert_eq!(MAX_MEMORY_GB, 16);
    assert_eq!(MAX_WALL_CLOCK_SECONDS, 86_400);
}

#[test]
fn the_new_maxima_are_accepted_and_one_more_is_refused() {
    let mut p = profile();
    p.cpus = 8;
    assert!(p.validate().is_ok());
    p.cpus = 9;
    assert!(p.validate().is_err());

    let mut p = profile();
    p.memory_gb = 16;
    assert!(p.validate().is_ok());
    p.memory_gb = 17;
    assert!(p.validate().is_err());

    let mut p = profile();
    p.wall_clock_seconds = 86_400;
    assert!(p.validate().is_ok());
    p.wall_clock_seconds = 86_401;
    assert!(p.validate().is_err());
}

#[test]
fn concurrency_stays_one() {
    let mut p = profile();
    p.concurrency = 2;
    assert!(p.validate().is_err());
    p.concurrency = 0;
    assert!(p.validate().is_err());
}

#[test]
fn core_cap_is_still_at_most_cpus_and_at_least_one() {
    let mut p = profile();
    p.cpus = 8;
    p.core_cap = 8;
    assert!(p.validate().is_ok());
    p.core_cap = 9;
    assert!(p.validate().is_err());
    p.core_cap = 0;
    assert!(p.validate().is_err());
}

#[test]
fn the_unchanged_bounds_still_hold_for_everything_the_decision_did_not_change() {
    let ok = profile();
    let mut p = ok.clone();
    p.work_dir = "/data/other".into();
    assert!(p.validate().is_err());
    let mut p = ok.clone();
    p.docker_socket_remote = "/tmp/docker.sock".into();
    assert!(p.validate().is_err());
    let mut p = ok.clone();
    p.container_prefix = "other-".into();
    assert!(p.validate().is_err());
    let mut p = ok.clone();
    p.load_threshold = 0.0;
    assert!(p.validate().is_err());
    let mut p = ok;
    p.min_free_gb = 2001;
    assert!(p.validate().is_err());
}

#[test]
fn the_example_profile_still_loads() {
    let p = profile();
    assert_eq!((p.cpus, p.memory_gb, p.wall_clock_seconds), (2, 8, 3600));
    assert!(!p.clean_run_dirs);
}
