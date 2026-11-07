//! A stage whose stored result was pruned shows as pruned, a solve remakes it only when asked,
//! and a pack carries what is left. Nothing is ever pruned by anything but a prune that is applied.
//! No test needs a compute host or a container engine.

use std::path::{Path, PathBuf};

use cfdcraft_compare::Tolerance;
use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::disk_ops::{disk_use, prune};
use cfdcraft_ops::scenario_ops::{LabelUpdate, scenario_label, scenario_set};
use cfdcraft_ops::{
    ExitClass, JobAction, JobOptions, Report, Repro, StatusData, document_status, pack_document,
    plan_job, plan_scenario_job_with, plan_update_report, save_document, scenario_status,
    solve_document, solve_scenario, unpack_document, update_documents,
};
use cfdcraft_scenarios::Override;
use cfdcraft_store::Store;
use cfdcraft_text::Line;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";

/// The cavity solved once (`run-1`, mesh and result) with every stored result pruned. The
/// runner writes the same content for the same study.
fn pruned_study(tag: &str) -> (PathBuf, FixtureRunner) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = vec![0.0];
    passed(&solve_document(&path, &mut runner).report);
    passed(&prune(&path, None, true).report);
    (path, runner)
}

fn status(report: &Report) -> &StatusData {
    match &report.repro {
        Some(Repro::Status(data)) => data.as_ref(),
        other => panic!("expected the status, found {other:?}"),
    }
}

fn pruned_list(path: &Path) -> Vec<cfdcraft_store::ContentHash> {
    open(path).unwrap().document.artifacts.pruned
}

fn run_count(path: &Path) -> usize {
    open(path).unwrap().document.artifacts.runs.len()
}

#[test]
fn a_pruned_stage_shows_as_pruned_not_stale_or_missing() {
    let (path, _) = pruned_study("ps-status");

    let done = document_status(&path);
    assert_eq!(done.report.class(), ExitClass::Passed);
    assert!(codes(&done.report).is_empty(), "{:?}", codes(&done.report));
    let found = status(&done.report);
    for stage in [&found.mesh, &found.result] {
        assert_eq!(stage.state, "pruned");
        assert_eq!(stage.run.as_deref(), Some("run-1"));
    }
    assert_eq!(
        found.result.sentence,
        "The stored result of run-1 was pruned to free disk; its key numbers are kept."
    );
    // Nothing is out of date, so there is nothing to update.
    assert_eq!(done.line, Some(Line::NothingToUpdate));
}

#[test]
fn a_result_missing_without_being_pruned_is_still_blob_missing() {
    let dir = scratch("ps-missing");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    let runs = open(&path).unwrap().document.artifacts.runs;
    let blob = runs[0].result.as_ref().unwrap().blob;
    let store = Store::open(store_dir(&path)).unwrap();
    std::fs::remove_file(store.blob_path(&blob)).unwrap();

    let done = document_status(&path);
    assert_eq!(done.report.class(), ExitClass::Passed);
    let found = status(&done.report);
    assert_eq!(found.result.state, "blob_missing");
    assert_eq!(found.mesh.state, "fresh");
    assert_eq!(codes(&done.report), ["CFD-REPRO-002"]);
}

#[test]
fn solve_starts_nothing_for_a_pruned_stage_by_default() {
    let (path, mut runner) = pruned_study("ps-solve-default");
    let before = snapshot(&path);

    let done = solve_scenario(&path, None, &mut runner);
    assert_eq!(done.report.class(), ExitClass::Passed);
    assert_eq!(done.line, Some(Line::NothingToRun));
    assert!(data(&done.report).run.is_none());
    assert!(codes(&done.report).is_empty());
    assert_eq!(runner.calls, 1);
    assert_eq!(run_count(&path), 1);
    assert_eq!(snapshot(&path), before);
}

