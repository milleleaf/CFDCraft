//! Checking the points of a stored sweep before anything is solved: each point is built into a
//! case with the same gates a solve runs, a failing point is reported with its reasons, and the
//! check writes and starts nothing.

use std::collections::HashSet;
use std::path::PathBuf;

use cfdcraft_ops::scenario_ops::scenario_set;
use cfdcraft_ops::sweep_ops::{sweep_create, sweep_preflight};
use cfdcraft_ops::{DocOutcome, ExitClass, PreflightData, Repro};
use cfdcraft_scenarios::Override;
use cfdcraft_sweep::{Parameter, Sweep, Target, Values};
use cfdcraft_text::Line;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";

fn parameter(name: &str, target: Target, values: Values) -> Parameter {
    Parameter {
        name: name.to_owned(),
        target,
        values,
    }
}

fn list(items: &[f64]) -> Values {
    Values::List {
        items: items.to_vec(),
    }
}

fn number(name: &str, setting: &str, items: &[f64]) -> Parameter {
    let target = Target::Number {
        setting: setting.to_owned(),
    };
    parameter(name, target, list(items))
}

fn count(name: &str, setting: &str, items: &[f64]) -> Parameter {
    let target = Target::Count {
        setting: setting.to_owned(),
    };
    parameter(name, target, list(items))
}

fn sweep(id: &str, base: &str, parameters: Vec<Parameter>) -> Sweep {
    Sweep {
        id: id.to_owned(),
        base: base.to_owned(),
        parameters,
    }
}

/// Three lid speeds by two cell counts, the lid varying slowest.
fn grid() -> Sweep {
    sweep(
        "grid",
        "base",
        vec![
            number("lid_speed", LID, &[0.1, 0.2, 0.3]),
            count("cells_x", CELLS_X, &[20.0, 40.0]),
        ],
    )
}

/// A new document of the cavity that holds the sweep.
fn document_with(tag: &str, definition: &Sweep) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    passed(&sweep_create(&path, definition).report);
    (dir, path)
}

fn checked(out: &DocOutcome) -> &PreflightData {
    match &out.report.repro {
        Some(Repro::Preflight(data)) => data.as_ref(),
        other => panic!("expected the checks of a sweep, found {other:?}"),
    }
}

/// Every point failed with a reason that holds `text`.
fn every_reason_holds(data: &PreflightData, text: &str) {
    for point in &data.points {
        assert!(!point.ok, "{}", point.id);
        assert_eq!(point.mesh_key, None);
        assert_eq!(point.result_key, None);
        assert_eq!(point.reasons.len(), 1, "{}: {:?}", point.id, point.reasons);
        assert!(
            point.reasons[0].contains(text),
            "{}: {}",
            point.id,
            point.reasons[0]
        );
    }
}

#[test]
fn a_sweep_that_is_sound_passes_and_counts_its_meshes() {
    let (_dir, path) = document_with("pre-sound", &grid());

    let out = sweep_preflight(&path, "grid");
    passed(&out.report);
    assert_eq!(out.report.exit_class, 0);
    assert_eq!(
        out.line,
        Some(Line::PreflightPassed {
            sweep: "grid".to_owned(),
            points: "6".to_owned(),
            meshes: "2".to_owned()
        })
    );
    let data = checked(&out);
    assert_eq!(data.sweep, "grid");
    assert_eq!(data.points.len(), 6);
    assert_eq!(data.distinct_meshes, 2);
    assert_eq!(data.failed, 0);
    assert!(data.points.iter().all(|p| p.ok && p.reasons.is_empty()));
    assert!(
        data.points
            .iter()
            .all(|p| p.mesh_key.is_some() && p.result_key.is_some())
    );
    let shared: Vec<Option<&str>> = data
        .points
        .iter()
        .map(|p| p.mesh_shared_with.as_deref())
        .collect();
    assert_eq!(
        shared,
        [
            None,
            None,
            Some("grid-001"),
            Some("grid-002"),
            Some("grid-001"),
            Some("grid-002")
        ]
    );
    // Six different studies make six different results.
    let results: HashSet<_> = data.points.iter().map(|p| p.result_key.clone()).collect();
    assert_eq!(results.len(), 6);
    let labels: Vec<&str> = data.points.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(labels[0], "lid_speed 0.1, cells_x 20");
    assert_eq!(labels[5], "lid_speed 0.3, cells_x 40");
}

