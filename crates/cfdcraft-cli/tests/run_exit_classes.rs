//! A batch run exits with the class of what happened: real `cfdcraft` processes against a fake ssh
//! and a fake docker. Every class from 0 to 4 is reached, two processes racing for one box produce
//! one run and one busy refusal, and nothing about the host reaches stdout or the JSON report.
#![cfg(unix)]

#[path = "../../cfdcraft-ops/tests/support/mod.rs"]
mod support;

use serde_json::Value;
use std::{
    ffi::OsStr,
    fs::File,
    path::Path,
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};
use support::*;

const GROUP: &str = "cli-exit-classes";

/// The built binary with the fakes first on its PATH, run from `target/` so its socket directory
/// is short enough for a unix socket. Run and socket names are unique per process.
fn cli<I, S>(root: &Path, docker_mode: &str, args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let path = std::env::join_paths(
        std::iter::once(root.join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_cfdcraft"));
    c.args(args)
        .env("PATH", path)
        .env("CFC_TEST_ROOT", root)
        .env("CFC_TEST_MODE", docker_mode)
        .stdin(Stdio::null())
        .current_dir(workspace_root().join("target"));
    c
}

/// `<verb> <study> --key <dummy key> --hosts <hosts file>` plus `extra`.
fn box_args(root: &Path, verb: &str, study: &str, extra: &[&str]) -> Vec<String> {
    let path = |name: &str| root.join(name).to_string_lossy().into_owned();
    let mut args = vec![
        verb.to_owned(),
        path(study),
        "--key".to_owned(),
        path("dummy-key"),
        "--hosts".to_owned(),
        path("hosts.toml"),
    ];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    args
}

/// A fake box with the cavity and the channel studies written next to it.
fn fresh(tag: &str) -> std::path::PathBuf {
    let root = new_root(GROUP, tag);
    std::fs::write(root.join("cavity_v1.json"), CAVITY).unwrap();
    std::fs::write(root.join("channel_v1.json"), CHANNEL).unwrap();
    root
}

fn code(o: &Output) -> i32 {
    o.status
        .code()
        .expect("the process ended by exit, not by signal")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).expect("stdout is exactly one JSON document")
}

/// The remote work directory the test hosts file configures.
fn work_dir(root: &Path) -> String {
    let hosts = std::fs::read_to_string(root.join("hosts.toml")).unwrap();
    let line = hosts.lines().find(|l| l.starts_with("work_dir")).unwrap();
    line.split('"').nth(1).unwrap().to_owned()
}

/// The values the test itself configured must not appear in public output.
fn assert_no_infrastructure(root: &Path, what: &str, public: &str) {
    for secret in [
        root.join("dummy-key").to_string_lossy().into_owned(),
        SSH_TARGET.to_owned(),
        work_dir(root),
    ] {
        assert!(
            !public.contains(&secret),
            "{what} names {secret}:\n{public}"
        );
    }
}

fn run_count(root: &Path) -> usize {
    events(root).iter().filter(|e| *e == "run").count()
}

#[test]
fn an_empty_result_is_class_1_and_the_text_says_why() {
    let root = fresh("c1");
    let o = cli(&root, "ok", box_args(&root, "run", "cavity_v1.json", &[]))
        .output()
        .unwrap();
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.contains("evidence verdict: FAILED"), "{out}");
    assert!(out.contains("study hash: "), "{out}");
    assert!(
        out.contains("  ") && out.contains("missing or empty"),
        "reasons follow the verdict:\n{out}"
    );
    assert_no_infrastructure(&root, "stdout", &out);

    for verb in ["run", "validate"] {
        let o = cli(
            &root,
            "ok",
            box_args(&root, verb, "cavity_v1.json", &["--json"]),
        )
        .output()
        .unwrap();
        assert_eq!(code(&o), 1, "{verb}: {}", stderr(&o));
        let report = json(&o);
        assert_eq!(report["exit_class"], 1);
        assert_eq!(report["exit_class_name"], "gate_failed");
        assert_eq!(report["evidence"]["verdict"], "failed");
        assert!(
            report.get("gate").is_none(),
            "the reference comparison is only made on passing evidence"
        );
        assert_no_infrastructure(&root, "the report", &stdout(&o));
    }
}

