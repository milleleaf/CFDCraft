//! The sweep criterion end to end, through the operations: a sweep over one or several
//! parameters (a list, a range, steps, a full factorial, a direction) is planned, stored and
//! checked point by point before any solve, and the points that fail are reported with their
//! reasons (`sc3_`). No test needs a compute host or a container engine.

use std::path::{Path, PathBuf};

use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::scenario_ops::scenario_list;
use cfdcraft_ops::sweep_ops::{sweep_create, sweep_plan, sweep_preflight};
use cfdcraft_ops::{
    DocOutcome, ExitClass, PreflightData, Repro, StatusData, SweepData, pack_document,
    scenario_status, solve_scenario, unpack_document,
};
use cfdcraft_sweep::{Assignment, Parameter, Sweep, Target, Values};
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";
const WIND_X: &str = "/boundary_conditions/lid/wind_x";
const WIND_Y: &str = "/boundary_conditions/lid/wind_y";

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

fn lid_speed(values: Values) -> Parameter {
    let target = Target::Number {
        setting: LID.to_owned(),
    };
    parameter("lid_speed", target, values)
}

fn cells_x(values: Values) -> Parameter {
    let target = Target::Count {
        setting: CELLS_X.to_owned(),
    };
    parameter("cells_x", target, values)
}

fn sweep(id: &str, parameters: Vec<Parameter>) -> Sweep {
    Sweep {
        id: id.to_owned(),
        base: "base".to_owned(),
        parameters,
    }
}

/// A sweep over `count` values of the cells in x, whole numbers from 20 up.
fn many_cells(id: &str, count: u32) -> Sweep {
    let values = Values::Steps {
        start: 20.0,
        stop: f64::from(19 + count),
        count,
    };
    sweep(id, vec![cells_x(values)])
}

/// Three lid speeds by three cell counts, the lid varying slowest. `cells` are the three counts.
fn factorial(id: &str, cells: [f64; 3]) -> Sweep {
    sweep(
        id,
        vec![lid_speed(list(&[0.1, 0.2, 0.3])), cells_x(list(&cells))],
    )
}

/// Three lid speeds by two cell counts, six points.
fn grid() -> Sweep {
    sweep(
        "grid",
        vec![
            lid_speed(list(&[0.1, 0.2, 0.3])),
            cells_x(list(&[20.0, 40.0])),
        ],
    )
}

/// A sweep of a vector setting turned through the four right angles.
fn wind() -> Sweep {
    let target = Target::Direction {
        setting_x: WIND_X.to_owned(),
        setting_y: WIND_Y.to_owned(),
        unit: "m/s".to_owned(),
        base_x: 10.0,
        base_y: 0.0,
    };
    let angles = Values::Range {
        start: 0.0,
        stop: 270.0,
        step: 90.0,
    };
    sweep("wind", vec![parameter("wind_dir", target, angles)])
}

/// A new document of the cavity, and the folder it is in.
fn cavity_document(tag: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    (dir, path)
}

/// A new document of the cavity that holds the sweep.
fn document_with(tag: &str, definition: &Sweep) -> (PathBuf, PathBuf) {
    let (dir, path) = cavity_document(tag);
    passed(&sweep_create(&path, definition).report);
    (dir, path)
}

fn planned(out: &DocOutcome) -> &SweepData {
    match &out.report.repro {
        Some(Repro::Sweep(data)) => data.as_ref(),
        other => panic!("expected the points of a sweep, found {other:?}"),
    }
}

fn checked(out: &DocOutcome) -> &PreflightData {
    match &out.report.repro {
        Some(Repro::Preflight(data)) => data.as_ref(),
        other => panic!("expected the checks of a sweep, found {other:?}"),
    }
}

fn stages(out: &DocOutcome) -> &StatusData {
    match &out.report.repro {
        Some(Repro::Status(data)) => data,
        other => panic!("expected the status, found {other:?}"),
    }
}

fn one(setting: &str, value: serde_json::Value) -> Vec<Assignment> {
    vec![Assignment {
        setting: setting.to_owned(),
        value,
    }]
}

fn components(x: f64, y: f64) -> Vec<Assignment> {
    let mut both = one(WIND_X, json!({ "value": x, "unit": "m/s" }));
    both.extend(one(WIND_Y, json!({ "value": y, "unit": "m/s" })));
    both
}

