//! The status of a document: which of the mesh and the result are up to date, which are not and
//! why, in the words a person reads. Reading never writes.

use std::fs;
use std::path::PathBuf;

use cfdcraft_doc::{Document, save};
use cfdcraft_model::{Diagnostic, Severity, Study};
use cfdcraft_ops::{
    ExitClass, Report, Repro, StatusData, document_status, save_document, solve_document,
};
use cfdcraft_runs::Change;
use serde_json::Value;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

/// A document made from the cavity and solved once.
fn solved(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    path
}

fn with_cells_x(cells: u64) -> String {
    edit(CAVITY, |study| {
        study["mesh"]["cells_x"] = Value::from(cells);
    })
}

fn status_of(report: &Report) -> &StatusData {
    match &report.repro {
        Some(Repro::Status(data)) => data,
        other => panic!("expected the status of a document, found {other:?}"),
    }
}

/// The stale and never-made warnings of a report, in order.
fn stale_warnings(report: &Report) -> Vec<&Diagnostic> {
    report
        .diagnostics
        .iter()
        .filter(|d| d.code == "CFD-REPRO-001")
        .collect()
}

#[test]
fn a_solved_document_is_up_to_date() {
    let path = solved("status-fresh");

    let done = document_status(&path);
    passed(&done.report);
    assert_eq!(done.report.operation, "status");
    assert!(
        done.report.diagnostics.is_empty(),
        "{}",
        done.report.render_text()
    );
    assert_eq!(
        done.line.as_ref().map(say).as_deref(),
        Some("Nothing is out of date.")
    );
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "fresh");
    assert_eq!(status.result.state, "fresh");
    assert_eq!(status.mesh.run.as_deref(), Some("run-1"));
    assert_eq!(status.result.run.as_deref(), Some("run-1"));
    assert_eq!(
        status.mesh.sentence,
        "The mesh is up to date (made by run-1)."
    );
    assert_eq!(
        status.result.sentence,
        "The result is up to date (made by run-1)."
    );
    assert!(status.mesh.reasons.is_empty() && status.mesh.changes.is_empty());
    assert_eq!(status.mesh.key.len(), 64);
    assert_ne!(status.mesh.key, status.result.key);
    assert!(done.report.study_hash.is_some());
}

#[test]
fn a_lid_speed_change_stales_the_result_and_not_the_mesh() {
    let path = solved("status-lid");
    passed(&save_document(&with_lid_speed(0.2), &path, true).report);

    let done = document_status(&path);
    passed(&done.report);
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "fresh");
    assert_eq!(status.result.state, "stale");
    assert_eq!(status.result.since.as_deref(), Some("run-1"));
    assert!(status.result.run.is_none());

    let stale = stale_warnings(&done.report);
    assert_eq!(stale.len(), 1, "{}", done.report.render_text());
    assert_eq!(stale[0].severity, Severity::Warning);
    assert_eq!(
        stale[0].message,
        "result stale because lid velocity 0.1 → 0.2 m/s"
    );
    assert_eq!(status.result.sentence, stale[0].message);
    assert_eq!(status.result.reasons, ["lid velocity 0.1 → 0.2 m/s"]);
    assert_eq!(
        status.result.changes,
        [Change::BoundaryVelocity {
            boundary: "lid".to_owned(),
            from: Some(0.1),
            to: Some(0.2),
        }]
    );
    assert!(done.line.is_none());
}

#[test]
fn a_cell_count_change_stales_both_with_their_reasons() {
    let path = solved("status-cells");
    passed(&save_document(&with_cells_x(256), &path, true).report);

    let done = document_status(&path);
    passed(&done.report);
    let stale = stale_warnings(&done.report);
    let messages: Vec<&str> = stale.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "mesh stale because cells in x 128 → 256",
            "result stale because its mesh changed"
        ]
    );
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "stale");
    assert_eq!(status.result.state, "stale");
    assert_eq!(status.mesh.since.as_deref(), Some("run-1"));
    assert_eq!(status.result.changes, [Change::MeshChanged]);
}

