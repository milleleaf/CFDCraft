//! Creating, changing, listing and labelling variants of a document: what is stored, what is
//! refused, and that a refusal leaves the file and its store as they were.

use std::path::PathBuf;

use cfdcraft_ops::scenario_ops::{LabelUpdate, scenario_label, scenario_list, scenario_set};
use cfdcraft_ops::{DocOutcome, ExitClass, Repro, ScenarioSummary};
use cfdcraft_scenarios::{Override, Scenario};
use cfdcraft_text::Line;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

fn lid(value: f64) -> Override {
    Override::Set {
        setting: "/boundary_conditions/lid/velocity/value".to_owned(),
        value: json!(value),
    }
}

fn cells_x(count: u32) -> Override {
    Override::Set {
        setting: "/mesh/cells_x".to_owned(),
        value: json!(count),
    }
}

fn viscosity(value: f64) -> Override {
    Override::Set {
        setting: "/fluid/kinematic_viscosity/value".to_owned(),
        value: json!(value),
    }
}

/// A scratch folder and a new document of the cavity in it.
fn cavity_document(tag: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    (dir, path)
}

/// Create a variant and check that it was created.
fn create(path: &std::path::Path, id: &str, parent: Option<&str>, sets: &[Override]) -> DocOutcome {
    let out = scenario_set(path, id, parent, sets, None);
    passed(&out.report);
    out
}

fn listed(path: &std::path::Path) -> Vec<ScenarioSummary> {
    let out = scenario_list(path);
    passed(&out.report);
    match out.report.repro {
        Some(Repro::Scenarios(data)) => data.scenarios,
        other => panic!("expected the list of variants, found {other:?}"),
    }
}

fn stored(path: &std::path::Path) -> Vec<Scenario> {
    cfdcraft_doc::open(path)
        .unwrap()
        .document
        .artifacts
        .scenarios
}

fn find<'a>(rows: &'a [ScenarioSummary], id: &str) -> &'a ScenarioSummary {
    rows.iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("no row for {id}"))
}

/// The refusal an operation answered with: it has this code and class 2, and no line.
fn refused_with(out: &DocOutcome, code: &str) {
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&out.report), [code], "{}", out.report.render_text());
    assert_eq!(out.line, None);
}

#[test]
fn a_variant_is_created_with_only_its_overrides() {
    let (_dir, path) = cavity_document("scn-create");
    let before = cfdcraft_doc::open(&path).unwrap().document.study;

    let out = scenario_set(&path, "fast-lid", None, &[lid(0.2)], None);
    passed(&out.report);
    assert_eq!(
        out.line,
        Some(Line::ScenarioCreated {
            scenario: "fast-lid".to_owned(),
            parent: "base".to_owned()
        })
    );

    let document = cfdcraft_doc::open(&path).unwrap().document;
    assert_eq!(document.study, before);
    assert_eq!(document.artifacts.scenarios.len(), 1);
    let variant = &document.artifacts.scenarios[0];
    assert_eq!(variant.id, "fast-lid");
    assert_eq!(variant.parent.as_deref(), Some("base"));
    assert_eq!(variant.overrides, vec![lid(0.2)]);
}

#[test]
fn a_variant_of_a_variant_is_created_under_its_parent() {
    let (_dir, path) = cavity_document("scn-nested");
    create(&path, "fine", None, &[cells_x(256)]);
    let out = create(&path, "fine-fast", Some("fine"), &[lid(0.3)]);
    assert_eq!(
        out.line,
        Some(Line::ScenarioCreated {
            scenario: "fine-fast".to_owned(),
            parent: "fine".to_owned()
        })
    );

    let rows = listed(&path);
    let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, ["base", "fine", "fine-fast"]);
    let variants = stored(&path);
    assert_eq!(variants[1].parent.as_deref(), Some("fine"));
    assert_eq!(variants[1].overrides, vec![lid(0.3)]);
}

#[test]
fn setting_again_replaces_the_override_of_the_same_setting_and_keeps_the_others() {
    let (_dir, path) = cavity_document("scn-again");
    create(&path, "fast-lid", None, &[lid(0.2), viscosity(0.0002)]);

    let out = scenario_set(&path, "fast-lid", None, &[lid(0.25), cells_x(256)], None);
    passed(&out.report);
    assert_eq!(
        out.line,
        Some(Line::ScenarioUpdated {
            scenario: "fast-lid".to_owned()
        })
    );

    let variants = stored(&path);
    assert_eq!(variants.len(), 1);
    assert_eq!(
        variants[0].overrides,
        vec![lid(0.25), viscosity(0.0002), cells_x(256)]
    );
}