#[test]
fn a_busy_box_is_class_4_with_the_holder_and_nothing_is_started() {
    let root = fresh("busy");
    hold_lock(&root, now_epoch(), 3600);
    let o = cli(
        &root,
        "ok",
        box_args(&root, "run", "cavity_v1.json", &["--json"]),
    )
    .output()
    .unwrap();
    assert_eq!(code(&o), 4, "{}", stderr(&o));
    let report = json(&o);
    assert_eq!(report["exit_class"], 4);
    assert_eq!(report["exit_class_name"], "infrastructure");
    assert_eq!(report["infra"]["code"], "CFD-INFRA-001");
    assert_eq!(report["infra"]["holder"]["run_id"], "other-run-1");
    assert!(report["infra"]["retry_after_seconds"].as_u64().unwrap() >= 30);
    assert_eq!(
        run_count(&root),
        0,
        "no container may start: {:?}",
        events(&root)
    );
    assert!(
        lock_dir(&root).exists(),
        "somebody else's lock is never broken"
    );
    assert_no_infrastructure(&root, "the report", &stdout(&o));
}

#[test]
fn ssh_and_setup_failures_are_class_4_and_the_public_output_names_no_infrastructure() {
    let root = fresh("infra");
    for json_mode in [true, false] {
        let extra: &[&str] = if json_mode { &["--json"] } else { &[] };
        let mut ssh_down = cli(&root, "ok", box_args(&root, "run", "cavity_v1.json", extra));
        ssh_down.env("CFC_TEST_FAIL_255", "uptime");
        let o = ssh_down.output().unwrap();
        assert_eq!(code(&o), 4, "{}", stderr(&o));
        if json_mode {
            assert_eq!(json(&o)["infra"]["code"], "CFD-INFRA-002");
        }
        assert_no_infrastructure(&root, "stdout", &stdout(&o));
        assert_eq!(run_count(&root), 0);

        let mut args = box_args(&root, "run", "cavity_v1.json", extra);
        let at = args.iter().position(|a| a == "--hosts").unwrap() + 1;
        args[at] = root
            .join("absent-hosts.toml")
            .to_string_lossy()
            .into_owned();
        let o = cli(&root, "ok", args).output().unwrap();
        assert_eq!(code(&o), 4, "{}", stderr(&o));
        if json_mode {
            assert_eq!(json(&o)["infra"]["code"], "CFD-INFRA-005");
        }
        assert_no_infrastructure(&root, "stdout", &stdout(&o));
    }
}

#[test]
fn the_five_exit_classes_each_come_from_a_real_process() {
    let root = fresh("table");
    let compile_out = root.join("compiled");
    let o0 = cli(
        &root,
        "ok",
        [
            "compile".to_owned(),
            root.join("cavity_v1.json").to_string_lossy().into_owned(),
            compile_out.to_string_lossy().into_owned(),
        ],
    )
    .output()
    .unwrap();

    let o1 = cli(&root, "ok", box_args(&root, "run", "cavity_v1.json", &[]))
        .output()
        .unwrap();

    let outlet = "    \"outlet\": {\n      \"faces\": [\"XMax\"],\n      \"kind\": \"PressureOutlet\"\n    },\n";
    let broken = CHANNEL.replace(outlet, "");
    assert_ne!(broken, CHANNEL, "the fixture changed shape");
    std::fs::write(root.join("broken.json"), broken).unwrap();
    let o2 = cli(
        &root,
        "ok",
        [
            "compile".to_owned(),
            root.join("broken.json").to_string_lossy().into_owned(),
            root.join("never").to_string_lossy().into_owned(),
        ],
    )
    .output()
    .unwrap();

    let dict = compile_out.join("system/blockMeshDict");
    let text = std::fs::read_to_string(&dict).unwrap();
    let planted = text.replace(
        "convertToMeters 1;",
        "convertToMeters #codeStream { code \"x\"; };",
    );
    assert_ne!(planted, text, "the compiled file changed shape");
    std::fs::write(&dict, planted).unwrap();
    let o3 = cli(
        &root,
        "ok",
        [
            "check-case".to_owned(),
            compile_out.to_string_lossy().into_owned(),
        ],
    )
    .output()
    .unwrap();

    let busy = fresh("table-busy");
    hold_lock(&busy, now_epoch(), 3600);
    let o4 = cli(&busy, "ok", box_args(&busy, "run", "cavity_v1.json", &[]))
        .output()
        .unwrap();

    let table = [
        ("passed", 0, &o0),
        ("gate failed", 1, &o1),
        ("study invalid", 2, &o2),
        ("guard refused", 3, &o3),
        ("infrastructure", 4, &o4),
    ];
    for (name, expected, output) in table {
        assert_eq!(code(output), expected, "{name}: {}", stderr(output));
    }
}