#[test]
fn a_failing_point_is_reported_with_its_reason_and_the_class_is_1() {
    let cells = sweep(
        "cells",
        "base",
        vec![count("cells_x", CELLS_X, &[20.0, 0.0, 40.0])],
    );
    let (_dir, path) = document_with("pre-failing", &cells);

    let out = sweep_preflight(&path, "cells");
    assert_eq!(out.report.class(), ExitClass::GateFailed);
    assert_eq!(out.report.exit_class, 1);
    assert_eq!(codes(&out.report), ["CFD-SWP-003"]);
    let finding = &out.report.diagnostics[0].message;
    assert!(finding.contains("cells-002"), "{finding}");
    assert!(
        finding.contains("The mesh needs at least one cell in x"),
        "{finding}"
    );
    assert_eq!(
        out.line,
        Some(Line::PreflightFailed {
            sweep: "cells".to_owned(),
            failed: "1".to_owned(),
            points: "3".to_owned()
        })
    );

    let data = checked(&out);
    assert_eq!(data.failed, 1);
    assert_eq!(data.distinct_meshes, 2);
    let ok: Vec<bool> = data.points.iter().map(|p| p.ok).collect();
    assert_eq!(ok, [true, false, true]);
    let bad = &data.points[1];
    assert_eq!(bad.id, "cells-002");
    assert!(bad.reasons[0].contains("at least one cell"), "{:?}", bad);
    assert_eq!(bad.mesh_key, None);
    assert_eq!(bad.result_key, None);
}

#[test]
fn nothing_is_solved_and_nothing_is_written() {
    let failing = sweep(
        "cells",
        "base",
        vec![count("cells_x", CELLS_X, &[20.0, 0.0, 40.0])],
    );
    for (tag, definition) in [("pre-quiet-good", grid()), ("pre-quiet-bad", failing)] {
        let (_dir, path) = document_with(tag, &definition);
        let before = snapshot(&path);
        let out = sweep_preflight(&path, &definition.id);
        assert!(!checked(&out).points.is_empty());
        assert_eq!(snapshot(&path), before, "{tag}");
        assert!(
            cfdcraft_doc::open(&path)
                .unwrap()
                .document
                .artifacts
                .runs
                .is_empty()
        );
    }
}

#[test]
fn the_case_files_are_built_and_not_only_the_study_checked() {
    let dir = scratch("pre-case");
    let path = new_document(&dir, "cavity", CAVITY);
    // A raw entry in a file the case does not have: the study's own checks accept it, the written
    // case cannot take it.
    let ghost = Override::Set {
        setting: "/raw_overrides/ghost".to_owned(),
        value: json!({
            "backend": "esi",
            "file": "system/fvOptions",
            "entry": "limits/x",
            "value": "1"
        }),
    };
    passed(&scenario_set(&path, "ghosted", None, &[ghost], None).report);
    let lids = sweep(
        "lids",
        "ghosted",
        vec![number("lid_speed", LID, &[0.1, 0.2])],
    );
    passed(&sweep_create(&path, &lids).report);

    let out = sweep_preflight(&path, "lids");
    assert_eq!(out.report.class(), ExitClass::GateFailed);
    assert_eq!(codes(&out.report), ["CFD-SWP-003", "CFD-SWP-003"]);
    let data = checked(&out);
    assert_eq!(data.failed, 2);
    every_reason_holds(data, "cannot be applied");
    every_reason_holds(data, "ghost");
    every_reason_holds(data, "system/fvOptions");
}