#[test]
fn a_variant_keeps_its_parent() {
    let (_dir, path) = cavity_document("scn-parent");
    create(&path, "fast-lid", None, &[lid(0.2)]);
    create(&path, "other", None, &[cells_x(256)]);
    let before = snapshot(&path);

    let out = scenario_set(&path, "fast-lid", Some("other"), &[lid(0.3)], None);
    refused_with(&out, "CFD-SCN-002");
    assert_eq!(snapshot(&path), before);

    let same = scenario_set(&path, "fast-lid", Some("base"), &[lid(0.3)], None);
    passed(&same.report);
    assert_eq!(stored(&path)[0].overrides, vec![lid(0.3)]);
}

#[test]
fn the_list_shows_base_first_with_the_kind_and_the_keys() {
    let (_dir, path) = cavity_document("scn-list");
    create(&path, "fast-lid", None, &[lid(0.2)]);
    create(&path, "fine", None, &[cells_x(256)]);
    create(&path, "fine-fast", Some("fine"), &[lid(0.3)]);

    let rows = listed(&path);
    let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, ["base", "fast-lid", "fine", "fine-fast"]);

    let base = find(&rows, "base");
    assert_eq!(base.kind, "base");
    assert_eq!(base.parent, None);
    assert_eq!(base.overrides, 0);
    assert!(base.mesh_key.is_some() && base.result_key.is_some());
    assert_eq!(base.mesh_shared_with, None);

    let fast = find(&rows, "fast-lid");
    assert_eq!(fast.kind, "setting");
    assert_eq!(fast.parent.as_deref(), Some("base"));
    assert_eq!(fast.overrides, 1);
    assert_eq!(fast.mesh_key, base.mesh_key);
    assert!(fast.result_key.is_some());
    assert_ne!(fast.result_key, base.result_key);
    assert_eq!(fast.mesh_shared_with.as_deref(), Some("base"));

    let fine = find(&rows, "fine");
    assert_eq!(fine.kind, "geometry");
    assert!(fine.mesh_key.is_some());
    assert_ne!(fine.mesh_key, base.mesh_key);
    assert_eq!(fine.mesh_shared_with, None);

    let both = find(&rows, "fine-fast");
    assert_eq!(both.kind, "setting");
    assert_eq!(both.mesh_key, fine.mesh_key);
    assert_ne!(both.result_key, fine.result_key);
    assert_eq!(both.mesh_shared_with.as_deref(), Some("fine"));
}

#[test]
fn a_variant_whose_study_fails_the_checks_is_listed_without_keys() {
    let (_dir, path) = cavity_document("scn-nokeys");
    create(&path, "stopped-lid", None, &[lid(0.0)]);

    let rows = listed(&path);
    let row = find(&rows, "stopped-lid");
    assert_eq!(row.mesh_key, None);
    assert_eq!(row.result_key, None);
    assert_eq!(row.kind, "setting");
}

#[test]
fn the_report_names_no_path() {
    let (dir, path) = cavity_document("scn-nopath");
    let created = scenario_set(&path, "fast-lid", None, &[lid(0.2)], Some("Fast lid"));
    let updated = scenario_set(&path, "fast-lid", None, &[lid(0.25)], None);
    let labelled = scenario_label(
        &path,
        "fast-lid",
        &LabelUpdate {
            notes: Some("check the corner".to_owned()),
            ..LabelUpdate::default()
        },
    );
    let list = scenario_list(&path);

    let folder = dir.to_string_lossy().into_owned();
    let folder_name = dir.file_name().unwrap().to_string_lossy().into_owned();
    for out in [created, updated, labelled, list] {
        passed(&out.report);
        let json = out.report.to_json();
        for text in [folder.as_str(), folder_name.as_str(), "cavity.cfdcraft"] {
            assert!(!json.contains(text), "{text} is in {json}");
        }
    }
}