#[test]
fn sc3_list_range_and_steps_each_give_the_points_a_person_expects() {
    let (_dir, path) = cavity_document("sc3-modes");
    let before = snapshot(&path);

    // A list: the values in the order given.
    let out = sweep_plan(
        &path,
        &sweep("lids", vec![lid_speed(list(&[0.1, 0.25, 0.4]))]),
    );
    passed(&out.report);
    let data = planned(&out);
    assert_eq!(data.points.len(), 3);
    let ids: Vec<&str> = data.points.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["lids-001", "lids-002", "lids-003"]);
    let labels: Vec<&str> = data.points.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(labels, ["lid_speed 0.1", "lid_speed 0.25", "lid_speed 0.4"]);
    assert_eq!(data.points[1].assignments, one(LID, json!(0.25)));

    // A range from 20 to 80 in steps of 20: four points, the stop included.
    let range = Values::Range {
        start: 20.0,
        stop: 80.0,
        step: 20.0,
    };
    let out = sweep_plan(&path, &sweep("cells", vec![cells_x(range)]));
    passed(&out.report);
    let data = planned(&out);
    let values: Vec<f64> = data.points.iter().map(|p| p.values[0]).collect();
    assert_eq!(values, [20.0, 40.0, 60.0, 80.0]);
    let labels: Vec<&str> = data.points.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(
        labels,
        ["cells_x 20", "cells_x 40", "cells_x 60", "cells_x 80"]
    );
    assert_eq!(data.points[3].assignments, one(CELLS_X, json!(80)));

    // Steps: five values spread evenly from 0.1 to 0.5, both ends included.
    let steps = Values::Steps {
        start: 0.1,
        stop: 0.5,
        count: 5,
    };
    let out = sweep_plan(&path, &sweep("steps", vec![lid_speed(steps)]));
    passed(&out.report);
    let data = planned(&out);
    let values: Vec<f64> = data.points.iter().map(|p| p.values[0]).collect();
    assert_eq!(values, [0.1, 0.2, 0.3, 0.4, 0.5]);
    let labels: Vec<&str> = data.points.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "lid_speed 0.1",
            "lid_speed 0.2",
            "lid_speed 0.3",
            "lid_speed 0.4",
            "lid_speed 0.5"
        ]
    );
    assert_eq!(data.points[2].assignments, one(LID, json!(0.3)));

    // Planning stores nothing.
    assert_eq!(snapshot(&path), before);
}

#[test]
fn sc3_several_parameters_give_the_full_factorial() {
    let (_dir, path) = cavity_document("sc3-factorial");
    let range = Values::Range {
        start: 20.0,
        stop: 80.0,
        step: 30.0,
    };
    let definition = sweep(
        "grid",
        vec![lid_speed(list(&[0.1, 0.2, 0.3])), cells_x(range)],
    );

    let out = sweep_plan(&path, &definition);
    passed(&out.report);
    let data = planned(&out);
    assert_eq!(data.points.len(), 9);

    // The first parameter varies slowest.
    let expected: Vec<(f64, u32)> = [0.1, 0.2, 0.3]
        .into_iter()
        .flat_map(|lid| [20u32, 50, 80].map(|cells| (lid, cells)))
        .collect();
    for (at, (lid, cells)) in expected.into_iter().enumerate() {
        let point = &data.points[at];
        assert_eq!(point.id, format!("grid-{:03}", at + 1));
        assert_eq!(point.index, at as u32);
        assert_eq!(point.values, [lid, f64::from(cells)]);
        assert_eq!(point.label, format!("lid_speed {lid}, cells_x {cells}"));
        let mut assigned = one(LID, json!(lid));
        assigned.extend(one(CELLS_X, json!(cells)));
        assert_eq!(point.assignments, assigned);
    }
    assert_eq!(data.points[0].id, "grid-001");
    assert_eq!(data.points[8].id, "grid-009");
}

