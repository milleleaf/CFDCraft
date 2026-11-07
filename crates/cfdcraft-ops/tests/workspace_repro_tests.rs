//! The read operations on documents, addressed by name inside the workspace: every one judges the
//! name first, shows no path of the server, follows no link out of the workspace and writes
//! nothing. These tests use no fake host, so they run on every platform.

use std::fs;
use std::path::PathBuf;

use cfdcraft_doc::{Document, Link, save};
use cfdcraft_model::Study;
use cfdcraft_ops::{ExitClass, Report, Workspace, save_document, solve_document};
use cfdcraft_store::ContentHash;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

/// A workspace holding the document `cavity`: two runs, then a third lid speed saved after them,
/// so the mesh is up to date and the result is not.
fn solved_workspace(tag: &str) -> (Workspace, PathBuf, PathBuf) {
    let dir = scratch(tag);
    let ws = Workspace::open(dir.join("ws")).unwrap();
    let root = dir.join("ws").canonicalize().unwrap();
    let path = new_document(&root.join("documents"), "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    passed(&save_document(&with_lid_speed(0.2), &path, true).report);
    passed(&solve_document(&path, &mut runner).report);
    passed(&save_document(&with_lid_speed(0.3), &path, true).report);
    (ws, root, path)
}

/// The answer of every read method for the document `name`.
fn every_answer(ws: &Workspace, name: &str) -> Vec<Report> {
    vec![
        ws.document_status(name),
        ws.list_runs(name),
        ws.document_result(name, None),
        ws.document_result(name, Some("run-1")),
        ws.document_provenance(name, None),
        ws.document_provenance(name, Some("run-2")),
        ws.compare_runs(name, "run-1", "run-2", None, None),
        ws.compare_runs(name, "run-1", "run-2", Some(1.0), Some(1.0)),
        ws.plan_update(&[name.to_owned()], None, None),
    ]
}

/// Every piece of text in a report that could carry a path.
fn texts(report: &Report) -> Vec<String> {
    let mut out = Vec::new();
    for found in &report.diagnostics {
        out.push(found.message.clone());
        out.push(found.setting.clone());
        out.extend(found.hint.clone());
        out.extend(found.location.as_ref().map(|at| at.file.clone()));
    }
    out.extend(report.artifacts.iter().map(|a| a.name.clone()));
    out
}

#[test]
fn every_method_refuses_a_bad_name_first() {
    let (ws, root, _) = solved_workspace("wr-names");
    let before = tree(&root);
    let long = "a".repeat(41);
    for bad in ["../x", "UPPER", "", long.as_str()] {
        let mut answers = every_answer(&ws, bad);
        answers.push(ws.compare_runs(bad, "run-1", "run-2", Some(-1.0), None));
        for report in answers {
            assert_eq!(
                report.class(),
                ExitClass::StudyInvalid,
                "{bad:?}: {}",
                report.render_text()
            );
            assert_eq!(codes(&report), ["CFD-WS-001"], "{bad:?}");
            assert!(report.repro.is_none());
        }
    }
    // A bad name among good ones refuses the whole plan, and so do too many names.
    let mixed = ["cavity".to_owned(), "../x".to_owned()];
    assert_eq!(codes(&ws.plan_update(&mixed, None, None)), ["CFD-WS-001"]);
    let many: Vec<String> = (0..17).map(|n| format!("d{n}")).collect();
    assert_eq!(codes(&ws.plan_update(&many, None, None)), ["CFD-WS-001"]);
    assert_eq!(tree(&root), before, "nothing was touched");
}

#[test]
fn reports_name_no_workspace_path() {
    let (ws, root, _) = solved_workspace("wr-paths");
    let shown = root.display().to_string();
    let doubled = shown.replace('\\', "\\\\");
    let mut answers = every_answer(&ws, "cavity");
    // Answers that are refusals still show no path.
    answers.push(ws.document_status("missing"));
    answers.push(ws.document_result("cavity", Some("run-9")));
    answers.push(ws.compare_runs("cavity", "run-1", "run-2", Some(-1.0), None));
    answers.push(ws.plan_update(
        &["cavity".to_owned()],
        Some(&["cavity:mesh".to_owned()]),
        None,
    ));
    for report in &answers {
        let json = serde_json::to_string(report).unwrap();
        assert!(!json.contains(&shown) && !json.contains(&doubled), "{json}");
        for text in texts(report) {
            assert!(!text.contains(&shown), "{text}");
            assert!(
                !text.split_whitespace().any(|word| word.starts_with('/')),
                "{text}"
            );
        }
        for artifact in &report.artifacts {
            assert_eq!(
                artifact.name.replace('\\', "/"),
                "documents/cavity.cfdcraft"
            );
        }
    }
}

#[test]
fn a_document_in_the_workspace_that_links_outside_is_not_followed() {
    let dir = scratch("wr-link");
    let ws = Workspace::open(dir.join("ws")).unwrap();
    let root = dir.join("ws").canonicalize().unwrap();
    fs::write(dir.join("outside.txt"), b"outside").unwrap();
    let mut document = Document::new(Study::from_json(CAVITY).unwrap());
    for (path, name) in [
        ("../../outside.txt", "outside.txt"),
        ("../studies/none.json", "none.json"),
    ] {
        document.artifacts.links.push(Link {
            id: name.replace('.', "-"),
            path: path.to_owned(),
            name: name.to_owned(),
            hash: ContentHash::of(b"x"),
        });
    }
    save(&document, &root.join("documents").join("a.cfdcraft")).unwrap();

    let outside = dir.display().to_string();
    for report in every_answer(&ws, "a") {
        // Whatever the answer is, it is about the runs and never about the links: opening the
        // document for its status reads no file the document points at.
        assert!(
            codes(&report)
                .iter()
                .all(|code| !code.starts_with("CFD-DOC")),
            "{}",
            report.render_text()
        );
        for text in texts(&report) {
            assert!(!text.contains(&outside), "{text}");
        }
    }
    let status = ws.document_status("a");
    passed(&status);
    let never_made = codes(&status)
        .into_iter()
        .filter(|code| *code == "CFD-REPRO-001")
        .count();
    assert_eq!(never_made, 2, "{}", status.render_text());
}

#[test]
fn the_methods_do_not_write() {
    let (ws, root, path) = solved_workspace("wr-write");
    let tree_before = tree(&root);
    let document_before = snapshot(&path);
    for name in ["cavity", "missing"] {
        let _ = every_answer(&ws, name);
    }
    assert_eq!(tree(&root), tree_before);
    assert_eq!(snapshot(&path), document_before);
}
