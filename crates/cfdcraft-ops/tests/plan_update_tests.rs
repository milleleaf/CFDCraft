//! The read-only plan of an update: the items waiting, the order an update would take them in, and
//! the same refusals an update gives. Planning never runs a case and never writes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::{
    ExitClass, PlanData, Report, Repro, plan_update_report, save_document, solve_document,
    update_documents,
};
use cfdcraft_store::Store;
use serde_json::Value;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

/// The cavity with a finer mesh in x, so both stages are out of date for a document solved before.
fn finer() -> String {
    edit(CAVITY, |study| study["mesh"]["cells_x"] = Value::from(256))
}

/// Documents `a` and `b`, each solved once and then given a finer mesh: both stages of both are
/// out of date.
fn stale_documents(tag: &str) -> (Vec<PathBuf>, PathBuf) {
    let dir = scratch(tag);
    let mut runner = FixtureRunner::new(&dir.join("setup-runs"));
    let mut paths = Vec::new();
    for name in ["a", "b"] {
        let path = new_document(&dir, name, CAVITY);
        passed(&solve_document(&path, &mut runner).report);
        passed(&save_document(&finer(), &path, true).report);
        paths.push(path);
    }
    (paths, dir)
}

fn texts(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn plan_of(report: &Report) -> &PlanData {
    match &report.repro {
        Some(Repro::Plan(data)) => data,
        other => panic!("expected a plan, found {other:?}"),
    }
}

/// Every file under `root` with its bytes.
fn contents(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let name = path.strip_prefix(root).unwrap().display().to_string();
                found.insert(name, fs::read(&path).unwrap());
            }
        }
    }
    found
}

/// A plan or an update asked for something it must refuse: class 2, one finding of code 008.
fn refusal(report: &Report) -> String {
    assert_eq!(report.class(), ExitClass::StudyInvalid);
    assert_eq!(report.diagnostics.len(), 1, "{}", report.render_text());
    assert_eq!(report.diagnostics[0].code, "CFD-REPRO-008");
    report.diagnostics[0].message.clone()
}

#[test]
fn the_plan_lists_the_pending_items_and_the_default_order() {
    let (paths, dir) = stale_documents("plan-default");
    let before = contents(&dir);

    let done = plan_update_report(&paths, None, None);
    passed(&done.report);
    assert_eq!(done.report.operation, "plan-update");
    let data = plan_of(&done.report);
    assert_eq!(data.pending, ["a:mesh", "a:result", "b:mesh", "b:result"]);
    assert_eq!(data.order, data.pending);
    assert!(done.line.is_none());
    assert_eq!(contents(&dir), before);
}

#[test]
fn the_plan_shows_the_order_the_user_asked_for() {
    let (paths, _) = stale_documents("plan-order");

    let order = texts(&["b:mesh", "a:mesh", "b:result", "a:result"]);
    let done = plan_update_report(&paths, None, Some(order.as_slice()));
    passed(&done.report);
    let data = plan_of(&done.report);
    assert_eq!(data.order, order);
    assert_eq!(data.pending, ["a:mesh", "a:result", "b:mesh", "b:result"]);

    let select = texts(&["b:mesh"]);
    let done = plan_update_report(&paths, Some(select.as_slice()), None);
    passed(&done.report);
    assert_eq!(plan_of(&done.report).order, ["b:mesh"]);
}

#[test]
fn the_plan_refuses_what_an_update_refuses() {
    let (paths, dir) = stale_documents("plan-refuses");
    let mut runner = FixtureRunner::new(&dir.join("update-runs"));

    // Each request with the sentence both operations answer it with.
    let upstream = texts(&["a:result"]);
    let breaking = texts(&["a:result", "a:mesh", "b:mesh", "b:result"]);
    let missing = texts(&["a:mesh", "a:result", "b:mesh"]);
    let unknown = texts(&["zzz:mesh"]);
    let cases = [
        (
            Some(upstream.as_slice()),
            None,
            "a:result is made from a:mesh, which is also out of date; update a:mesh too, and first.",
        ),
        (
            None,
            Some(breaking.as_slice()),
            "a:result cannot come before a:mesh, which it is made from.",
        ),
        (
            None,
            Some(missing.as_slice()),
            "The order must list each selected item exactly once.",
        ),
        (
            Some(unknown.as_slice()),
            None,
            "There is no item zzz:mesh; an item is written name:mesh or name:result.",
        ),
    ];
    let before = contents(&dir);
    for (select, order, sentence) in cases {
        let planned = plan_update_report(&paths, select, order);
        assert_eq!(refusal(&planned.report), sentence);
        let updated = update_documents(&paths, select, order, &mut runner);
        assert_eq!(refusal(&updated.report), sentence);
    }
    assert_eq!(runner.calls, 0);
    assert_eq!(contents(&dir), before);

    // An item that is up to date is refused the same way, once its mesh has been updated.
    let mesh = texts(&["a:mesh"]);
    passed(&update_documents(&paths, Some(mesh.as_slice()), None, &mut runner).report);
    assert_eq!(runner.calls, 1);
    let before = contents(&dir);
    let sentence = "a:mesh is up to date, so there is nothing to update.";
    let planned = plan_update_report(&paths, Some(mesh.as_slice()), None);
    assert_eq!(refusal(&planned.report), sentence);
    let updated = update_documents(&paths, Some(mesh.as_slice()), None, &mut runner);
    assert_eq!(refusal(&updated.report), sentence);
    assert_eq!(runner.calls, 1);
    assert_eq!(contents(&dir), before);
}

