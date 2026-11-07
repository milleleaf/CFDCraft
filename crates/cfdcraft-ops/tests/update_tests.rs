//! Updating the stale items of several documents: all of them in dependency order, some, or one,
//! and in an order the user chooses where the dependencies allow. A request that cannot be carried
//! out is refused before anything runs. No test needs a compute host or a container engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_doc::open;
use cfdcraft_model::Diagnostic;
use cfdcraft_ops::{
    DocOutcome, ExitClass, Report, Repro, UpdateData, document_status, save_document,
    solve_document, update_documents,
};
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

/// A runner for the update itself, which counts from zero.
fn runner_in(dir: &Path) -> FixtureRunner {
    FixtureRunner::new(&dir.join("update-runs"))
}

fn texts(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn update_of(report: &Report) -> &UpdateData {
    match &report.repro {
        Some(Repro::Update(data)) => data,
        other => panic!("expected the data of an update, found {other:?}"),
    }
}

/// The items an update made, in order.
fn made(report: &Report) -> Vec<&str> {
    update_of(report)
        .updated
        .iter()
        .map(|made| made.item.as_str())
        .collect()
}

/// The only finding of a report, checked to have this code.
fn sole<'a>(report: &'a Report, code: &str) -> &'a Diagnostic {
    assert_eq!(report.diagnostics.len(), 1, "{}", report.render_text());
    let found = &report.diagnostics[0];
    assert_eq!(found.code, code, "{}", report.render_text());
    found
}

fn states(path: &Path) -> (String, String) {
    match document_status(path).report.repro {
        Some(Repro::Status(data)) => {
            let data = *data;
            (data.mesh.state, data.result.state)
        }
        other => panic!("expected the status, found {other:?}"),
    }
}

fn snapshots(paths: &[PathBuf]) -> Vec<(Vec<u8>, Vec<String>)> {
    paths.iter().map(|path| snapshot(path)).collect()
}

/// An update asked for something it must refuse: class 2, one finding of code 008 with this
/// sentence, nothing run and nothing written.
fn assert_refused(
    done: &DocOutcome,
    sentence: &str,
    runner: &FixtureRunner,
    paths: &[PathBuf],
    before: &[(Vec<u8>, Vec<String>)],
) {
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    assert_eq!(sole(&done.report, "CFD-REPRO-008").message, sentence);
    assert_eq!(runner.calls, 0);
    assert_eq!(snapshots(paths), before);
}

#[test]
fn all_stale_items_are_updated_in_dependency_order() {
    let (paths, dir) = stale_documents("update-all");
    let mut runner = runner_in(&dir);

    let done = update_documents(&paths, None, None, &mut runner);
    passed(&done.report);
    assert_eq!(done.report.operation, "update");
    assert_eq!(runner.calls, 2);
    assert_eq!(
        made(&done.report),
        ["a:mesh", "a:result", "b:mesh", "b:result"]
    );
    let runs: Vec<&str> = update_of(&done.report)
        .updated
        .iter()
        .map(|made| made.run.as_str())
        .collect();
    assert_eq!(runs, ["run-2", "run-3", "run-2", "run-3"]);
    assert_eq!(
        done.line.as_ref().map(say).as_deref(),
        Some("Updated b:result in run-3.")
    );

    for path in &paths {
        let all = open(path).unwrap().document.artifacts.runs;
        assert_eq!(all.len(), 3);
        let (mesh_run, result_run) = (&all[1], &all[2]);
        assert!(mesh_run.mesh.is_some() && mesh_run.result.is_none());
        assert!(result_run.mesh.is_none() && result_run.result.is_some());
        assert_eq!(
            result_run.provenance.mesh_hash,
            mesh_run.mesh.as_ref().unwrap().blob
        );
        assert_eq!(states(path), ("fresh".to_owned(), "fresh".to_owned()));
    }
}

#[test]
fn one_item_can_be_updated_alone() {
    let (paths, dir) = stale_documents("update-one");
    let untouched = snapshot(&paths[1]);
    let mut runner = runner_in(&dir);

    let select = texts(&["a:mesh"]);
    let done = update_documents(&paths, Some(select.as_slice()), None, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 1);
    assert_eq!(made(&done.report), ["a:mesh"]);

    assert_eq!(open(&paths[0]).unwrap().document.artifacts.runs.len(), 2);
    assert_eq!(states(&paths[0]), ("fresh".to_owned(), "stale".to_owned()));
    assert_eq!(snapshot(&paths[1]), untouched);
}

