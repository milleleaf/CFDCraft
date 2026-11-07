//! The verbs for variants, run through the real binary: making, listing, showing, comparing and
//! labelling them, and how a bad argument or an unknown variant is refused. No test needs a
//! compute host.

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use repro_support::{CAVITY, FixtureRunner, new_document, passed, scratch};
use serde_json::Value;

const LID: &str = "/boundary_conditions/lid/velocity/value";

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

/// The data a verb printed on stdout.
fn data(o: &Output) -> Value {
    serde_json::from_str(&out(o)).expect("stdout is the JSON data")
}

/// A new document of the cavity.
fn cavity(tag: &str) -> PathBuf {
    new_document(&scratch(tag), "cavity", CAVITY)
}

/// `fast-lid`: the lid at 0.2 m/s.
fn with_fast_lid(doc: &Path) {
    let set = format!("{LID}=0.2");
    let o = run(&["scenario", "set", s(doc), "fast-lid", "--set", &set]);
    assert_eq!(code(&o), 0, "{}", err(&o));
}

fn listed(doc: &Path) -> Value {
    let o = run(&["scenario", "list", s(doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    data(&o)
}

#[test]
fn scenario_set_creates_a_variant_and_prints_the_sentence() {
    let doc = cavity("cli-scn-set");
    let set = format!("{LID}=0.2");
    let o = run(&["scenario", "set", s(&doc), "fast-lid", "--set", &set]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Created variant fast-lid of base."),
        "{}",
        out(&o)
    );
}

#[test]
fn scenario_list_prints_the_data_and_json_prints_the_report() {
    let doc = cavity("cli-scn-list");
    with_fast_lid(&doc);

    let shown = listed(&doc);
    assert_eq!(shown["scenarios"][0]["id"], "base");
    assert_eq!(shown["scenarios"][1]["id"], "fast-lid");
    assert_eq!(shown["scenarios"][1]["kind"], "setting");

    let o = run(&["scenario", "list", s(&doc), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(err(&o).is_empty(), "{}", err(&o));
    assert_eq!(json_out(&o)["repro"]["kind"], "scenarios");
}

#[test]
fn scenario_show_prints_overrides_and_raw_overrides() {
    let doc = cavity("cli-scn-show");
    with_fast_lid(&doc);

    let o = run(&["scenario", "show", s(&doc), "fast-lid"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = data(&o);
    assert_eq!(shown["overrides"][0]["setting"], LID);
    assert_eq!(shown["overrides"][0]["value"], 0.2);

    let raw = "/raw_overrides/relax-u={\"backend\":\"esi\",\"file\":\"system/fvSolution\",\"entry\":\"relaxationFactors/equations/U\",\"value\":\"0.5\"}";
    let o = run(&["scenario", "set", s(&doc), "relaxed", "--set", raw]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let o = run(&["scenario", "show", s(&doc), "relaxed"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = data(&o);
    assert_eq!(shown["raw_overrides"][0]["id"], "relax-u");
    assert_eq!(shown["raw_overrides"][0]["file"], "system/fvSolution");
    assert_eq!(shown["raw_overrides"][0]["value"], "0.5");
}

#[test]
fn scenario_diff_prints_the_sentence_a_person_reads() {
    let doc = cavity("cli-scn-diff");
    with_fast_lid(&doc);

    let o = run(&["scenario", "diff", s(&doc), "base", "fast-lid"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("fast-lid differs from base: lid velocity 0.1 → 0.2 m/s."),
        "{}",
        out(&o)
    );

    let set = format!("{LID}=0.2");
    let o = run(&["scenario", "set", s(&doc), "also-fast", "--set", &set]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let o = run(&["scenario", "diff", s(&doc), "fast-lid", "also-fast"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("fast-lid and also-fast make the same case."),
        "{}",
        out(&o)
    );
}

#[test]
fn scenario_label_sets_name_pin_and_notes() {
    let doc = cavity("cli-scn-label");
    with_fast_lid(&doc);

    let o = run(&[
        "scenario",
        "label",
        s(&doc),
        "fast-lid",
        "--name",
        "Fast lid",
        "--pin",
        "--notes",
        "check",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let row = &listed(&doc)["scenarios"][1];
    assert_eq!(row["name"], "Fast lid");
    assert_eq!(row["pinned"], true);
    assert_eq!(row["notes"], "check");

    let o = run(&[
        "scenario",
        "label",
        s(&doc),
        "fast-lid",
        "--unpin",
        "--name",
        "",
        "--notes",
        "",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let row = &listed(&doc)["scenarios"][1];
    assert_eq!(row["name"], Value::Null);
    assert_eq!(row["pinned"], false);
    assert_eq!(row["notes"], Value::Null);

    let o = run(&["scenario", "label", s(&doc), "fast-lid", "--pin", "--unpin"]);
    assert_eq!(code(&o), 2, "{}", err(&o));
}

#[test]
fn a_malformed_set_argument_is_exit_2_with_a_sentence() {
    let doc = cavity("cli-scn-badset");
    let before = fs::read(&doc).unwrap();
    for bad in ["/a", "/a=notjson", "=1"] {
        let o = run(&["scenario", "set", s(&doc), "x", "--set", bad]);
        assert_eq!(code(&o), 2, "{bad}: {}", err(&o));
        assert!(err(&o).contains(bad), "{bad}: {}", err(&o));
        assert!(out(&o).is_empty(), "{bad}: {}", out(&o));
        assert!(!err(&o).contains("panicked"), "{bad}: {}", err(&o));
    }
    assert_eq!(fs::read(&doc).unwrap(), before);
}

#[test]
fn an_unknown_variant_is_exit_2_and_names_it() {
    let doc = cavity("cli-scn-unknown");
    for args in [
        vec!["scenario", "show", s(&doc), "nope"],
        vec!["scenario", "label", s(&doc), "nope", "--pin"],
    ] {
        let o = run(&args);
        assert_eq!(code(&o), 2, "{}", err(&o));
        assert!(err(&o).contains("CFD-SCN-001"), "{}", err(&o));
    }
}

#[test]
fn nothing_private_is_printed() {
    let doc = cavity("cli-scn-private");
    let folder = s(doc.parent().unwrap()).to_owned();
    with_fast_lid(&doc);
    let verbs = [
        vec!["scenario", "list", s(&doc)],
        vec!["scenario", "show", s(&doc), "fast-lid"],
        vec!["scenario", "diff", s(&doc), "base", "fast-lid"],
        vec!["scenario", "label", s(&doc), "fast-lid", "--pin"],
        vec!["scenario", "show", s(&doc), "nope"],
        vec!["scenario", "list", s(&doc), "--json"],
    ];
    for args in verbs {
        let o = run(&args);
        assert!(!out(&o).contains(&folder), "{args:?}: {}", out(&o));
        assert!(!err(&o).contains(&folder), "{args:?}: {}", err(&o));
    }
}

/// A new document of the cavity with the study itself solved once.
fn solved(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    let doc = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&cfdcraft_ops::solve_document(&doc, &mut runner).report);
    doc
}

#[test]
fn status_with_a_scenario_reports_that_variants_stages() {
    let doc = solved("cli-scn-status");
    with_fast_lid(&doc);

    let o = run(&["status", s(&doc), "--scenario", "fast-lid"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        err(&o).contains("No result has been made yet."),
        "{}",
        err(&o)
    );

    let o = run(&["status", s(&doc), "--scenario", "fast-lid", "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let report = json_out(&o);
    assert_eq!(report["repro"]["mesh"]["state"], "fresh");
    assert_eq!(report["repro"]["result"]["state"], "never_made");

    let o = run(&["status", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Nothing is out of date."), "{}", out(&o));
}

#[test]
fn status_with_an_unknown_scenario_is_exit_2() {
    let doc = solved("cli-scn-status-unknown");
    let o = run(&["status", s(&doc), "--scenario", "nope"]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("CFD-SCN-001"), "{}", err(&o));
}

#[test]
fn solve_with_an_unknown_scenario_is_exit_2_before_any_host_is_used() {
    let doc = solved("cli-scn-solve-unknown");
    let o = run(&[
        "solve",
        s(&doc),
        "--key",
        "missing.key",
        "--scenario",
        "nope",
    ]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("CFD-SCN-001"), "{}", err(&o));
    assert!(!err(&o).contains("missing.key"), "{}", err(&o));
}

#[test]
fn solve_takes_the_new_flags() {
    let o = run(&["solve", "--help"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("--scenario"), "{}", out(&o));
    assert!(out(&o).contains("--remake-pruned"), "{}", out(&o));

    // The study itself is named base; with nothing out of date no host is needed.
    let doc = solved("cli-scn-solve-base");
    let o = run(&[
        "solve",
        s(&doc),
        "--key",
        "missing.key",
        "--scenario",
        "base",
        "--remake-pruned",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Nothing to run: the mesh and the result are up to date."),
        "{}",
        out(&o)
    );
}

#[test]
fn the_existing_verbs_are_unchanged() {
    let doc = solved("cli-scn-solve-plain");
    let o = run(&["solve", s(&doc), "--key", "missing.key"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Nothing to run: the mesh and the result are up to date."),
        "{}",
        out(&o)
    );
}
