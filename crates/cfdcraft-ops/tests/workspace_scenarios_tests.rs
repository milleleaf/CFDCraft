//! The variant operations of a workspace, addressed by document name: every one judges the name
//! first, shows no path of the server, and changes only the document it names. These tests use no
//! fake host, so they run on every platform.

use std::path::PathBuf;

use cfdcraft_ops::scenario_ops::{LabelUpdate, scenario_set};
use cfdcraft_ops::{ExitClass, Report, Repro, Workspace};
use cfdcraft_scenarios::Override;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";

fn lid(value: f64) -> Override {
    Override::Set {
        setting: LID.to_owned(),
        value: json!(value),
    }
}

/// A workspace holding the document `cavity` with the variant `fast-lid`, and the document's file.
fn workspace_with_variant(tag: &str) -> (Workspace, PathBuf, PathBuf) {
    let dir = scratch(tag);
    let ws = Workspace::open(dir.join("ws")).unwrap();
    let root = dir.join("ws").canonicalize().unwrap();
    let path = new_document(&root.join("documents"), "cavity", CAVITY);
    passed(&scenario_set(&path, "fast-lid", None, &[lid(0.2)], None).report);
    (ws, root, path)
}

/// A workspace holding the document `cavity` and no variant.
fn plain_workspace(tag: &str) -> (Workspace, PathBuf, PathBuf) {
    let dir = scratch(tag);
    let ws = Workspace::open(dir.join("ws")).unwrap();
    let root = dir.join("ws").canonicalize().unwrap();
    let path = new_document(&root.join("documents"), "cavity", CAVITY);
    (ws, root, path)
}

/// The answer of every variant method for the document `name`.
fn every_answer(ws: &Workspace, name: &str) -> Vec<Report> {
    vec![
        ws.scenario_list(name),
        ws.scenario_show(name, "fast-lid"),
        ws.scenario_diff(name, "base", "fast-lid"),
        ws.scenario_status(name, Some("fast-lid")),
        ws.scenario_set(name, "other", None, &[lid(0.3)], None),
        ws.scenario_label(name, "fast-lid", &LabelUpdate::default()),
    ]
}

#[test]
fn the_workspace_lists_shows_and_diffs_variants_by_document_name() {
    let (ws, root, _) = workspace_with_variant("wss-read");

    let list = ws.scenario_list("cavity");
    passed(&list);
    assert_eq!(list.operation, "scenario_list");
    let Some(Repro::Scenarios(data)) = &list.repro else {
        panic!("expected the list of variants: {list:?}");
    };
    let ids: Vec<&str> = data.scenarios.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, ["base", "fast-lid"]);

    let shown = ws.scenario_show("cavity", "fast-lid");
    passed(&shown);
    assert_eq!(shown.operation, "scenario_show");
    let Some(Repro::Scenario(one)) = &shown.repro else {
        panic!("expected one variant: {shown:?}");
    };
    assert_eq!(one.overrides, vec![lid(0.2)]);

    let diff = ws.scenario_diff("cavity", "base", "fast-lid");
    passed(&diff);
    assert_eq!(diff.operation, "scenario_diff");
    let Some(Repro::ScenarioDiff(changes)) = &diff.repro else {
        panic!("expected a difference: {diff:?}");
    };
    assert!(!changes.same);
    assert_eq!(
        changes.sentence,
        "fast-lid differs from base: lid velocity 0.1 → 0.2 m/s."
    );

    let shown_root = root.display().to_string();
    for report in [&list, &shown, &diff] {
        let json = serde_json::to_string(report).unwrap();
        assert!(!json.contains(&shown_root), "{json}");
    }
}

#[test]
fn a_bad_document_name_is_cfd_ws_001() {
    let (ws, root, _) = workspace_with_variant("wss-names");
    let before = tree(&root);
    for bad in ["../x", "UPPER", "a/b", ""] {
        for report in every_answer(&ws, bad) {
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
    assert_eq!(tree(&root), before, "nothing was touched");
}

#[test]
fn a_missing_document_is_a_report_not_a_panic() {
    let (ws, root, _) = plain_workspace("wss-missing");
    let before = tree(&root);
    for report in every_answer(&ws, "absent") {
        assert_eq!(
            report.class(),
            ExitClass::StudyInvalid,
            "{}",
            report.render_text()
        );
        assert!(
            codes(&report).contains(&"CFD-DOC-001"),
            "{}",
            report.render_text()
        );
    }
    assert_eq!(tree(&root), before, "no document was made");
}

#[test]
fn the_workspace_creates_and_changes_a_variant_by_name() {
    let (ws, _root, path) = plain_workspace("wss-set");

    let made = ws.scenario_set("cavity", "fast-lid", None, &[lid(0.2)], Some("Fast lid"));
    passed(&made);
    assert_eq!(made.operation, "scenario_set");
    assert!(
        made.artifacts
            .iter()
            .any(|a| a.kind == "scenario" && a.name == "fast-lid"),
        "{made:?}"
    );

    let again = ws.scenario_set("cavity", "fast-lid", None, &[lid(0.25)], None);
    passed(&again);
    let stored = cfdcraft_doc::open(&path)
        .unwrap()
        .document
        .artifacts
        .scenarios;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].overrides, vec![lid(0.25)]);
    assert_eq!(stored[0].name.as_deref(), Some("Fast lid"));
}

#[test]
fn the_workspace_labels_a_variant() {
    let (ws, _root, path) = workspace_with_variant("wss-label");
    let stored = |path: &std::path::Path| {
        cfdcraft_doc::open(path)
            .unwrap()
            .document
            .artifacts
            .scenarios[0]
            .clone()
    };

    let set = LabelUpdate {
        name: Some("Fast lid".to_owned()),
        pinned: Some(true),
        notes: Some("the first guess".to_owned()),
    };
    let done = ws.scenario_label("cavity", "fast-lid", &set);
    passed(&done);
    assert_eq!(done.operation, "scenario_label");
    let held = stored(&path);
    assert_eq!(held.name.as_deref(), Some("Fast lid"));
    assert!(held.pinned);
    assert_eq!(held.notes.as_deref(), Some("the first guess"));

    let clear = LabelUpdate {
        name: Some(String::new()),
        pinned: Some(false),
        notes: Some(String::new()),
    };
    passed(&ws.scenario_label("cavity", "fast-lid", &clear));
    let held = stored(&path);
    assert_eq!(held.name, None);
    assert!(!held.pinned);
    assert_eq!(held.notes, None);
}

#[test]
fn document_status_takes_a_scenario() {
    let (ws, root, _) = workspace_with_variant("wss-status");
    let before = tree(&root);

    let variant = ws.scenario_status("cavity", Some("fast-lid"));
    passed(&variant);
    assert_eq!(variant.operation, "document_status");
    assert!(matches!(variant.repro, Some(Repro::Status(_))));
    assert!(
        variant
            .artifacts
            .iter()
            .any(|a| a.kind == "scenario" && a.name == "fast-lid"),
        "{variant:?}"
    );

    let study = ws.scenario_status("cavity", None);
    assert_eq!(
        serde_json::to_value(&study).unwrap(),
        serde_json::to_value(ws.document_status("cavity")).unwrap()
    );

    let unknown = ws.scenario_status("cavity", Some("nope"));
    assert_eq!(unknown.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&unknown), ["CFD-SCN-001"]);
    assert_eq!(tree(&root), before, "reading wrote nothing");
}