#[test]
fn sc3_before_any_solve_every_points_case_is_built_and_failures_are_reported() {
    // Nine points; the three that carry a cell count of zero cannot be built into a case.
    let definition = factorial("grid", [20.0, 0.0, 50.0]);
    let (_dir, path) = document_with("sc3-failing", &definition);
    let before = snapshot(&path);

    let out = sweep_preflight(&path, "grid");
    assert_eq!(out.report.class(), ExitClass::GateFailed);
    assert_eq!(out.report.exit_class, 1);
    assert_eq!(codes(&out.report), ["CFD-SWP-003"; 3]);
    let data = checked(&out);
    assert_eq!(data.points.len(), 9);
    assert_eq!(data.failed, 3);

    // Exactly the points with the zero fail, each with the study's own reason.
    let failing: Vec<&str> = data
        .points
        .iter()
        .filter(|p| !p.ok)
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(failing, ["grid-002", "grid-005", "grid-008"]);
    for point in &data.points {
        if point.ok {
            assert!(point.reasons.is_empty(), "{}", point.id);
            assert!(point.mesh_key.is_some() && point.result_key.is_some());
        } else {
            assert_eq!(point.reasons.len(), 1, "{}: {:?}", point.id, point.reasons);
            assert!(
                point.reasons[0].contains("at least one cell in x"),
                "{}: {}",
                point.id,
                point.reasons[0]
            );
            assert!(point.mesh_key.is_none() && point.result_key.is_none());
        }
    }
    for finding in &out.report.diagnostics {
        assert!(
            finding.message.contains("at least one cell in x"),
            "{}",
            finding.message
        );
    }
    assert!(
        data.points
            .iter()
            .any(|p| !p.ok && p.label.contains("cells_x 0"))
    );

    // Two different good cell counts make two meshes.
    assert_eq!(data.distinct_meshes, 2);

    // Nothing was solved or written: no run, no stored result, and the file is as it was.
    assert_eq!(snapshot(&path), before);
    assert!(open(&path).unwrap().document.artifacts.runs.is_empty());
    assert!(tree(&store_dir(&path)).is_empty());
}

#[test]
fn sc3_points_that_differ_only_in_a_setting_share_one_mesh() {
    let definition = sweep("lids", vec![lid_speed(list(&[0.1, 0.2, 0.3]))]);
    let (_dir, path) = document_with("sc3-share", &definition);

    let out = sweep_preflight(&path, "lids");
    passed(&out.report);
    let data = checked(&out);
    assert_eq!(data.failed, 0);
    assert_eq!(data.distinct_meshes, 1);
    let shared: Vec<Option<&str>> = data
        .points
        .iter()
        .map(|p| p.mesh_shared_with.as_deref())
        .collect();
    assert_eq!(shared, [None, Some("lids-001"), Some("lids-001")]);
    let meshes: std::collections::HashSet<_> = data.points.iter().map(|p| &p.mesh_key).collect();
    assert_eq!(meshes.len(), 1);
    let results: std::collections::HashSet<_> =
        data.points.iter().map(|p| p.result_key.clone()).collect();
    assert_eq!(results.len(), 3);
}

#[test]
fn sc3_a_direction_parameter_rotates_a_vector_setting() {
    let (_dir, path) = cavity_document("sc3-direction");

    let out = sweep_plan(&path, &wind());
    passed(&out.report);
    let data = planned(&out);
    assert_eq!(data.points.len(), 4);
    let expected = [
        ("wind_dir 0", 10.0, 0.0),
        ("wind_dir 90", 0.0, 10.0),
        ("wind_dir 180", -10.0, 0.0),
        ("wind_dir 270", 0.0, -10.0),
    ];
    for (point, (label, x, y)) in data.points.iter().zip(expected) {
        assert_eq!(point.label, label);
        assert_eq!(point.assignments, components(x, y), "{label}");
    }

    // The cavity has no such setting, so every point of the stored sweep fails the check, and
    // each reason says the change does not apply to the study.
    passed(&sweep_create(&path, &wind()).report);
    let out = sweep_preflight(&path, "wind");
    assert_eq!(out.report.class(), ExitClass::GateFailed);
    assert_eq!(codes(&out.report), ["CFD-SWP-003"; 4]);
    let data = checked(&out);
    assert_eq!(data.failed, 4);
    assert_eq!(data.distinct_meshes, 0);
    for point in &data.points {
        assert!(!point.ok, "{}", point.id);
        assert_eq!(point.reasons.len(), 1, "{}: {:?}", point.id, point.reasons);
        assert!(
            point.reasons[0].contains("does not apply to the study"),
            "{}: {}",
            point.id,
            point.reasons[0]
        );
        assert!(point.reasons[0].contains(&point.id));
    }
}