#[test]
fn a_document_with_no_runs_has_nothing_made() {
    let dir = scratch("status-empty");
    let path = new_document(&dir, "cavity", CAVITY);

    let done = document_status(&path);
    passed(&done.report);
    let stale = stale_warnings(&done.report);
    let messages: Vec<&str> = stale.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        ["No mesh has been made yet.", "No result has been made yet."]
    );
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "never_made");
    assert_eq!(status.result.state, "never_made");
    assert!(status.mesh.run.is_none() && status.mesh.since.is_none());
    assert!(done.line.is_none());
}

#[test]
fn a_stored_result_that_is_gone_is_reported_as_missing() {
    let path = solved("status-blob");
    fs::remove_dir_all(cfdcraft_doc::store_dir(&path)).unwrap();

    let done = document_status(&path);
    passed(&done.report);
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "blob_missing");
    assert_eq!(status.result.state, "blob_missing");
    assert_eq!(status.mesh.run.as_deref(), Some("run-1"));
    let codes: Vec<&str> = done
        .report
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .filter(|code| code.starts_with("CFD-REPRO"))
        .collect();
    assert_eq!(codes, ["CFD-REPRO-002", "CFD-REPRO-002"]);
    assert!(status.mesh.sentence.contains("is missing from the store"));
    assert!(done.line.is_none());
}

#[test]
fn status_never_writes() {
    let fresh = solved("never-writes-fresh");
    let lid = solved("never-writes-lid");
    passed(&save_document(&with_lid_speed(0.2), &lid, true).report);
    let cells = solved("never-writes-cells");
    passed(&save_document(&with_cells_x(256), &cells, true).report);
    let empty = new_document(&scratch("never-writes-empty"), "cavity", CAVITY);

    for path in [fresh, lid, cells, empty] {
        let before = snapshot(&path);
        passed(&document_status(&path).report);
        assert_eq!(snapshot(&path), before, "{}", path.display());
    }
}

#[test]
fn an_inconsistent_log_is_a_class_2_failure() {
    let path = solved("status-inconsistent");
    // The recorded mesh no longer matches the key it was recorded with.
    tamper_with_index(&path, "\"x\": 128,", "\"x\": 129,");
    let before = snapshot(&path);

    let done = document_status(&path);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    assert_eq!(done.report.operation, "status");
    assert_eq!(done.report.diagnostics.len(), 1);
    assert_eq!(done.report.diagnostics[0].code, "CFD-REPRO-003");
    assert!(done.report.diagnostics[0].message.contains("run-1"));
    assert!(done.report.repro.is_none());
    assert_eq!(snapshot(&path), before);
}

#[test]
fn an_invalid_study_is_a_class_2_failure_with_its_findings() {
    let dir = scratch("status-invalid");
    let path = dir.join("empty-mesh.cfdcraft");
    let mut study = Study::from_json(CAVITY).unwrap();
    study.mesh.cells_x = 0;
    save(&Document::new(study), &path).unwrap();
    let before = snapshot(&path);

    let done = document_status(&path);
    assert_eq!(
        done.report.class(),
        ExitClass::StudyInvalid,
        "{}",
        done.report.render_text()
    );
    assert_eq!(done.report.operation, "status");
    assert!(
        done.report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    );
    assert!(done.report.repro.is_none());
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_file_that_is_not_a_study_file_is_a_class_2_failure() {
    let dir = scratch("status-plain");
    let plain = dir.join("plain.cfdcraft");
    fs::write(&plain, CAVITY).unwrap();

    let done = document_status(&plain);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    assert_eq!(done.report.diagnostics[0].code, "CFD-DOC-001");
    assert_eq!(fs::read_to_string(&plain).unwrap(), CAVITY);
}
