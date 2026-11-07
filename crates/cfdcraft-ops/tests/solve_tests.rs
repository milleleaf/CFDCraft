//! Solving a document with a fixture runner: what is recorded, what is reused, and what a changed
//! setting runs again. No test here needs a compute host or a container engine, so they run on
//! every platform.

use std::fs;
use std::io::{Cursor, Write};

use cfdcraft_backend_esi::Dialect;
use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::{save_document, solve_document, stage_keys, tool_identity};
use cfdcraft_store::Store;
use cfdcraft_text::Line;
use serde_json::Value;
use zip::{ZipWriter, write::SimpleFileOptions};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");

fn artifact(report: &cfdcraft_ops::Report, kind: &str) -> String {
    report
        .artifacts
        .iter()
        .find(|a| a.kind == kind)
        .map(|a| a.name.clone())
        .unwrap_or_default()
}

#[test]
fn solving_a_fresh_document_records_run_1() {
    let dir = scratch("fresh");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));

    let done = solve_document(&path, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 1);
    assert_eq!(done.report.operation, "solve");
    let line = done.line.as_ref().expect("a sentence for a person");
    assert_eq!(
        line,
        &Line::RunRecorded {
            run: "run-1".to_owned(),
            stages: "mesh, result".to_owned(),
        }
    );
    assert_eq!(say(line), "Recorded run-1: mesh, result.");
    assert_eq!(
        artifact(&done.report, "document"),
        path.display().to_string()
    );

    let document = open(&path).unwrap().document;
    assert_eq!(document.artifacts.runs.len(), 1);
    let run = &document.artifacts.runs[0];
    assert_eq!(run.id, "run-1");
    let mesh = run.mesh.as_ref().expect("the mesh is recorded");
    let result = run.result.as_ref().expect("the result is recorded");
    let listed: Vec<_> = document.artifacts.blobs.iter().map(|b| b.hash).collect();
    assert_eq!(listed, [mesh.blob, result.blob]);

    let store = Store::open(store_dir(&path)).unwrap();
    store.verify(&mesh.blob).unwrap();
    store.verify(&result.blob).unwrap();

    let found = data(&done.report);
    assert_eq!(found.run.as_deref(), Some("run-1"));
    assert_eq!(found.recorded, ["mesh", "result"]);
    assert!(found.reused.is_empty());
    let keys = stage_keys(CAVITY);
    assert_eq!(found.mesh_key, artifact(&keys, "mesh_key"));
    assert_eq!(found.result_key, artifact(&keys, "result_key"));
    assert_eq!(mesh.key.to_string(), found.mesh_key);
    assert_eq!(result.key.to_string(), found.result_key);
}

