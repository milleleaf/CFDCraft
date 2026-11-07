//! The status of a variant, and the refusals that come before anything runs: a variant that does
//! not exist, a change that does not apply to the study, a study that fails the checks, and a list
//! of variants the file cannot be read with. Reading never writes.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_doc::{DocError, open, save};
use cfdcraft_ops::{
    ExitClass, Report, Repro, RunsData, StatusData, document_status, list_runs, save_document,
    scenario_status, solve_document, solve_scenario,
};
use cfdcraft_scenarios::{Override, Scenario};
use serde_json::{Value, json};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";

fn set(setting: &str, value: Value) -> Override {
    Override::Set {
        setting: setting.to_owned(),
        value,
    }
}

fn variant(id: &str, parent: &str, overrides: Vec<Override>) -> Scenario {
    Scenario {
        id: id.to_owned(),
        parent: Some(parent.to_owned()),
        name: None,
        pinned: false,
        notes: None,
        overrides,
        origin: None,
    }
}

/// Change the list of variants the document at `path` holds and save it.
fn edit_variants(path: &Path, change: impl FnOnce(&mut Vec<Scenario>)) {
    let mut document = open(path).unwrap().document;
    change(&mut document.artifacts.scenarios);
    save(&document, path).unwrap();
}

fn fast_lid() -> Scenario {
    variant("fast-lid", "base", vec![set(LID, json!(0.2))])
}

/// A document made from the cavity that holds these variants and has had its study solved.
fn solved_with(tag: &str, variants: Vec<Scenario>) -> (PathBuf, FixtureRunner) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    edit_variants(&path, |list| *list = variants);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    (path, runner)
}

fn status_of(report: &Report) -> &StatusData {
    match &report.repro {
        Some(Repro::Status(data)) => data,
        other => panic!("expected the status of a document, found {other:?}"),
    }
}

fn has_scenario(report: &Report, id: &str) -> bool {
    report
        .artifacts
        .iter()
        .any(|a| a.kind == "scenario" && a.name == id)
}

/// Check that a report is a refusal of class 2 with this code, and that it names `named`.
fn assert_refused(report: &Report, code: &str, named: &str) {
    assert_eq!(
        report.class(),
        ExitClass::StudyInvalid,
        "{}",
        report.render_text()
    );
    let found = report
        .diagnostics
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no {code} in {}", report.render_text()));
    assert!(found.message.contains(named), "{}", found.message);
}

#[test]
fn a_variant_with_a_solved_parent_has_a_fresh_mesh_and_no_result() {
    let (path, _runner) = solved_with("scs-fresh-mesh", vec![fast_lid()]);

    let done = scenario_status(&path, Some("fast-lid"));
    passed(&done.report);
    assert_eq!(done.report.operation, "status");
    assert!(has_scenario(&done.report, "fast-lid"));
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "fresh");
    assert_eq!(status.mesh.run.as_deref(), Some("run-1"));
    assert_eq!(status.result.state, "never_made");
    assert!(done.line.is_none());
}

#[test]
fn after_its_own_solve_both_stages_are_fresh() {
    let (path, mut runner) = solved_with("scs-both-fresh", vec![fast_lid()]);
    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);

    let done = scenario_status(&path, Some("fast-lid"));
    passed(&done.report);
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "fresh");
    assert_eq!(status.mesh.run.as_deref(), Some("run-1"));
    assert_eq!(status.result.state, "fresh");
    assert_eq!(status.result.run.as_deref(), Some("run-2"));
    assert!(done.line.is_some());
}

#[test]
fn a_stale_variant_result_is_judged_against_its_own_run() {
    let (path, mut runner) = solved_with("scs-stale", vec![fast_lid()]);
    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);
    edit_variants(&path, |list| list[0].overrides = vec![set(LID, json!(0.3))]);

    let done = scenario_status(&path, Some("fast-lid"));
    passed(&done.report);
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "fresh");
    assert_eq!(status.result.state, "stale");
    assert_eq!(status.result.since.as_deref(), Some("run-2"));
    assert_eq!(status.result.reasons, ["lid velocity 0.2 → 0.3 m/s"]);
}

#[test]
fn the_status_of_the_study_never_mentions_a_variants_run() {
    let (path, mut runner) = solved_with("scs-study-alone", vec![fast_lid()]);
    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);
    passed(&save_document(&with_lid_speed(0.15), &path, true).report);

    let done = document_status(&path);
    passed(&done.report);
    assert!(!has_scenario(&done.report, "fast-lid"));
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "fresh");
    assert_eq!(status.result.state, "stale");
    assert_eq!(status.result.since.as_deref(), Some("run-1"));
}