#[test]
fn the_user_can_choose_the_order_across_documents() {
    let (paths, dir) = stale_documents("update-order");
    let mut runner = runner_in(&dir);

    let order = texts(&["b:mesh", "a:mesh", "b:result", "a:result"]);
    let done = update_documents(&paths, None, Some(order.as_slice()), &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    assert_eq!(
        made(&done.report),
        ["b:mesh", "a:mesh", "b:result", "a:result"]
    );
    for path in &paths {
        assert_eq!(states(path), ("fresh".to_owned(), "fresh".to_owned()));
    }
}

#[test]
fn a_result_cannot_be_updated_before_its_mesh() {
    let (paths, dir) = stale_documents("update-upstream");
    let before = snapshots(&paths);
    let mut runner = runner_in(&dir);

    let select = texts(&["a:result"]);
    let done = update_documents(&paths, Some(select.as_slice()), None, &mut runner);
    assert_refused(
        &done,
        "a:result is made from a:mesh, which is also out of date; update a:mesh too, and first.",
        &runner,
        &paths,
        &before,
    );
}

#[test]
fn an_order_that_breaks_a_dependency_or_misses_an_item_is_refused() {
    let (paths, dir) = stale_documents("update-bad-order");
    let before = snapshots(&paths);
    let mut runner = runner_in(&dir);

    let breaking = texts(&["a:result", "a:mesh", "b:mesh", "b:result"]);
    let done = update_documents(&paths, None, Some(breaking.as_slice()), &mut runner);
    assert_refused(
        &done,
        "a:result cannot come before a:mesh, which it is made from.",
        &runner,
        &paths,
        &before,
    );

    let missing = texts(&["a:mesh", "a:result", "b:mesh"]);
    let done = update_documents(&paths, None, Some(missing.as_slice()), &mut runner);
    assert_refused(
        &done,
        "The order must list each selected item exactly once.",
        &runner,
        &paths,
        &before,
    );
}

#[test]
fn an_item_that_is_up_to_date_is_refused() {
    let (paths, dir) = stale_documents("update-fresh-item");
    let mut runner = runner_in(&dir);
    let select = texts(&["a:mesh"]);
    passed(&update_documents(&paths, Some(select.as_slice()), None, &mut runner).report);
    assert_eq!(runner.calls, 1);
    let before = snapshots(&paths);

    let again = update_documents(&paths, Some(select.as_slice()), None, &mut runner);
    assert_eq!(again.report.class(), ExitClass::StudyInvalid);
    assert_eq!(
        sole(&again.report, "CFD-REPRO-008").message,
        "a:mesh is up to date, so there is nothing to update."
    );
    assert_eq!(runner.calls, 1);
    assert_eq!(snapshots(&paths), before);
}

#[test]
fn nothing_stale_is_nothing_to_update() {
    let dir = scratch("update-nothing");
    let path = new_document(&dir, "a", CAVITY);
    let mut solver = FixtureRunner::new(&dir.join("setup-runs"));
    passed(&solve_document(&path, &mut solver).report);
    let before = snapshot(&path);
    let mut runner = runner_in(&dir);

    let done = update_documents(std::slice::from_ref(&path), None, None, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 0);
    assert!(update_of(&done.report).updated.is_empty());
    assert_eq!(
        done.line.as_ref().map(say).as_deref(),
        Some("Nothing is out of date.")
    );
    assert_eq!(snapshot(&path), before);
}

#[test]
fn an_unknown_document_or_a_malformed_item_is_refused() {
    let (paths, dir) = stale_documents("update-unknown");
    let before = snapshots(&paths);
    let mut runner = runner_in(&dir);

    for text in ["zzz:mesh", "zzz"] {
        let select = texts(&[text]);
        let done = update_documents(&paths, Some(select.as_slice()), None, &mut runner);
        assert_refused(
            &done,
            &format!("There is no item {text}; an item is written name:mesh or name:result."),
            &runner,
            &paths,
            &before,
        );
    }
    let order = texts(&["zzz:result"]);
    let done = update_documents(&paths, None, Some(order.as_slice()), &mut runner);
    assert_refused(
        &done,
        "There is no item zzz:result; an item is written name:mesh or name:result.",
        &runner,
        &paths,
        &before,
    );
}

#[test]
fn two_documents_with_one_name_are_refused() {
    let dir = scratch("update-twins");
    let mut paths = Vec::new();
    for folder in ["x", "y"] {
        let inner = dir.join(folder);
        fs::create_dir_all(&inner).unwrap();
        paths.push(new_document(&inner, "a", CAVITY));
    }
    let before = snapshots(&paths);
    let mut runner = runner_in(&dir);

    let done = update_documents(&paths, None, None, &mut runner);
    assert_refused(
        &done,
        "Two documents are called a; give them different file names.",
        &runner,
        &paths,
        &before,
    );
}

#[test]
fn a_runner_failure_stops_the_update_and_keeps_what_was_done() {
    let (paths, dir) = stale_documents("update-failure");
    let untouched = snapshot(&paths[1]);
    let mut runner = runner_in(&dir);
    let mut failing = Report::new("run");
    failing.set_class(ExitClass::Infrastructure);
    failing.diagnostics.push(Diagnostic::error(
        "CFD-INFRA-001",
        "compute host",
        "",
        "The compute host could not be reached.",
    ));
    runner.fail_with = Some(failing.clone());
    runner.fail_on = Some(2);

    let done = update_documents(&paths, None, None, &mut runner);
    assert_eq!(runner.calls, 2);
    assert_eq!(done.report.class(), ExitClass::Infrastructure);
    assert_eq!(done.report.operation, "update");
    assert_eq!(done.report.diagnostics, failing.diagnostics);
    assert_eq!(made(&done.report), ["a:mesh", "a:result"]);

    assert_eq!(open(&paths[0]).unwrap().document.artifacts.runs.len(), 3);
    assert_eq!(states(&paths[0]), ("fresh".to_owned(), "fresh".to_owned()));
    assert_eq!(snapshot(&paths[1]), untouched);
}