#[test]
fn nothing_pending_is_an_empty_plan() {
    let dir = scratch("plan-nothing");
    let path = new_document(&dir, "a", CAVITY);
    let mut solver = FixtureRunner::new(&dir.join("setup-runs"));
    passed(&solve_document(&path, &mut solver).report);

    let done = plan_update_report(std::slice::from_ref(&path), None, None);
    passed(&done.report);
    let data = plan_of(&done.report);
    assert!(data.pending.is_empty());
    assert!(data.order.is_empty());
    assert_eq!(
        done.line.as_ref().map(say).as_deref(),
        Some("Nothing is out of date.")
    );
}

#[test]
fn a_plan_followed_by_an_update_with_that_order_does_the_same() {
    let (paths, dir) = stale_documents("plan-then-update");
    let mut runner = FixtureRunner::new(&dir.join("update-runs"));

    let wanted = texts(&["b:mesh", "a:mesh", "b:result", "a:result"]);
    let planned = plan_update_report(&paths, None, Some(wanted.as_slice()));
    passed(&planned.report);
    let order = plan_of(&planned.report).order.clone();
    assert_eq!(order, wanted);

    let done = update_documents(&paths, None, Some(order.as_slice()), &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    match &done.report.repro {
        Some(Repro::Update(data)) => {
            let made: Vec<&str> = data.updated.iter().map(|u| u.item.as_str()).collect();
            assert_eq!(made, order);
        }
        other => panic!("expected the data of an update, found {other:?}"),
    }

    let after = plan_update_report(&paths, None, None);
    assert!(plan_of(&after.report).pending.is_empty());
}

#[test]
fn the_plan_never_writes() {
    let (paths, dir) = stale_documents("plan-readonly");
    let before = contents(&dir);

    let order = texts(&["b:mesh", "a:mesh", "b:result", "a:result"]);
    let select = texts(&["a:result"]);
    let unknown = texts(&["zzz"]);
    let calls = [
        (None, None),
        (None, Some(order.as_slice())),
        (Some(select.as_slice()), None),
        (Some(unknown.as_slice()), None),
    ];
    for (select, order) in calls {
        let _ = plan_update_report(&paths, select, order);
        assert_eq!(contents(&dir), before);
    }
}

/// A document solved once whose mesh blob was then deleted from the store.
fn without_mesh_blob(tag: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let path = new_document(&dir, "a", CAVITY);
    let mut setup = FixtureRunner::new(&dir.join("setup-runs"));
    passed(&solve_document(&path, &mut setup).report);
    let first = open(&path).unwrap().document.artifacts.runs[0].clone();
    let store = Store::open(store_dir(&path)).unwrap();
    fs::remove_file(store.blob_path(&first.mesh.as_ref().unwrap().blob)).unwrap();
    (path, dir)
}

#[test]
fn a_fresh_stage_whose_blob_is_gone_is_planned_and_repaired_by_an_update() {
    let (path, dir) = without_mesh_blob("plan-blob-missing");
    let paths = vec![path.clone()];

    let done = plan_update_report(&paths, None, None);
    passed(&done.report);
    assert_eq!(plan_of(&done.report).pending, ["a:mesh"]);
    assert_eq!(plan_of(&done.report).order, ["a:mesh"]);
    assert!(done.line.is_none());

    let mut runner = FixtureRunner::new(&dir.join("update-runs"));
    let item = texts(&["a:mesh"]);
    let done = update_documents(&paths, Some(item.as_slice()), None, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 1);
    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    assert!(runs[1].mesh.is_some() && runs[1].result.is_none());
    let store = Store::open(store_dir(&path)).unwrap();
    assert!(store.contains(&runs[1].mesh.as_ref().unwrap().blob));

    let again = plan_update_report(&paths, None, None);
    assert!(plan_of(&again.report).pending.is_empty());
}

#[test]
fn a_stale_result_over_a_mesh_whose_blob_is_gone_plans_the_mesh_first() {
    let (path, dir) = without_mesh_blob("plan-blob-missing-upstream");
    // A new lid speed changes the result recipe and leaves the mesh key alone.
    passed(&save_document(&with_lid_speed(0.2), &path, true).report);
    let paths = vec![path.clone()];

    let done = plan_update_report(&paths, None, None);
    passed(&done.report);
    assert_eq!(plan_of(&done.report).pending, ["a:mesh", "a:result"]);
    let only_result = texts(&["a:result"]);
    let refused = plan_update_report(&paths, Some(only_result.as_slice()), None);
    refusal(&refused.report);

    let mut runner = FixtureRunner::new(&dir.join("update-runs"));
    let done = update_documents(&paths, None, None, &mut runner);
    passed(&done.report);
    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 3);
    assert!(runs[1].mesh.is_some() && runs[2].result.is_some());
}