#[test]
fn an_unknown_variant_is_cfd_scn_001_and_nothing_runs() {
    let (path, mut runner) = solved_with("scs-unknown", vec![fast_lid()]);
    let before = snapshot(&path);

    let solved = solve_scenario(&path, Some("nope"), &mut runner);
    assert_refused(&solved.report, "CFD-SCN-001", "nope");
    assert_eq!(runner.calls, 1);
    let status = scenario_status(&path, Some("nope"));
    assert_refused(&status.report, "CFD-SCN-001", "nope");
    assert_eq!(status.report.operation, "status");
    assert_eq!(snapshot(&path), before);
}

#[test]
fn an_override_that_does_not_apply_is_cfd_scn_003() {
    let variants = vec![
        variant(
            "lost",
            "base",
            vec![set("/boundary_conditions/nope/kind", json!("NoSlipWall"))],
        ),
        variant("dense", "base", vec![set("/fluid/density", json!(1.2))]),
        variant(
            "wrong-type",
            "base",
            vec![set("/mesh/cells_x", json!("many"))],
        ),
    ];
    let (path, mut runner) = solved_with("scs-not-applied", variants);
    let before = snapshot(&path);

    let solved = solve_scenario(&path, Some("lost"), &mut runner);
    assert_refused(&solved.report, "CFD-SCN-003", "lost");
    assert_refused(
        &solved.report,
        "CFD-SCN-003",
        "/boundary_conditions/nope/kind",
    );
    let status = scenario_status(&path, Some("lost"));
    assert_refused(
        &status.report,
        "CFD-SCN-003",
        "/boundary_conditions/nope/kind",
    );

    for name in ["dense", "wrong-type"] {
        let solved = solve_scenario(&path, Some(name), &mut runner);
        assert_refused(&solved.report, "CFD-SCN-003", name);
        let status = scenario_status(&path, Some(name));
        assert_refused(&status.report, "CFD-SCN-003", name);
    }
    assert_eq!(runner.calls, 1);
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_variant_whose_study_fails_the_checks_reports_the_study_findings() {
    let still = variant("still", "base", vec![set(LID, json!(0.0))]);
    let (path, mut runner) = solved_with("scs-bad-study", vec![still]);
    let before = snapshot(&path);

    let solved = solve_scenario(&path, Some("still"), &mut runner);
    assert_eq!(solved.report.class(), ExitClass::StudyInvalid);
    assert!(
        codes(&solved.report).contains(&"CFD-VAL-002"),
        "{}",
        solved.report.render_text()
    );
    let status = scenario_status(&path, Some("still"));
    assert_eq!(status.report.class(), ExitClass::StudyInvalid);
    assert!(codes(&status.report).contains(&"CFD-VAL-002"));
    assert_eq!(runner.calls, 1);
    assert_eq!(snapshot(&path), before);
}

#[test]
fn list_runs_shows_the_variant_of_each_run() {
    let (path, mut runner) = solved_with("scs-list", vec![fast_lid()]);
    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);

    let done = list_runs(&path);
    passed(&done.report);
    let Some(Repro::Runs(RunsData { runs })) = &done.report.repro else {
        panic!("expected the list of runs, found {:?}", done.report.repro);
    };
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].scenario, None);
    assert_eq!(runs[1].scenario.as_deref(), Some("fast-lid"));
}

#[test]
fn a_document_whose_variant_list_is_refused_says_cfd_scn_002() {
    let dir = scratch("scs-refused-list");
    let path = new_document(&dir, "cavity", CAVITY);
    edit_variants(&path, |list| {
        list.push(variant("a", "base", vec![set(LID, json!(0.2))]));
        list.push(variant("b", "a", vec![set(LID, json!(0.3))]));
    });
    // Make the second variant its own parent, so its parent is not listed before it.
    tamper_with_index(&path, "\"parent\": \"a\"", "\"parent\": \"b\"");
    assert!(matches!(open(&path), Err(DocError::Scenarios(_))));
    let before = fs::read(&path).unwrap();
    let mut runner = FixtureRunner::new(&dir.join("runs"));

    let solved = solve_scenario(&path, Some("a"), &mut runner);
    assert_refused(&solved.report, "CFD-SCN-002", "\"b\"");
    let status = scenario_status(&path, Some("a"));
    assert_refused(&status.report, "CFD-SCN-002", "\"b\"");
    let plain = document_status(&path);
    assert_refused(&plain.report, "CFD-SCN-002", "\"b\"");
    assert_eq!(runner.calls, 0);
    assert_eq!(fs::read(&path).unwrap(), before);
}