#[test]
fn labelling_sets_name_pin_and_notes_and_clears_them() {
    let (_dir, path) = cavity_document("scn-label");
    create(&path, "fast-lid", None, &[lid(0.2)]);

    let out = scenario_label(
        &path,
        "fast-lid",
        &LabelUpdate {
            name: Some("Fast lid".to_owned()),
            pinned: Some(true),
            notes: Some("check the corner".to_owned()),
        },
    );
    passed(&out.report);
    assert_eq!(
        out.line,
        Some(Line::ScenarioLabelled {
            scenario: "fast-lid".to_owned()
        })
    );
    let rows = listed(&path);
    let row = find(&rows, "fast-lid");
    assert_eq!(row.name.as_deref(), Some("Fast lid"));
    assert!(row.pinned);
    assert_eq!(row.notes.as_deref(), Some("check the corner"));

    // A field left out is kept.
    let unpinned = LabelUpdate {
        pinned: Some(false),
        ..LabelUpdate::default()
    };
    passed(&scenario_label(&path, "fast-lid", &unpinned).report);
    let rows = listed(&path);
    let row = find(&rows, "fast-lid");
    assert!(!row.pinned);
    assert_eq!(row.name.as_deref(), Some("Fast lid"));
    assert_eq!(row.notes.as_deref(), Some("check the corner"));

    // An empty text clears.
    let cleared = LabelUpdate {
        name: Some(String::new()),
        pinned: Some(false),
        notes: Some(String::new()),
    };
    passed(&scenario_label(&path, "fast-lid", &cleared).report);
    let rows = listed(&path);
    let row = find(&rows, "fast-lid");
    assert_eq!(row.name, None);
    assert!(!row.pinned);
    assert_eq!(row.notes, None);
    assert_eq!(stored(&path)[0].overrides, vec![lid(0.2)]);
}

#[test]
fn the_base_can_be_labelled_and_is_listed_first() {
    let (_dir, path) = cavity_document("scn-label-base");
    create(&path, "fast-lid", None, &[lid(0.2)]);

    let first = LabelUpdate {
        name: Some("Reference".to_owned()),
        pinned: Some(true),
        notes: Some("start here".to_owned()),
    };
    passed(&scenario_label(&path, "base", &first).report);

    let records = stored(&path);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id, "base");
    assert_eq!(records[0].parent, None);
    assert!(records[0].overrides.is_empty());
    assert_eq!(records[1].id, "fast-lid");

    let rows = listed(&path);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "base");
    assert_eq!(rows[0].name.as_deref(), Some("Reference"));
    assert!(rows[0].pinned);
    assert_eq!(rows[0].notes.as_deref(), Some("start here"));
    assert_eq!(rows[0].kind, "base");

    let second = LabelUpdate {
        name: Some("Start".to_owned()),
        ..LabelUpdate::default()
    };
    passed(&scenario_label(&path, "base", &second).report);
    assert_eq!(stored(&path).len(), 2);
    assert_eq!(listed(&path)[0].name.as_deref(), Some("Start"));
}

#[test]
fn a_name_or_note_with_a_control_character_is_refused() {
    let (_dir, path) = cavity_document("scn-control");
    create(&path, "fast-lid", None, &[lid(0.2)]);
    let before = snapshot(&path);

    let refusals = [
        LabelUpdate {
            name: Some("two\nlines".to_owned()),
            ..LabelUpdate::default()
        },
        LabelUpdate {
            name: Some("a\ttab".to_owned()),
            ..LabelUpdate::default()
        },
        LabelUpdate {
            notes: Some("a\ttab".to_owned()),
            ..LabelUpdate::default()
        },
        LabelUpdate {
            name: Some("n".repeat(81)),
            ..LabelUpdate::default()
        },
        LabelUpdate {
            notes: Some("n".repeat(2001)),
            ..LabelUpdate::default()
        },
    ];
    for update in &refusals {
        refused_with(&scenario_label(&path, "fast-lid", update), "CFD-SCN-002");
        assert_eq!(snapshot(&path), before);
    }

    let long = LabelUpdate {
        name: Some("n".repeat(80)),
        notes: Some("first line\nsecond line".to_owned()),
        ..LabelUpdate::default()
    };
    passed(&scenario_label(&path, "fast-lid", &long).report);
    let rows = listed(&path);
    assert_eq!(
        find(&rows, "fast-lid").notes.as_deref(),
        Some("first line\nsecond line")
    );
}