#[test]
fn solving_again_reuses_everything_and_runs_nothing() {
    let dir = scratch("again");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    let before = fs::read(&path).unwrap();

    let done = solve_document(&path, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 1);
    let line = done.line.as_ref().unwrap();
    assert_eq!(line, &Line::NothingToRun);
    assert_eq!(
        say(line),
        "Nothing to run: the mesh and the result are up to date."
    );
    assert!(done.report.diagnostics.is_empty());
    let found = data(&done.report);
    assert_eq!(found.run, None);
    assert!(found.recorded.is_empty());
    assert_eq!(found.reused, ["mesh", "result"]);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn a_changed_lid_speed_makes_a_new_run_that_reuses_the_mesh() {
    let dir = scratch("lid");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    let first = open(&path).unwrap().document.artifacts.runs[0].clone();

    passed(&save_document(&with_lid_speed(0.2), &path, true).report);
    let done = solve_document(&path, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    let found = data(&done.report);
    assert_eq!(found.recorded, ["result"]);
    assert_eq!(found.reused, ["mesh"]);

    let document = open(&path).unwrap().document;
    assert_eq!(document.artifacts.runs.len(), 2);
    assert_eq!(document.artifacts.runs[0], first);
    let second = &document.artifacts.runs[1];
    assert_eq!(second.id, "run-2");
    assert!(second.mesh.is_none());
    let result = second.result.as_ref().expect("a new result");
    let first_result = first.result.as_ref().unwrap();
    assert_ne!(result.key, first_result.key);
    assert_ne!(result.blob, first_result.blob);
    assert_eq!(
        second.provenance.mesh_hash,
        first.mesh.as_ref().unwrap().blob
    );
    assert_eq!(second.provenance.mesh_key, first.provenance.mesh_key);

    let store = Store::open(store_dir(&path)).unwrap();
    assert!(store.contains(&first_result.blob));
    assert!(store.contains(&result.blob));
    assert_eq!(document.artifacts.blobs.len(), 3);
}

#[test]
fn a_changed_cell_count_makes_a_new_mesh_and_a_new_result() {
    let dir = scratch("cells");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);

    let finer = edit(CAVITY, |study| study["mesh"]["cells_x"] = Value::from(256));
    passed(&save_document(&finer, &path, true).report);
    let done = solve_document(&path, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    assert_eq!(data(&done.report).recorded, ["mesh", "result"]);

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    let (first, second) = (&runs[0], &runs[1]);
    let (first_mesh, second_mesh) = (first.mesh.as_ref().unwrap(), second.mesh.as_ref().unwrap());
    assert_ne!(first_mesh.key, second_mesh.key);
    assert_ne!(first_mesh.blob, second_mesh.blob);
    assert_ne!(
        first.result.as_ref().unwrap().key,
        second.result.as_ref().unwrap().key
    );
    assert_eq!(second.provenance.mesh_hash, second_mesh.blob);
}

#[test]
fn going_back_to_the_first_settings_runs_nothing() {
    let dir = scratch("back");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    passed(&save_document(&with_lid_speed(0.2), &path, true).report);
    passed(&solve_document(&path, &mut runner).report);
    assert_eq!(runner.calls, 2);

    passed(&save_document(CAVITY, &path, true).report);
    let done = solve_document(&path, &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    assert_eq!(done.line, Some(Line::NothingToRun));
    let found = data(&done.report);
    assert!(found.recorded.is_empty());
    assert_eq!(found.reused, ["mesh", "result"]);
    assert_eq!(open(&path).unwrap().document.artifacts.runs.len(), 2);
}

#[test]
fn the_provenance_names_everything_the_roadmap_lists() {
    let dir = scratch("provenance");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);

    let document = open(&path).unwrap().document;
    let run = &document.artifacts.runs[0];
    let provenance = &run.provenance;
    let geometry = provenance.geometry_hash.to_string();
    assert_eq!(geometry.len(), 64);
    assert!(geometry.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(provenance.mesh_hash, run.mesh.as_ref().unwrap().blob);
    assert_eq!(provenance.mesh_key, run.mesh.as_ref().unwrap().key);
    assert_eq!(provenance.settings, document.study);
    assert!(provenance.tool.image_digest.starts_with("sha256:"));
    assert_eq!(provenance.tool.dialect, "esi-v2512");
    assert_eq!(
        provenance.tool.writer_version,
        tool_identity(Dialect::EsiV2512).writer_version
    );
    assert_eq!(provenance.host.as_deref(), Some("fixture-host"));
    assert_eq!(
        provenance.result_key,
        Some(run.result.as_ref().unwrap().key)
    );
    assert_eq!(run.checks.len(), 1);
    assert!(run.checks[0].passed);
    assert!(!run.key_numbers.is_empty());
    assert!(run.comparison.is_none());
}

fn zip_of(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn an_older_file_is_saved_at_the_current_schema_and_the_report_says_so() {
    let dir = scratch("older");
    let path = dir.join("old.cfdcraft");
    let manifest = serde_json::json!({
        "format": "cfdcraft-study",
        "schema_version": 0,
        "writer": { "name": "other-tool", "version": "9" }
    })
    .to_string()
    .into_bytes();
    fs::write(
        &path,
        zip_of(&[
            ("manifest.json", manifest),
            ("study.json", CAVITY_V0.as_bytes().to_vec()),
        ]),
    )
    .unwrap();
    assert_eq!(open(&path).unwrap().schema_version, 0);
    let current = new_document(&dir, "current", CAVITY);

    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let done = solve_document(&path, &mut runner);
    passed(&done.report);

    let reopened = open(&path).unwrap();
    assert_eq!(
        reopened.schema_version,
        open(&current).unwrap().schema_version
    );
    assert!(reopened.notes.is_empty());
    assert_eq!(reopened.document.artifacts.runs.len(), 1);
    let note = done
        .report
        .migration
        .as_ref()
        .expect("a note about the older file");
    assert_eq!(note.from, 0);
    assert!(note.summary.contains("saved"), "{}", note.summary);
}

#[test]
fn a_replace_save_keeps_the_history() {
    let dir = scratch("replace");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    let before = open(&path).unwrap().document;

    passed(&save_document(&with_lid_speed(0.3), &path, true).report);
    let after = open(&path).unwrap().document;
    assert_ne!(after.study, before.study);
    assert_eq!(after.artifacts, before.artifacts);
    assert_eq!(after.thumbnail, before.thumbnail);
    assert_eq!(runner.calls, 1);
}
