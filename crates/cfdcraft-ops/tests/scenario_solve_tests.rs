//! Solving a variant: its effective study goes through the same checks and keys as the study, a
//! mesh any run holds the key of is reused, and the run is recorded with the variant and the study
//! it was made from. No test needs a compute host or a container engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_compare::{Tolerance, Verdict};
use cfdcraft_doc::{check_scenario_records, open, save};
use cfdcraft_ops::{ExitClass, JobAction, plan_job, save_document, solve_document, solve_scenario};
use cfdcraft_runs::verify_runs;
use cfdcraft_scenarios::{Override, Scenario};
use cfdcraft_text::Line;
use serde_json::{Value, json};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";

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

/// The speed of the lid in a study written as JSON.
fn lid_in(study: &Value) -> f64 {
    study["boundary_conditions"]["lid"]["velocity"]["value"]
        .as_f64()
        .unwrap()
}

/// Check that the lid of a study written as JSON moves at `speed`.
fn assert_lid(study: &Value, speed: f64) {
    let found = lid_in(study);
    assert!((found - speed).abs() < 1e-9, "the lid moves at {found}");
}

/// The study text the runner was given last, as JSON.
fn given(runner: &FixtureRunner) -> Value {
    serde_json::from_str(runner.last_study.as_deref().unwrap()).unwrap()
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

fn fast_lid() -> Scenario {
    variant("fast-lid", "base", vec![set(LID, json!(0.2))])
}

#[test]
fn a_setting_only_variant_reuses_the_parents_mesh_and_records_only_a_result() {
    let (path, mut runner) = solved_with("scn-setting", vec![fast_lid()]);
    let first = open(&path).unwrap().document.artifacts.runs[0].clone();

    let done = solve_scenario(&path, Some("fast-lid"), &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    assert_eq!(done.report.operation, "solve");
    let found = data(&done.report);
    assert_eq!(found.run.as_deref(), Some("run-2"));
    assert_eq!(found.recorded, ["result"]);
    assert_eq!(found.reused, ["mesh"]);
    assert_eq!(found.mesh_key, first.mesh.as_ref().unwrap().key.to_string());
    assert_ne!(
        found.result_key,
        first.result.as_ref().unwrap().key.to_string()
    );
    assert!(
        done.report
            .artifacts
            .iter()
            .any(|a| a.kind == "scenario" && a.name == "fast-lid")
    );
    assert_lid(&given(&runner), 0.2);

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    let second = &runs[1];
    assert_eq!(second.id, "run-2");
    assert_eq!(second.scenario.as_deref(), Some("fast-lid"));
    assert_eq!(runs[0], first);
    assert!(second.mesh.is_none());
    assert!(second.result.is_some());
    assert_eq!(second.provenance.mesh_key, first.provenance.mesh_key);
    assert_eq!(
        second.provenance.mesh_hash,
        first.mesh.as_ref().unwrap().blob
    );
    let settings = serde_json::to_value(&second.provenance.settings).unwrap();
    assert_lid(&settings, 0.2);
}

#[test]
fn a_geometry_variant_gets_its_own_mesh() {
    let fine = variant("fine", "base", vec![set(CELLS_X, json!(256))]);
    let (path, mut runner) = solved_with("scn-geometry", vec![fine]);
    let first = open(&path).unwrap().document.artifacts.runs[0].clone();

    let done = solve_scenario(&path, Some("fine"), &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    let found = data(&done.report);
    assert_eq!(found.run.as_deref(), Some("run-2"));
    assert_eq!(found.recorded, ["mesh", "result"]);
    assert!(found.reused.is_empty());
    assert_ne!(found.mesh_key, first.mesh.as_ref().unwrap().key.to_string());

    let runs = open(&path).unwrap().document.artifacts.runs;
    let second = &runs[1];
    assert_eq!(second.scenario.as_deref(), Some("fine"));
    assert!(second.mesh.is_some() && second.result.is_some());
    assert_ne!(second.provenance.mesh_hash, first.provenance.mesh_hash);
}

#[test]
fn a_variant_of_a_variant_is_solved_with_both_changes() {
    let fine = variant("fine", "base", vec![set(CELLS_X, json!(256))]);
    let fine_fast = variant("fine-fast", "fine", vec![set(LID, json!(0.3))]);
    let (path, mut runner) = solved_with("scn-chain", vec![fine, fine_fast]);
    passed(&solve_scenario(&path, Some("fine"), &mut runner).report);
    let runs = open(&path).unwrap().document.artifacts.runs;
    let fine_mesh = runs[1].mesh.as_ref().unwrap().clone();

    let done = solve_scenario(&path, Some("fine-fast"), &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 3);
    let found = data(&done.report);
    assert_eq!(found.run.as_deref(), Some("run-3"));
    assert_eq!(found.recorded, ["result"]);
    assert_eq!(found.reused, ["mesh"]);
    assert_eq!(found.mesh_key, fine_mesh.key.to_string());

    let study = given(&runner);
    assert_eq!(study["mesh"]["cells_x"], json!(256));
    assert_lid(&study, 0.3);
    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs[2].scenario.as_deref(), Some("fine-fast"));
    assert_eq!(runs[2].provenance.mesh_hash, fine_mesh.blob);
}

#[test]
fn the_second_solve_of_the_same_variant_starts_nothing() {
    let (path, mut runner) = solved_with("scn-again", vec![fast_lid()]);
    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);
    assert_eq!(runner.calls, 2);
    let before = fs::read(&path).unwrap();

    let done = solve_scenario(&path, Some("fast-lid"), &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    assert_eq!(done.line, Some(Line::NothingToRun));
    let found = data(&done.report);
    assert_eq!(found.run, None);
    assert!(found.recorded.is_empty());
    assert_eq!(found.reused, ["mesh", "result"]);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn solving_the_study_itself_is_unchanged() {
    let dir = scratch("scn-base");
    let mut seen = Vec::new();
    for (name, scenario) in [("plain", None), ("none", None), ("base", Some("base"))] {
        let path = new_document(&dir, name, CAVITY);
        let mut runner = FixtureRunner::new(&dir.join(format!("runs-{name}")));
        let done = if name == "plain" {
            solve_document(&path, &mut runner)
        } else {
            solve_scenario(&path, scenario, &mut runner)
        };
        passed(&done.report);
        assert!(done.report.artifacts.iter().all(|a| a.kind != "scenario"));
        let found = data(&done.report);
        seen.push((
            found.recorded.clone(),
            found.reused.clone(),
            found.mesh_key.clone(),
            found.result_key.clone(),
        ));
        let runs = open(&path).unwrap().document.artifacts.runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].scenario, None);
    }
    assert_eq!(seen[0], seen[1]);
    assert_eq!(seen[0], seen[2]);
    assert_eq!(seen[0].0, ["mesh", "result"]);
}

#[test]
fn a_rerun_of_a_variants_run_keeps_the_variant_and_its_settings() {
    let (path, mut runner) = solved_with("scn-rerun", vec![fast_lid()]);
    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);
    let runs = open(&path).unwrap().document.artifacts.runs;
    let original = runs[1].clone();
    assert_eq!(original.scenario.as_deref(), Some("fast-lid"));

    // Changing the variant afterwards does not change what a rerun of its old run repeats.
    edit_variants(&path, |list| list[0].overrides = vec![set(LID, json!(0.3))]);
    let action = JobAction::Rerun {
        run: "run-2".to_owned(),
        tolerance: Tolerance::same_image(),
        allow_tool_change: false,
    };
    let done = match plan_job(&path, action) {
        Ok(job) => job.run_with(&mut runner),
        Err(outcome) => *outcome,
    };
    passed(&done.report);
    assert_eq!(done.report.operation, "rerun");
    assert_eq!(runner.calls, 3);
    assert_lid(&given(&runner), 0.2);
    assert!(
        done.report
            .artifacts
            .iter()
            .any(|a| a.kind == "scenario" && a.name == "fast-lid")
    );

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 3);
    let repeated = &runs[2];
    assert_eq!(repeated.id, "run-3");
    assert_eq!(repeated.scenario.as_deref(), Some("fast-lid"));
    assert_eq!(repeated.rerun_of.as_deref(), Some("run-2"));
    assert_eq!(repeated.provenance.settings, original.provenance.settings);
    let comparison = repeated
        .comparison
        .as_ref()
        .expect("the comparison is kept");
    assert_eq!(comparison.verdict, Verdict::Reproduced);
    verify_runs(&runs).unwrap();
}

