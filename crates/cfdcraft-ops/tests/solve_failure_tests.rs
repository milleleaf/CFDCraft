//! Solving that goes wrong records nothing: a runner that fails, a check that does not pass, an
//! incomplete results folder, a document that changed meanwhile, a run history that disagrees with
//! itself. Every refusal leaves the document file byte for byte as it was. No test needs a compute
//! host or a container engine.

use std::cell::RefCell;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::rc::Rc;

use cfdcraft_doc::{Document, open, save, store_dir};
use cfdcraft_model::{Diagnostic, Severity, Study};
use cfdcraft_ops::{ExitClass, Report, link_file, save_document, solve_document};
use cfdcraft_store::Store;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const RECORD_FAILURE: &str = "The output of the run could not be recorded: ";

/// The only finding of a report, checked to have this code.
fn sole<'a>(report: &'a Report, code: &str) -> &'a Diagnostic {
    assert_eq!(report.diagnostics.len(), 1, "{}", report.render_text());
    let found = &report.diagnostics[0];
    assert_eq!(found.code, code, "{}", report.render_text());
    found
}

#[test]
fn a_runner_failure_is_returned_as_it_came_and_nothing_is_recorded() {
    let dir = scratch("runner-failure");
    let path = new_document(&dir, "cavity", CAVITY);
    let before = fs::read(&path).unwrap();
    let mut failing = Report::new("run");
    failing.set_class(ExitClass::Infrastructure);
    failing.diagnostics.push(Diagnostic::error(
        "CFD-INFRA-001",
        "compute host",
        "",
        "The compute host could not be reached.",
    ));
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.fail_with = Some(failing.clone());

    let done = solve_document(&path, &mut runner);
    assert_eq!(runner.calls, 1);
    assert_eq!(done.report.class(), ExitClass::Infrastructure);
    assert_eq!(done.report.operation, "solve");
    assert_eq!(done.report.diagnostics, failing.diagnostics);
    assert!(done.line.is_none());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!store_dir(&path).exists());
}

#[test]
fn a_failed_check_records_nothing_and_is_class_1() {
    let dir = scratch("failed-check");
    let path = new_document(&dir, "cavity", CAVITY);
    let before = fs::read(&path).unwrap();
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.failing_check = Some("ghia".to_owned());

    let done = solve_document(&path, &mut runner);
    assert_eq!(done.report.class(), ExitClass::GateFailed);
    let found = sole(&done.report, "CFD-REPRO-009");
    assert_eq!(
        found.message,
        format!("{RECORD_FAILURE}the check \"ghia\" did not pass")
    );
    assert!(done.report.repro.is_none());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!store_dir(&path).exists());
}

#[test]
fn an_incomplete_results_folder_records_nothing_and_is_class_1() {
    let dir = scratch("incomplete");
    let path = new_document(&dir, "cavity", CAVITY);
    let before = fs::read(&path).unwrap();
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.omit = Some("log.simpleFoam");

    let done = solve_document(&path, &mut runner);
    assert_eq!(done.report.class(), ExitClass::GateFailed);
    let found = sole(&done.report, "CFD-REPRO-009");
    assert!(
        found.message.starts_with(RECORD_FAILURE),
        "{}",
        found.message
    );
    assert!(
        found.message.contains("log.simpleFoam"),
        "{}",
        found.message
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(open(&path).unwrap().document.artifacts.runs.is_empty());
}

#[test]
fn a_blob_missing_from_the_store_counts_as_out_of_date_and_warns() {
    let dir = scratch("blob-missing");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    let first = open(&path).unwrap().document.artifacts.runs[0].clone();
    let result = first.result.as_ref().unwrap();
    let store = Store::open(store_dir(&path)).unwrap();
    fs::remove_file(store.blob_path(&result.blob)).unwrap();
    assert!(!store.contains(&result.blob));

    let done = solve_document(&path, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    assert_eq!(codes(&done.report), ["CFD-REPRO-002"]);
    let found = data(&done.report);
    assert_eq!(found.recorded, ["result"]);
    assert_eq!(found.reused, ["mesh"]);

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0], first);
    assert!(runs[1].mesh.is_none());
    assert_eq!(runs[1].result.as_ref().unwrap().key, result.key);
    assert!(store.contains(&result.blob));

    let again = solve_document(&path, &mut runner);
    passed(&again.report);
    assert_eq!(runner.calls, 2);
    assert!(again.report.diagnostics.is_empty());
}

#[test]
fn a_pack_is_not_solved() {
    let dir = scratch("pack");
    let path = dir.join("x.cfdpack");
    let mut runner = FixtureRunner::new(&dir.join("runs"));

    let done = solve_document(&path, &mut runner);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    sole(&done.report, "CFD-DOC-009");
    assert_eq!(runner.calls, 0);
    assert!(!path.exists());
}