#[test]
fn remake_pruned_runs_the_stage_again() {
    let (path, mut runner) = pruned_study("ps-remake");
    assert!(!pruned_list(&path).is_empty());

    let options = JobOptions {
        remake_pruned: true,
    };
    let job = plan_scenario_job_with(&path, None, JobAction::Solve, &options).expect("a plan");
    let done = job.run_with(&mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    assert_eq!(data(&done.report).recorded, ["mesh", "result"]);
    assert!(
        codes(&done.report).is_empty(),
        "no warning for a pruned stage that is asked for"
    );

    let document = open(&path).unwrap().document;
    assert_eq!(document.artifacts.runs.len(), 2);
    let store = Store::open(store_dir(&path)).unwrap();
    for hash in document.artifacts.blobs.iter().map(|b| b.hash) {
        assert!(store.contains(&hash));
    }
    // The fixture wrote the same content, so the same results are stored again.
    assert!(document.artifacts.pruned.is_empty());
    let after = document_status(&path);
    assert_eq!(status(&after.report).result.state, "fresh");
    assert_eq!(after.line, Some(Line::NothingToUpdate));
}

#[test]
fn a_remade_result_that_differs_leaves_the_old_hash_pruned() {
    let (path, mut runner) = pruned_study("ps-remake-differs");
    runner.shifts = vec![0.0, 0.05];
    let old = open(&path).unwrap().document.artifacts.runs[0]
        .result
        .as_ref()
        .unwrap()
        .blob;

    let options = JobOptions {
        remake_pruned: true,
    };
    let job = plan_scenario_job_with(&path, None, JobAction::Solve, &options).expect("a plan");
    passed(&job.run_with(&mut runner).report);

    let document = open(&path).unwrap().document;
    let new = document.artifacts.runs[1].result.as_ref().unwrap().blob;
    assert_ne!(new, old);
    assert!(document.artifacts.pruned.contains(&old));
    assert!(!document.artifacts.pruned.contains(&new));
    assert!(document.artifacts.blobs.iter().any(|b| b.hash == new));
    let store = Store::open(store_dir(&path)).unwrap();
    assert!(store.contains(&new));
    assert!(!store.contains(&old));

    let done = document_status(&path);
    let found = status(&done.report);
    assert_eq!(found.result.state, "fresh");
    assert_eq!(found.result.run.as_deref(), Some("run-2"));
    assert_eq!(found.mesh.state, "fresh");
}

#[test]
fn update_does_not_remake_pruned_stages() {
    let (path, mut runner) = pruned_study("ps-update");
    let before = snapshot(&path);

    let planned = plan_update_report(std::slice::from_ref(&path), None, None);
    passed(&planned.report);
    match &planned.report.repro {
        Some(Repro::Plan(plan)) => {
            assert!(plan.pending.is_empty());
            assert!(plan.order.is_empty());
        }
        other => panic!("expected a plan, found {other:?}"),
    }
    assert_eq!(planned.line, Some(Line::NothingToUpdate));

    let done = update_documents(std::slice::from_ref(&path), None, None, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 1);
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_stale_stage_over_a_pruned_mesh_is_remade_with_its_mesh_only_when_asked() {
    // The mesh is pruned and the lid changes, so the result is out of date and the mesh is not.
    let faster = |tag: &str| {
        let (path, runner) = pruned_study(tag);
        passed(&save_document(&with_lid_speed(0.2), &path, true).report);
        let found = document_status(&path);
        let stages = status(&found.report);
        assert_eq!(stages.mesh.state, "pruned");
        assert_eq!(stages.result.state, "stale");
        (path, runner)
    };

    let (path, mut runner) = faster("ps-stale-default");
    let mesh = open(&path).unwrap().document.artifacts.runs[0]
        .mesh
        .as_ref()
        .unwrap()
        .blob;
    let done = solve_document(&path, &mut runner);
    passed(&done.report);
    assert_eq!(data(&done.report).recorded, ["result"]);
    assert_eq!(runner.calls, 2);
    let runs = open(&path).unwrap().document.artifacts.runs;
    assert!(runs[1].mesh.is_none());
    assert_eq!(runs[1].provenance.mesh_hash, mesh);
    assert!(pruned_list(&path).contains(&mesh));

    let (path, mut runner) = faster("ps-stale-remake");
    let options = JobOptions {
        remake_pruned: true,
    };
    let job = plan_scenario_job_with(&path, None, JobAction::Solve, &options).expect("a plan");
    let done = job.run_with(&mut runner);
    passed(&done.report);
    assert_eq!(data(&done.report).recorded, ["mesh", "result"]);
    assert!(!pruned_list(&path).contains(&mesh));
}

#[test]
fn a_variant_status_shows_the_parents_pruned_mesh_as_pruned() {
    let dir = scratch("ps-variant");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    let lid = Override::Set {
        setting: LID.to_owned(),
        value: json!(0.2),
    };
    passed(&scenario_set(&path, "fast-lid", Some("base"), &[lid], None).report);
    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);
    passed(&prune(&path, None, true).report);

    let done = scenario_status(&path, Some("fast-lid"));
    assert_eq!(done.report.class(), ExitClass::Passed);
    assert!(codes(&done.report).is_empty());
    let found = status(&done.report);
    assert_eq!(found.mesh.state, "pruned");
    assert_eq!(found.mesh.run.as_deref(), Some("run-1"));
    assert_eq!(found.result.state, "pruned");
    assert_eq!(found.result.run.as_deref(), Some("run-2"));
}

#[test]
fn pack_after_prune_through_the_operation() {
    let dir = scratch("ps-pack");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = vec![0.0, 0.05];
    passed(&solve_document(&path, &mut runner).report);
    let lid = Override::Set {
        setting: LID.to_owned(),
        value: json!(0.2),
    };
    passed(&scenario_set(&path, "fast-lid", Some("base"), &[lid], None).report);
    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);
    let pin = LabelUpdate {
        pinned: Some(true),
        ..LabelUpdate::default()
    };
    passed(&scenario_label(&path, "fast-lid", &pin).report);
    passed(&prune(&path, None, true).report);
    // The study's result is gone; the mesh and the result of the pinned variant are stored.
    assert_eq!(pruned_list(&path).len(), 1);

    let packed = dir.join("cavity.cfdpack");
    let done = pack_document(&path, &packed);
    passed(&done.report);
    assert_eq!(
        done.line,
        Some(Line::Packed {
            count: 2,
            path: packed.display().to_string(),
        })
    );

    let there = dir.join("unpacked");
    let done = unpack_document(&packed, &there);
    passed(&done.report);
    let target = there.join("cavity.cfdcraft");
    assert_eq!(
        open(&target).unwrap().document.artifacts.pruned,
        pruned_list(&path)
    );
    assert!(
        done.report
            .artifacts
            .iter()
            .filter(|a| a.kind == "stored_result")
            .count()
            == 2
    );

    let found = document_status(&target);
    assert_eq!(found.report.class(), ExitClass::Passed);
    assert!(codes(&found.report).is_empty());
    assert_eq!(status(&found.report).mesh.state, "fresh");
    assert_eq!(status(&found.report).result.state, "pruned");
    let variant = scenario_status(&target, Some("fast-lid"));
    assert_eq!(status(&variant.report).result.state, "fresh");
}