/// Solve the variant `fast-lid` of a fresh document while `during` is called as the case runs, and
/// say what the solve answered with.
fn solve_while(
    tag: &str,
    during: impl FnOnce(&Path) + 'static,
) -> (PathBuf, cfdcraft_ops::DocOutcome, FixtureRunner) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    edit_variants(&path, |list| list.push(fast_lid()));
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let target = path.clone();
    let mut during = Some(during);
    runner.hook = Some(Box::new(move || {
        if let Some(change) = during.take() {
            change(target.as_path());
        }
    }));
    let done = solve_scenario(&path, Some("fast-lid"), &mut runner);
    (path, done, runner)
}

fn refused_as_changed(done: &cfdcraft_ops::DocOutcome) {
    assert_eq!(
        done.report.class(),
        ExitClass::GateFailed,
        "{}",
        done.report.render_text()
    );
    let found = done
        .report
        .diagnostics
        .iter()
        .find(|d| d.code == "CFD-REPRO-009")
        .expect("the refusal is told");
    assert!(
        found
            .message
            .ends_with("the document changed while the case ran; solve it again"),
        "{}",
        found.message
    );
}

#[test]
fn a_variant_edited_while_its_case_ran_is_not_overwritten() {
    let (path, done, runner) = solve_while("scn-edited", |path| {
        edit_variants(path, |list| list[0].overrides = vec![set(LID, json!(0.25))]);
    });
    assert_eq!(runner.calls, 1);
    refused_as_changed(&done);

    let document = open(&path).unwrap().document;
    assert!(document.artifacts.runs.is_empty());
    assert_eq!(
        document.artifacts.scenarios[0].overrides,
        [set(LID, json!(0.25))]
    );
}