/// Wait for the first of two children to end.
fn first_to_exit(children: &mut [Child; 2]) -> usize {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        for (i, child) in children.iter_mut().enumerate() {
            if child.try_wait().unwrap().is_some() {
                return i;
            }
        }
        assert!(Instant::now() < deadline, "neither process ended");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn race_once(tag: &str) {
    let root = fresh(tag);
    let spawn = |name: &str| {
        let out = File::create(root.join(format!("{name}.out"))).unwrap();
        cli(
            &root,
            "hold",
            box_args(&root, "run", "cavity_v1.json", &["--json"]),
        )
        .stdout(out)
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
    };
    // Both are running before either is waited on.
    let mut children = [spawn("a"), spawn("b")];
    let loser = first_to_exit(&mut children);
    let winner = 1 - loser;
    assert!(
        children[winner].try_wait().unwrap().is_none(),
        "the winner is still held by its container"
    );
    let loser_status = children[loser].wait().unwrap();
    // The held container finishes only now.
    std::fs::write(root.join("release"), "").unwrap();
    let winner_status = children[winner].wait().unwrap();

    assert_eq!(
        loser_status.code(),
        Some(4),
        "the loser is told the box is busy"
    );
    assert_eq!(
        winner_status.code(),
        Some(1),
        "the winner ran and its empty result failed"
    );
    let name = |i: usize| if i == 0 { "a" } else { "b" };
    let loser_report: Value =
        serde_json::from_slice(&std::fs::read(root.join(format!("{}.out", name(loser)))).unwrap())
            .unwrap();
    assert_eq!(loser_report["infra"]["code"], "CFD-INFRA-001");
    assert_eq!(run_count(&root), 1, "exactly one run: {:?}", events(&root));
    assert!(!lock_dir(&root).exists(), "no lock remains");
}

#[test]
fn two_processes_for_one_box_give_one_run_and_one_class_4() {
    race_once("race1");
    race_once("race2");
}

#[test]
fn a_study_the_mode_cannot_run_is_class_2_before_any_connection() {
    let root = fresh("modes");
    let o = cli(
        &root,
        "ok",
        box_args(&root, "validate", "channel_v1.json", &["--json"]),
    )
    .output()
    .unwrap();
    assert_eq!(code(&o), 2, "{}", stderr(&o));
    assert_eq!(json(&o)["diagnostics"][0]["code"], "CFD-RUN-002");

    let o = cli(
        &root,
        "ok",
        box_args(&root, "run", "cavity_v1.json", &["--smoke", "--json"]),
    )
    .output()
    .unwrap();
    assert_eq!(code(&o), 2, "{}", stderr(&o));
    assert_eq!(json(&o)["diagnostics"][0]["code"], "CFD-RUN-001");
    assert!(ssh_log(&root).is_empty(), "nothing reached the host");
    assert_eq!(run_count(&root), 0);
}