/// The file with `fragment` replaced by `changed` in its `artifacts.json`.
fn damage_index(path: &Path, fragment: &str, changed: &str) {
    let mut zip = ZipArchive::new(Cursor::new(fs::read(path).unwrap())).unwrap();
    let mut member = |name: &str| {
        let mut bytes = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
        bytes
    };
    let manifest = member("manifest.json");
    let study = member("study.json");
    let index = String::from_utf8(member("artifacts.json")).unwrap();
    let thumbnail = member("thumbnail.png");
    assert_eq!(
        index.matches(fragment).count(),
        1,
        "the fragment is in the index once"
    );
    let damaged = index.replace(fragment, changed);
    assert_ne!(damaged, index);

    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", manifest),
        ("study.json", study),
        ("artifacts.json", damaged.into_bytes()),
        ("thumbnail.png", thumbnail),
    ] {
        out.start_file(name, SimpleFileOptions::default()).unwrap();
        out.write_all(&bytes).unwrap();
    }
    fs::write(path, out.finish().unwrap().into_inner()).unwrap();
}

#[test]
fn an_inconsistent_run_log_is_refused_before_anything_runs() {
    let dir = scratch("inconsistent");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    // The recorded mesh no longer matches the key it was recorded with.
    damage_index(&path, "\"x\": 128,", "\"x\": 129,");
    let before = fs::read(&path).unwrap();

    let done = solve_document(&path, &mut runner);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    let found = sole(&done.report, "CFD-REPRO-003");
    assert!(found.message.contains("run-1"), "{}", found.message);
    assert_eq!(runner.calls, 1);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn a_missing_or_unreadable_file_is_a_class_2_failure() {
    let dir = scratch("unreadable");
    let mut runner = FixtureRunner::new(&dir.join("runs"));

    let missing = solve_document(&dir.join("absent.cfdcraft"), &mut runner);
    assert_eq!(missing.report.class(), ExitClass::StudyInvalid);
    sole(&missing.report, "CFD-DOC-001");

    let plain = dir.join("plain.cfdcraft");
    fs::write(&plain, CAVITY).unwrap();
    let done = solve_document(&plain, &mut runner);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    sole(&done.report, "CFD-DOC-001");
    assert_eq!(fs::read_to_string(&plain).unwrap(), CAVITY);
    assert_eq!(runner.calls, 0);
}

#[test]
fn a_study_that_fails_the_gates_is_not_solved() {
    let dir = scratch("gates");
    let path = dir.join("empty-mesh.cfdcraft");
    let mut study = Study::from_json(CAVITY).unwrap();
    study.mesh.cells_x = 0;
    save(&Document::new(study), &path).unwrap();
    let before = fs::read(&path).unwrap();
    let mut runner = FixtureRunner::new(&dir.join("runs"));

    let done = solve_document(&path, &mut runner);
    assert_eq!(
        done.report.class(),
        ExitClass::StudyInvalid,
        "{}",
        done.report.render_text()
    );
    assert!(
        done.report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    );
    assert_eq!(done.report.operation, "solve");
    assert_eq!(runner.calls, 0);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn a_document_changed_while_the_case_ran_is_not_overwritten() {
    let dir = scratch("changed-meanwhile");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let left = Rc::new(RefCell::new(Vec::new()));
    let (seen, target, edited) = (Rc::clone(&left), path.clone(), with_lid_speed(0.2));
    runner.hook = Some(Box::new(move || {
        let saved = save_document(&edited, &target, true);
        assert_eq!(saved.report.class(), ExitClass::Passed);
        *seen.borrow_mut() = fs::read(&target).unwrap();
    }));

    let done = solve_document(&path, &mut runner);
    assert_eq!(runner.calls, 1);
    assert_eq!(done.report.class(), ExitClass::GateFailed);
    let found = sole(&done.report, "CFD-REPRO-009");
    assert_eq!(
        found.message,
        format!("{RECORD_FAILURE}the document changed while the case ran; solve it again")
    );
    assert_eq!(fs::read(&path).unwrap(), *left.borrow());
    assert!(open(&path).unwrap().document.artifacts.runs.is_empty());
}

#[test]
fn a_link_added_while_the_case_ran_is_kept() {
    let dir = scratch("link-meanwhile");
    let path = new_document(&dir, "cavity", CAVITY);
    let notes = dir.join("notes.txt");
    fs::write(&notes, "a note").unwrap();
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let target = path.clone();
    runner.hook = Some(Box::new(move || {
        let linked = link_file(&target, &notes, "notes");
        assert_eq!(linked.report.class(), ExitClass::Passed);
    }));

    let done = solve_document(&path, &mut runner);
    passed(&done.report);
    let document = open(&path).unwrap().document;
    assert_eq!(document.artifacts.runs.len(), 1);
    assert_eq!(document.artifacts.links.len(), 1);
    assert_eq!(document.artifacts.links[0].id, "notes");
}