#[test]
fn a_variant_added_while_a_case_ran_also_counts_as_a_change() {
    let (path, done, runner) = solve_while("scn-added", |path| {
        edit_variants(path, |list| {
            list.push(variant("elsewhere", "base", vec![set(LID, json!(0.4))]));
        });
    });
    assert_eq!(runner.calls, 1);
    refused_as_changed(&done);

    let document = open(&path).unwrap().document;
    assert!(document.artifacts.runs.is_empty());
    assert_eq!(document.artifacts.scenarios.len(), 2);
}

#[test]
fn the_record_of_a_variant_run_passes_the_log_checks() {
    let (path, mut runner) = solved_with("scn-checks", vec![fast_lid()]);
    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);

    let document = open(&path).unwrap().document;
    verify_runs(&document.artifacts.runs).unwrap();
    check_scenario_records(&document.artifacts).unwrap();
    let copy = path.with_file_name("copy.cfdcraft");
    save(&document, &copy).unwrap();
    let again = open(&copy).unwrap().document;
    assert_eq!(again.artifacts.runs, document.artifacts.runs);
    assert_eq!(again.artifacts.scenarios, document.artifacts.scenarios);
}

#[test]
fn the_study_itself_is_still_refused_when_it_changes_while_the_case_ran() {
    let dir = scratch("scn-study-moved");
    let path = new_document(&dir, "cavity", CAVITY);
    edit_variants(&path, |list| list.push(fast_lid()));
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let (target, edited) = (path.clone(), with_lid_speed(0.15));
    runner.hook = Some(Box::new(move || {
        let saved = save_document(&edited, &target, true);
        assert_eq!(saved.report.class(), ExitClass::Passed);
    }));

    let done = solve_document(&path, &mut runner);
    assert_eq!(runner.calls, 1);
    refused_as_changed(&done);
    let document = open(&path).unwrap().document;
    assert!(document.artifacts.runs.is_empty());
    assert_eq!(document.artifacts.scenarios.len(), 1);
}
