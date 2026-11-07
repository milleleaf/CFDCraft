//! The verbs that read a solved study file, run through the real binary: whether it is up to
//! date and why not, its runs, what a run produced and was made from, and how two runs compare.
//! The documents are solved with a runner that writes a prepared results folder, so no test needs
//! a compute host.

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use repro_support::{
    CAVITY, FixtureRunner, new_document, passed, scratch, two_runs, with_lid_speed,
};
use serde_json::{Value, json};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .output()
        .unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("the binary ended by itself")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn json_out(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).expect("stdout is exactly one JSON document")
}

/// A new document with one solved run, `run-1`, that recorded the mesh and the result.
fn solved(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    let doc = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&cfdcraft_ops::solve_document(&doc, &mut runner).report);
    doc
}

/// Change the lid speed of the study in `doc` with the `save` verb.
fn change_lid(doc: &Path, speed: f64) {
    let study = doc.with_file_name("lid.json");
    fs::write(&study, with_lid_speed(speed)).unwrap();
    let saved = run(&["save", s(&study), s(doc), "--replace"]);
    assert_eq!(code(&saved), 0, "{}", err(&saved));
}

#[test]
fn status_of_a_solved_document_says_nothing_is_out_of_date() {
    let doc = solved("cli-status-fresh");
    let o = run(&["status", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Nothing is out of date."), "{}", out(&o));
}

#[test]
fn status_after_a_lid_change_prints_the_reason_and_still_exits_0() {
    let doc = solved("cli-status-stale");
    change_lid(&doc, 0.2);
    let o = run(&["status", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = err(&o);
    assert!(
        text.contains("result stale because lid velocity 0.1 → 0.2 m/s"),
        "{text}"
    );
    assert!(!text.contains("mesh stale"), "{text}");
}

#[test]
fn status_json_is_one_report_with_the_stage_states() {
    let doc = solved("cli-status-json");
    change_lid(&doc, 0.2);
    let o = run(&["status", s(&doc), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(err(&o).is_empty(), "{}", err(&o));
    let report = json_out(&o);
    assert_eq!(report["repro"]["kind"], "status");
    assert_eq!(report["repro"]["mesh"]["state"], "fresh");
    assert_eq!(report["repro"]["result"]["state"], "stale");
    assert_eq!(
        report["repro"]["result"]["reasons"][0],
        "lid velocity 0.1 → 0.2 m/s"
    );
}

#[test]
fn runs_prints_the_run_list_as_json_data() {
    let doc = solved("cli-runs");
    let o = run(&["runs", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let data: Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(data["runs"][0]["id"], "run-1");
    assert_eq!(data["runs"][0]["recorded"], json!(["mesh", "result"]));

    let o = run(&["runs", s(&doc), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(json_out(&o)["repro"]["kind"], "runs");
}

#[test]
fn result_prints_the_checks_and_key_numbers() {
    let doc = solved("cli-result");
    for extra in [&[][..], &["--run", "run-1"][..]] {
        let mut args = vec!["result", s(&doc)];
        args.extend_from_slice(extra);
        let o = run(&args);
        assert_eq!(code(&o), 0, "{}", err(&o));
        let data: Value = serde_json::from_str(&out(&o)).unwrap();
        assert_eq!(data["run"], "run-1");
        assert_eq!(data["checks"][0]["name"], "evidence");
        assert_eq!(data["checks"][0]["passed"], true);
        assert!(
            data["key_numbers"]["solver.iterations"].is_number(),
            "{data}"
        );
    }
}

#[test]
fn provenance_prints_the_recorded_provenance() {
    let doc = solved("cli-provenance");
    let o = run(&["provenance", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let data: Value = serde_json::from_str(&out(&o)).unwrap();
    let digest = data["provenance"]["tool"]["image_digest"].as_str().unwrap();
    assert!(digest.starts_with("sha256:"), "{digest}");
    assert_eq!(data["provenance"]["settings"]["schema_version"], 1);
    assert_eq!(data["provenance"]["host"], "fixture-host");
}

#[test]
fn an_unknown_run_exits_2_and_says_so() {
    let doc = solved("cli-unknown-run");
    let o = run(&["result", s(&doc), "--run", "run-9"]);
    assert_eq!(code(&o), 2);
    assert!(
        err(&o).contains("There is no run run-9 in this file."),
        "{}",
        err(&o)
    );
    assert!(out(&o).is_empty(), "{}", out(&o));
}

#[test]
fn compare_exits_by_verdict_and_takes_a_tolerance() {
    // The second run has a lid twice as fast, so its samples are 0.1 higher.
    let (apart, _) = two_runs("cli-compare-apart", 0.2, Vec::new());
    let o = run(&["compare", s(&apart), "run-1", "run-2"]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(err(&o).contains("CFD-REPRO-007"), "{}", err(&o));

    let o = run(&[
        "compare",
        s(&apart),
        "run-1",
        "run-2",
        "--abs-tol",
        "1",
        "--rel-tol",
        "1",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));

    let (same, _) = two_runs("cli-compare-same", 0.2, vec![0.0, 0.0]);
    let o = run(&["compare", s(&same), "run-1", "run-2"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("run-2 reproduced run-1: every checked quantity is identical."),
        "{}",
        out(&o)
    );
}

#[test]
fn a_bad_tolerance_exits_2() {
    let doc = solved("cli-bad-tolerance");
    let o = run(&["compare", s(&doc), "run-1", "run-1", "--abs-tol", "-1"]);
    assert_eq!(code(&o), 2);
    let text = err(&o);
    assert!(text.contains("error:") && text.contains("-1"), "{text}");
}

#[test]
fn a_file_that_is_not_a_study_file_exits_2() {
    let dir = scratch("cli-not-a-study");
    let plain = dir.join("plain.txt");
    fs::write(&plain, "this is not a study file").unwrap();
    let plain = s(&plain);
    let verbs: [&[&str]; 5] = [
        &["status", plain],
        &["runs", plain],
        &["result", plain],
        &["provenance", plain],
        &["compare", plain, "run-1", "run-2"],
    ];
    for args in verbs {
        let o = run(args);
        assert_eq!(code(&o), 2, "{args:?}: {}", err(&o));
    }
}

/// Pack `doc` with `extra` flags, unpack the pack into a new folder and return the unpacked file.
fn packed_and_unpacked(doc: &Path, extra: &[&str]) -> PathBuf {
    let dir = doc.parent().unwrap();
    let pack = dir.join("shared.cfdpack");
    let mut args = vec!["pack", s(doc), s(&pack)];
    args.extend_from_slice(extra);
    let made = run(&args);
    assert_eq!(code(&made), 0, "{}", err(&made));
    let target = dir.join("shared");
    let opened = run(&["unpack", s(&pack), s(&target)]);
    assert_eq!(code(&opened), 0, "{}", err(&opened));
    // unpack names the study after the pack, so `shared.cfdpack` gives `shared.cfdcraft`
    target.join("shared.cfdcraft")
}

/// The `host` of the newest run's provenance, as the `provenance` verb prints it.
fn host_of(doc: &Path) -> Value {
    let o = run(&["provenance", s(doc), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    json_out(&o)["repro"]["provenance"]["host"].clone()
}

#[test]
fn pack_keeps_the_host_label_by_default() {
    let doc = solved("cli-pack-default");
    let unpacked = packed_and_unpacked(&doc, &[]);
    assert_eq!(host_of(&unpacked), "fixture-host");
}

#[test]
fn pack_with_redact_host_removes_every_host_label() {
    let doc = solved("cli-pack-redacted");
    let unpacked = packed_and_unpacked(&doc, &["--redact-host"]);
    assert!(host_of(&unpacked).is_null());

    // The runs and the numbers are the original's, and the stored results are all there.
    for verb in ["runs", "result"] {
        let original = run(&[verb, s(&doc)]);
        let copy = run(&[verb, s(&unpacked)]);
        assert_eq!(code(&copy), 0, "{}", err(&copy));
        let strip = |text: String| {
            let mut data: Value = serde_json::from_str(&text).unwrap();
            if let Some(runs) = data["runs"].as_array_mut() {
                for run in runs {
                    run["host"] = Value::Null;
                }
            }
            data
        };
        assert_eq!(strip(out(&original)), strip(out(&copy)));
    }
    let status = run(&["status", s(&unpacked)]);
    assert_eq!(code(&status), 0, "{}", err(&status));
    assert!(
        out(&status).contains("Nothing is out of date."),
        "{}",
        out(&status)
    );

    // The source file still shows its label.
    assert_eq!(host_of(&doc), "fixture-host");
}