#[test]
fn a_direction_sweep_on_a_study_without_a_vector_setting_fails_every_point() {
    let wind = sweep(
        "wind",
        "base",
        vec![parameter(
            "wind_dir",
            Target::Direction {
                setting_x: "/boundary_conditions/lid/wind_x".to_owned(),
                setting_y: "/boundary_conditions/lid/wind_y".to_owned(),
                unit: "m/s".to_owned(),
                base_x: 10.0,
                base_y: 0.0,
            },
            Values::Range {
                start: 0.0,
                stop: 270.0,
                step: 90.0,
            },
        )],
    );
    let (_dir, path) = document_with("pre-direction", &wind);

    let out = sweep_preflight(&path, "wind");
    assert_eq!(out.report.class(), ExitClass::GateFailed);
    assert_eq!(codes(&out.report), ["CFD-SWP-003"; 4]);
    let data = checked(&out);
    assert_eq!(data.points.len(), 4);
    assert_eq!(data.failed, 4);
    assert_eq!(data.distinct_meshes, 0);
    every_reason_holds(data, "does not apply to the study");
    for point in &data.points {
        assert!(point.reasons[0].contains(&point.id), "{:?}", point.reasons);
    }
}

#[test]
fn an_unknown_sweep_is_cfd_swp_004() {
    let (_dir, path) = document_with("pre-unknown", &grid());
    let before = snapshot(&path);

    let out = sweep_preflight(&path, "nope");
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&out.report), ["CFD-SWP-004"]);
    assert_eq!(out.line, None);
    assert!(
        out.report.diagnostics[0].message.contains("nope"),
        "{}",
        out.report.render_text()
    );
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_point_whose_variant_is_missing_fails_with_that_reason() {
    let (_dir, path) = document_with("pre-missing", &grid());
    let mut document = cfdcraft_doc::open(&path).unwrap().document;
    document.artifacts.scenarios.retain(|s| s.id != "grid-003");
    cfdcraft_doc::save(&document, &path).unwrap();

    let out = sweep_preflight(&path, "grid");
    assert_eq!(out.report.class(), ExitClass::GateFailed);
    assert_eq!(codes(&out.report), ["CFD-SWP-003"]);
    let data = checked(&out);
    assert_eq!(data.failed, 1);
    let missing = &data.points[2];
    assert_eq!(missing.id, "grid-003");
    assert!(!missing.ok);
    assert!(
        missing.reasons[0].contains("the variant is missing"),
        "{:?}",
        missing.reasons
    );
    assert_eq!(data.points.iter().filter(|p| p.ok).count(), 5);
    // The point after the missing one still reports its shared mesh.
    assert_eq!(data.points[3].mesh_shared_with.as_deref(), Some("grid-002"));
}

#[test]
fn warnings_of_a_passing_point_are_kept() {
    let coarse = sweep(
        "coarse",
        "base",
        vec![
            count("cells_x", CELLS_X, &[4.0]),
            count("cells_y", "/mesh/cells_y", &[4.0]),
        ],
    );
    let (_dir, path) = document_with("pre-warning", &coarse);

    let out = sweep_preflight(&path, "coarse");
    passed(&out.report);
    assert_eq!(codes(&out.report), ["CFD-WARN-002", "CFD-WARN-002"]);
    let data = checked(&out);
    assert_eq!(data.failed, 0);
    assert!(data.points[0].ok);
    assert!(data.points[0].mesh_key.is_some());
}

#[test]
fn the_report_is_json_with_the_frozen_members() {
    let (dir, path) = document_with("pre-json", &grid());
    let out = sweep_preflight(&path, "grid");
    passed(&out.report);

    let value = serde_json::to_value(checked(&out)).unwrap();
    let keys = |value: &serde_json::Value| {
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys(&value),
        ["distinct_meshes", "failed", "points", "sweep"]
    );
    assert_eq!(
        keys(&value["points"][0]),
        [
            "id",
            "label",
            "mesh_key",
            "mesh_shared_with",
            "ok",
            "reasons",
            "result_key"
        ]
    );
    assert!(value["points"][0]["mesh_shared_with"].is_null());
    assert_eq!(value["points"][2]["mesh_shared_with"], "grid-001");

    // The whole report names neither the folder nor the file.
    let text = out.report.to_json();
    assert!(!text.contains(&dir.display().to_string()), "{text}");
    assert!(!text.contains("cavity.cfdcraft"), "{text}");
    assert!(text.contains("\"distinct_meshes\""), "{text}");
}