#[test]
fn an_unknown_variant_cannot_be_labelled() {
    let (_dir, path) = cavity_document("scn-label-unknown");
    let before = snapshot(&path);
    let update = LabelUpdate {
        pinned: Some(true),
        ..LabelUpdate::default()
    };
    let out = scenario_label(&path, "ghost", &update);
    refused_with(&out, "CFD-SCN-001");
    assert!(out.report.diagnostics[0].message.contains("ghost"));
    assert_eq!(snapshot(&path), before);
}

#[test]
fn an_id_must_be_a_plain_name_and_not_base() {
    let (_dir, path) = cavity_document("scn-ids");
    let before = snapshot(&path);
    for id in ["Fast Lid", "base"] {
        refused_with(
            &scenario_set(&path, id, None, &[lid(0.2)], None),
            "CFD-SCN-002",
        );
        assert_eq!(snapshot(&path), before);
    }
}

#[test]
fn a_parent_that_does_not_exist_is_cfd_scn_001() {
    let (_dir, path) = cavity_document("scn-no-parent");
    let before = snapshot(&path);
    let out = scenario_set(&path, "kid", Some("ghost"), &[lid(0.2)], None);
    refused_with(&out, "CFD-SCN-001");
    assert!(out.report.diagnostics[0].message.contains("ghost"));
    assert_eq!(snapshot(&path), before);
}

#[test]
fn an_override_that_does_not_apply_saves_nothing() {
    let (_dir, path) = cavity_document("scn-no-apply");
    create(&path, "fast-lid", None, &[lid(0.2)]);
    let before = snapshot(&path);

    let density = Override::Set {
        setting: "/fluid/density".to_owned(),
        value: json!(1.2),
    };
    let out = scenario_set(&path, "dense", None, &[density], None);
    refused_with(&out, "CFD-SCN-003");
    assert!(out.report.diagnostics[0].message.contains("dense"));
    assert_eq!(snapshot(&path), before);

    let nowhere = Override::Set {
        setting: "/boundary_conditions/nope/kind".to_owned(),
        value: json!("NoSlipWall"),
    };
    let out = scenario_set(&path, "fast-lid", None, &[nowhere], None);
    refused_with(&out, "CFD-SCN-003");
    assert_eq!(snapshot(&path), before);
}

#[test]
fn an_edit_that_would_break_a_child_is_refused() {
    let (_dir, path) = cavity_document("scn-break-child");
    create(&path, "parent-v", None, &[lid(0.2)]);
    create(&path, "child-v", Some("parent-v"), &[lid(0.3)]);
    let before = snapshot(&path);

    let drop_velocity = Override::Remove {
        setting: "/boundary_conditions/lid/velocity".to_owned(),
    };
    let out = scenario_set(&path, "parent-v", None, &[drop_velocity], None);
    refused_with(&out, "CFD-SCN-003");
    assert!(
        out.report.diagnostics[0].message.contains("child-v"),
        "{}",
        out.report.diagnostics[0].message
    );
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_pack_file_is_never_changed() {
    let (dir, path) = cavity_document("scn-pack");
    let packed = dir.join("cavity.cfdpack");
    std::fs::copy(&path, &packed).unwrap();
    let before = std::fs::read(&packed).unwrap();

    let out = scenario_set(&packed, "fast-lid", None, &[lid(0.2)], None);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&out.report), ["CFD-DOC-009"]);
    let update = LabelUpdate {
        pinned: Some(true),
        ..LabelUpdate::default()
    };
    let out = scenario_label(&packed, "base", &update);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&out.report), ["CFD-DOC-009"]);
    assert_eq!(std::fs::read(&packed).unwrap(), before);
}

#[test]
fn the_variant_limits_hold() {
    let (_dir, path) = cavity_document("scn-limit");
    let mut document = cfdcraft_doc::open(&path).unwrap().document;
    document.artifacts.scenarios = (0..1000)
        .map(|number| Scenario {
            id: format!("v-{number}"),
            parent: Some("base".to_owned()),
            name: None,
            pinned: false,
            notes: None,
            overrides: Vec::new(),
            origin: None,
        })
        .collect();
    cfdcraft_doc::save(&document, &path).unwrap();
    let before = snapshot(&path);

    let out = scenario_set(&path, "one-too-many", None, &[lid(0.2)], None);
    refused_with(&out, "CFD-SCN-002");
    assert_eq!(snapshot(&path), before);
}