#[test]
fn sc3_a_sweep_over_the_limit_is_refused_by_name() {
    let (_dir, path) = cavity_document("sc3-limit");
    let before = snapshot(&path);

    for out in [
        sweep_plan(&path, &many_cells("big", 201)),
        sweep_create(&path, &many_cells("big", 201)),
    ] {
        assert_eq!(out.report.class(), ExitClass::StudyInvalid);
        assert_eq!(codes(&out.report), ["CFD-SWP-001"]);
        assert_eq!(
            out.report.diagnostics[0].message,
            "Sweep big would make 201 points, more than the 200 allowed."
        );
        assert_eq!(out.line, None);
    }
    assert_eq!(snapshot(&path), before);
    assert!(open(&path).unwrap().document.artifacts.sweeps.is_empty());

    // Exactly the limit plans fine.
    let out = sweep_plan(&path, &many_cells("big", 200));
    passed(&out.report);
    assert_eq!(planned(&out).points.len(), 200);
}

#[test]
fn sc3_a_created_point_is_solved_like_any_variant_after_the_check() {
    let (dir, path) = document_with("sc3-solve", &grid());
    let out = sweep_preflight(&path, "grid");
    passed(&out.report);
    assert_eq!(checked(&out).distinct_meshes, 2);

    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let done = solve_scenario(&path, Some("grid-003"), &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 1);
    assert_eq!(data(&done.report).recorded, ["mesh", "result"]);

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].scenario.as_deref(), Some("grid-003"));

    // The other points are untouched: their results were never made.
    for id in ["grid-001", "grid-002", "grid-004", "grid-005", "grid-006"] {
        let out = scenario_status(&path, Some(id));
        passed(&out.report);
        assert_eq!(stages(&out).result.state, "never_made", "{id}");
    }
    // The point that shares its mesh with the solved one finds it.
    let sibling = scenario_status(&path, Some("grid-001"));
    assert_eq!(stages(&sibling).mesh.state, "fresh");
    assert_eq!(stages(&sibling).mesh.run.as_deref(), Some("run-1"));
    let own = scenario_status(&path, Some("grid-003"));
    assert_eq!(stages(&own).result.state, "fresh");

    // Every point is still listed as a variant of the sweep.
    let listed = scenario_list(&path);
    passed(&listed.report);
    match listed.report.repro {
        Some(Repro::Scenarios(rows)) => {
            let from_sweep = rows
                .scenarios
                .iter()
                .filter(|row| row.sweep.as_deref() == Some("grid"))
                .count();
            assert_eq!(from_sweep, 6);
        }
        other => panic!("expected the list of variants, found {other:?}"),
    }
}

/// The document at `path` with the sweeps, variants and runs it holds.
fn held(path: &Path) -> cfdcraft_doc::ArtifactIndex {
    open(path).unwrap().document.artifacts
}

#[test]
fn sc3_the_sweep_survives_a_pack_and_an_unpack() {
    let (dir, path) = document_with("sc3-pack", &grid());
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_scenario(&path, Some("grid-002"), &mut runner).report);

    let packed = dir.join("cavity.cfdpack");
    passed(&pack_document(&path, &packed).report);
    let there = dir.join("unpacked");
    passed(&unpack_document(&packed, &there).report);

    let before = held(&path);
    let after = held(&there.join("cavity.cfdcraft"));
    assert_eq!(after.sweeps, vec![grid()]);
    assert_eq!(after.sweeps, before.sweeps);
    assert_eq!(after.scenarios, before.scenarios);
    assert_eq!(after.scenarios.len(), 6);
    for (at, variant) in after.scenarios.iter().enumerate() {
        let origin = variant.origin.as_ref().expect("a point keeps its origin");
        assert_eq!(origin.sweep, "grid");
        assert_eq!(origin.index as usize, at);
    }
    assert_eq!(after.runs, before.runs);

    // The unpacked sweep still checks.
    let out = sweep_preflight(&there.join("cavity.cfdcraft"), "grid");
    passed(&out.report);
    assert_eq!(checked(&out).failed, 0);
}
