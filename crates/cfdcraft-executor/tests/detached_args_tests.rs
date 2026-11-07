//! The argument list of a detached container: labels, process count, and the
//! same name and folder checks as an attached run.

use cfdcraft_executor::{
    HostProfile,
    docker::{RunSpec, detached_args},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

const IMAGE: &str =
    "example/openfoam@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const REMOTE: &str = "/data/cfdcraft/work/cfdcraft-demo-1-2";
const NAME: &str = "cfdcraft-demo-1-2";

/// The example profile with a core cap of two.
fn profile() -> HostProfile {
    let text = include_str!("hosts.example.toml").replace(
        "remote_user_uid = 1000",
        "remote_user_uid = 1000\ncore_cap = 2",
    );
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let file = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "detached-args-{}-{}.toml",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, text).unwrap();
    let p = HostProfile::load(&file, "box").unwrap();
    let _ = std::fs::remove_file(&file);
    p
}

fn spec() -> RunSpec {
    RunSpec::allrun(IMAGE).unwrap()
}

fn has_pair(args: &[String], a: &str, b: &str) -> bool {
    args.windows(2).any(|w| w[0] == a && w[1] == b)
}

#[test]
fn detached_args_carry_labels_np_and_no_rm() {
    let args = detached_args(&profile(), REMOTE, NAME, "demo", 2, &spec()).unwrap();
    assert_eq!(&args[..5], ["run", "-d", "--pull=never", "--name", NAME]);
    assert!(has_pair(&args, "--label", &format!("cfdcraft.run={NAME}")));
    assert!(has_pair(&args, "--label", "cfdcraft.doc=demo"));
    assert!(has_pair(&args, "--cpus", "2"));
    assert!(has_pair(&args, "--network", "none"));
    assert!(has_pair(&args, "--user", "1000:1000"));
    assert!(has_pair(&args, "-v", &format!("{REMOTE}:/w")));
    assert!(!args.iter().any(|a| a == "--rm"));
}

#[test]
fn np_above_the_core_cap_or_zero_is_refused() {
    let p = profile();
    assert!(detached_args(&p, REMOTE, NAME, "demo", 0, &spec()).is_err());
    assert!(detached_args(&p, REMOTE, NAME, "demo", 3, &spec()).is_err());
    assert!(detached_args(&p, REMOTE, NAME, "demo", 1, &spec()).is_ok());
}

#[test]
fn a_doc_label_with_a_space_or_a_quote_is_refused() {
    let p = profile();
    let long = "x".repeat(41);
    for bad in ["a b", "a'b", "a\"b", "a/b", "a;b", "", long.as_str()] {
        assert!(
            detached_args(&p, REMOTE, NAME, bad, 1, &spec()).is_err(),
            "{bad:?}"
        );
    }
    assert!(detached_args(&p, REMOTE, NAME, &"x".repeat(40), 1, &spec()).is_ok());
}

#[test]
fn a_remote_folder_outside_the_work_dir_is_refused() {
    let p = profile();
    for bad in [
        "/tmp/cfdcraft-demo-1-2",
        "/data/cfdcraft/workx/a",
        "/data/cfdcraft/work/a b",
        "/data/cfdcraft/work/a'b",
        "/data/cfdcraft/work",
    ] {
        assert!(
            detached_args(&p, bad, NAME, "demo", 1, &spec()).is_err(),
            "{bad:?}"
        );
    }
    assert!(detached_args(&p, REMOTE, "other-1", "demo", 1, &spec()).is_err());
    assert!(detached_args(&p, REMOTE, "cfdcraft-a'b", "demo", 1, &spec()).is_err());
}

#[test]
fn the_timed_script_wraps_allrun_in_timeout() {
    let timed = RunSpec::allrun_timed(IMAGE, 120).unwrap();
    let args = detached_args(&profile(), REMOTE, NAME, "demo", 1, &timed).unwrap();
    assert_eq!(
        args.last().map(String::as_str),
        Some("cd /w && timeout -k 30 120 sh ./Allrun")
    );
    assert!(RunSpec::allrun_timed(IMAGE, 0).is_err());
    assert!(RunSpec::allrun_timed(IMAGE, 86401).is_err());
    assert!(RunSpec::allrun_timed(IMAGE, 86400).is_ok());
    assert!(RunSpec::allrun_timed("example/openfoam:latest", 60).is_err());
    let plain = detached_args(&profile(), REMOTE, NAME, "demo", 1, &spec()).unwrap();
    assert_eq!(
        plain.last().map(String::as_str),
        Some("cd /w && sh ./Allrun")
    );
}