#[test]
fn nothing_is_pruned_automatically() {
    let dir = scratch("ps-never");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);

    // A change, an update (planned and applied), a status, a rerun and a second solve.
    passed(&save_document(&with_lid_speed(0.2), &path, true).report);
    passed(&plan_update_report(std::slice::from_ref(&path), None, None).report);
    passed(&update_documents(std::slice::from_ref(&path), None, None, &mut runner).report);
    passed(&document_status(&path).report);
    let again = JobAction::Rerun {
        run: "run-1".to_owned(),
        tolerance: Tolerance::same_image(),
        allow_tool_change: false,
    };
    let rerun = plan_job(&path, again)
        .expect("a plan")
        .run_with(&mut runner);
    passed(&rerun.report);
    passed(&solve_document(&path, &mut runner).report);
    assert_eq!(run_count(&path), 3);

    // Reading what is stored and planning a prune remove nothing either.
    passed(&disk_use(&path).report);
    passed(&prune(&path, None, false).report);

    // And neither do packing and unpacking.
    let packed = dir.join("cavity.cfdpack");
    passed(&pack_document(&path, &packed).report);
    let there = dir.join("unpacked");
    passed(&unpack_document(&packed, &there).report);

    for target in [path.clone(), there.join("cavity.cfdcraft")] {
        let document = open(&target).unwrap().document;
        assert!(document.artifacts.pruned.is_empty());
        assert!(!document.artifacts.blobs.is_empty());
        let store = Store::open(store_dir(&target)).unwrap();
        for blob in &document.artifacts.blobs {
            assert!(store.contains(&blob.hash));
        }
    }
}
