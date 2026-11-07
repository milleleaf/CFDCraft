//! Planning and creating sweeps: what is stored, what is refused, and that a refusal leaves the
//! file and its store as they were. A sweep point is a variant like any other, so the tests also
//! list and solve the created points.

use std::path::{Path, PathBuf};

use cfdcraft_ops::scenario_ops::{scenario_list, scenario_set};
use cfdcraft_ops::sweep_ops::{sweep_create, sweep_plan};
use cfdcraft_ops::{DocOutcome, ExitClass, Repro, ScenarioSummary, SweepData, solve_scenario};
use cfdcraft_scenarios::{Origin, Override, Scenario};
use cfdcraft_sweep::{Assignment, Parameter, Sweep, Target, Values};
use cfdcraft_text::Line;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";

fn number(name: &str, setting: &str, items: &[f64]) -> Parameter {
    Parameter {
        name: name.to_owned(),
        target: Target::Number {
            setting: setting.to_owned(),
        },
        values: Values::List {
            items: items.to_vec(),
        },
    }
}

fn count(name: &str, setting: &str, items: &[f64]) -> Parameter {
    Parameter {
        name: name.to_owned(),
        target: Target::Count {
            setting: setting.to_owned(),
        },
        values: Values::List {
            items: items.to_vec(),
        },
    }
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

const GRID_LIDS: [f64; 6] = [0.1, 0.1, 0.2, 0.2, 0.3, 0.3];
const GRID_CELLS: [u32; 6] = [20, 40, 20, 40, 20, 40];

/// A scratch folder and a new document of the cavity in it.
fn cavity_document(tag: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    (dir, path)
}

fn lid(value: f64) -> Override {
    Override::Set {
        setting: LID.to_owned(),
        value: json!(value),
    }
}

fn sweep_data(out: &DocOutcome) -> &SweepData {
    match &out.report.repro {
        Some(Repro::Sweep(data)) => data.as_ref(),
        other => panic!("expected the points of a sweep, found {other:?}"),
    }
}

fn stored(path: &Path) -> cfdcraft_doc::Document {
    cfdcraft_doc::open(path).unwrap().document
}

/// The refusal an operation answered with: it has this code and class 2, and no line.
fn refused_with(out: &DocOutcome, code: &str) {
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&out.report), [code], "{}", out.report.render_text());
    assert_eq!(out.line, None);
}

fn message(out: &DocOutcome) -> &str {
    &out.report.diagnostics[0].message
}

fn listed(path: &Path) -> Vec<ScenarioSummary> {
    let out = scenario_list(path);
    passed(&out.report);
    match out.report.repro {
        Some(Repro::Scenarios(data)) => data.scenarios,
        other => panic!("expected the list of variants, found {other:?}"),
    }
}

fn labels() -> Vec<String> {
    GRID_LIDS
        .iter()
        .zip(GRID_CELLS)
        .map(|(lid, cells)| format!("lid_speed {lid}, cells_x {cells}"))
        .collect()
}

#[test]
fn a_sweep_is_planned_without_storing_anything() {
    let (_dir, path) = cavity_document("swp-plan");
    let before = snapshot(&path);

    let out = sweep_plan(&path, &grid());
    passed(&out.report);
    assert_eq!(out.report.exit_class, 0);
    let data = sweep_data(&out);
    assert_eq!(data.sweep, "grid");
    assert_eq!(data.base, "base");
    assert!(!data.stored);
    let ids: Vec<&str> = data.points.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "grid-001", "grid-002", "grid-003", "grid-004", "grid-005", "grid-006"
        ]
    );
    let planned: Vec<&str> = data.points.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(planned, labels());
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_sweep_is_created_as_variants_with_origin() {
    let (_dir, path) = cavity_document("swp-create");
    let before = stored(&path).study;

    let out = sweep_create(&path, &grid());
    passed(&out.report);
    assert_eq!(
        out.line,
        Some(Line::SweepCreated {
            sweep: "grid".to_owned(),
            points: "6".to_owned()
        })
    );
    assert!(sweep_data(&out).stored);
    assert_eq!(sweep_data(&out).points.len(), 6);

    let document = stored(&path);
    assert_eq!(document.study, before);
    assert_eq!(document.artifacts.sweeps, vec![grid()]);
    let variants = &document.artifacts.scenarios;
    assert_eq!(variants.len(), 6);
    let names = labels();
    for (at, variant) in variants.iter().enumerate() {
        assert_eq!(variant.id, format!("grid-{:03}", at + 1));
        assert_eq!(variant.parent.as_deref(), Some("base"));
        assert_eq!(
            variant.origin,
            Some(Origin {
                sweep: "grid".to_owned(),
                index: at as u32
            })
        );
        assert_eq!(variant.name.as_deref(), Some(names[at].as_str()));
        assert!(!variant.pinned);
        assert_eq!(variant.notes, None);
        let cells = Override::Set {
            setting: CELLS_X.to_owned(),
            value: json!(GRID_CELLS[at]),
        };
        assert_eq!(variant.overrides, vec![lid(GRID_LIDS[at]), cells]);
    }
}

#[test]
fn the_created_variants_list_with_their_kinds() {
    let (_dir, path) = cavity_document("swp-list");
    passed(&sweep_create(&path, &grid()).report);

    let rows = listed(&path);
    let expected: [(&str, Option<&str>); 6] = [
        ("grid-001", None),
        ("grid-002", None),
        ("grid-003", Some("grid-001")),
        ("grid-004", Some("grid-002")),
        ("grid-005", Some("grid-001")),
        ("grid-006", Some("grid-002")),
    ];
    for (id, shares) in expected {
        let row = rows.iter().find(|row| row.id == id).unwrap();
        assert_eq!(row.kind, "geometry", "{id}");
        assert_eq!(row.sweep.as_deref(), Some("grid"), "{id}");
        assert_eq!(row.parent.as_deref(), Some("base"), "{id}");
        assert_eq!(row.overrides, 2, "{id}");
        assert_eq!(row.mesh_shared_with.as_deref(), shares, "{id}");
    }
}

#[test]
fn a_created_point_can_be_solved_like_any_variant() {
    let (dir, path) = cavity_document("swp-solve");
    passed(&sweep_create(&path, &grid()).report);

    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let done = solve_scenario(&path, Some("grid-003"), &mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 1);
    let runs = stored(&path).artifacts.runs;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].scenario.as_deref(), Some("grid-003"));
}

#[test]
fn a_sweep_over_a_variant_uses_it_as_the_parent() {
    let (_dir, path) = cavity_document("swp-parent");
    passed(&scenario_set(&path, "fast-lid", None, &[lid(0.2)], None).report);

    let cells = sweep(
        "wide",
        "fast-lid",
        vec![count("cells_x", CELLS_X, &[20.0, 40.0])],
    );
    let out = sweep_create(&path, &cells);
    passed(&out.report);
    assert_eq!(sweep_data(&out).base, "fast-lid");

    let variants = stored(&path).artifacts.scenarios;
    let ids: Vec<&str> = variants.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["fast-lid", "wide-001", "wide-002"]);
    assert_eq!(variants[1].parent.as_deref(), Some("fast-lid"));
    assert_eq!(variants[2].parent.as_deref(), Some("fast-lid"));
}

#[test]
fn creating_a_sweep_twice_is_refused() {
    let (_dir, path) = cavity_document("swp-twice");
    passed(&sweep_create(&path, &grid()).report);
    let before = snapshot(&path);

    let out = sweep_create(&path, &grid());
    refused_with(&out, "CFD-SWP-002");
    assert!(message(&out).contains("grid"), "{}", message(&out));
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_point_id_that_is_already_a_variant_is_refused() {
    let (_dir, path) = cavity_document("swp-clash");
    passed(&scenario_set(&path, "grid-002", None, &[lid(0.2)], None).report);
    let before = snapshot(&path);

    let out = sweep_create(&path, &grid());
    refused_with(&out, "CFD-SWP-002");
    assert!(message(&out).contains("grid-002"), "{}", message(&out));
    assert_eq!(snapshot(&path), before);
    assert!(stored(&path).artifacts.sweeps.is_empty());
}

#[test]
fn an_unknown_base_is_cfd_scn_001() {
    let (_dir, path) = cavity_document("swp-base");
    let before = snapshot(&path);
    let ghost = sweep("grid", "nope", vec![number("lid_speed", LID, &[0.1])]);

    for out in [sweep_plan(&path, &ghost), sweep_create(&path, &ghost)] {
        refused_with(&out, "CFD-SCN-001");
        assert!(message(&out).contains("nope"), "{}", message(&out));
    }
    assert_eq!(snapshot(&path), before);
}

#[test]
fn a_long_label_is_not_used_as_a_name() {
    let (_dir, path) = cavity_document("swp-label");
    let long = sweep(
        "long",
        "base",
        vec![
            number(&"a".repeat(24), "/s/a", &[1.0]),
            number(&"b".repeat(24), "/s/b", &[1.0]),
            number(&"c".repeat(24), "/s/c", &[1.0]),
            number(&"d".repeat(24), "/s/d", &[1.0]),
        ],
    );
    let out = sweep_create(&path, &long);
    passed(&out.report);
    assert!(sweep_data(&out).points[0].label.chars().count() > 80);

    let variants = stored(&path).artifacts.scenarios;
    assert_eq!(variants.len(), 1);
    assert_eq!(variants[0].id, "long-001");
    assert_eq!(variants[0].name, None);
    assert_eq!(variants[0].overrides.len(), 4);
}

/// A sweep over `count` values of the cells in x, whole numbers from 20 up.
fn cells(id: &str, count: u32) -> Sweep {
    let values = Values::Steps {
        start: 20.0,
        stop: f64::from(19 + count),
        count,
    };
    sweep(
        id,
        "base",
        vec![Parameter {
            name: "cells_x".to_owned(),
            target: Target::Count {
                setting: CELLS_X.to_owned(),
            },
            values,
        }],
    )
}

#[test]
fn a_sweep_over_the_point_limit_is_refused_by_name() {
    let (_dir, path) = cavity_document("swp-limit");
    let before = snapshot(&path);
    let big = cells("grid", 201);

    for out in [sweep_plan(&path, &big), sweep_create(&path, &big)] {
        refused_with(&out, "CFD-SWP-001");
        assert_eq!(
            message(&out),
            "Sweep grid would make 201 points, more than the 200 allowed."
        );
    }
    assert_eq!(snapshot(&path), before);

    // Exactly the limit is accepted.
    let edge = sweep_plan(&path, &cells("grid", 200));
    passed(&edge.report);
    assert_eq!(sweep_data(&edge).points.len(), 200);
}

#[test]
fn too_many_variants_in_total_are_refused() {
    let (_dir, path) = cavity_document("swp-total");
    let mut document = stored(&path);
    document.artifacts.scenarios = (0..900)
        .map(|at| Scenario {
            id: format!("v-{at}"),
            parent: Some("base".to_owned()),
            name: None,
            pinned: false,
            notes: None,
            overrides: vec![lid(0.2)],
            origin: None,
        })
        .collect();
    cfdcraft_doc::save(&document, &path).unwrap();
    let before = snapshot(&path);

    let out = sweep_create(&path, &cells("grid", 200));
    refused_with(&out, "CFD-SWP-002");
    assert!(message(&out).contains("1000"), "{}", message(&out));
    assert_eq!(snapshot(&path), before);
    assert_eq!(stored(&path).artifacts.scenarios.len(), 900);
}

#[test]
fn a_bad_sweep_is_cfd_swp_002_with_the_reason() {
    let (_dir, path) = cavity_document("swp-bad");
    let before = snapshot(&path);
    let mut range = grid();
    range.parameters[0].values = Values::Range {
        start: 0.1,
        stop: 0.3,
        step: 0.0,
    };
    let mut twice = grid();
    twice.parameters[1].name = "lid_speed".to_owned();
    let mut capital = grid();
    capital.id = "Grid".to_owned();
    let bad = [
        ("an empty parameter list", sweep("grid", "base", Vec::new())),
        ("a step of zero", range),
        ("a duplicate parameter name", twice),
        (
            "a setting without a slash",
            sweep("grid", "base", vec![number("lid_speed", "mesh", &[0.1])]),
        ),
        ("a sweep id with a capital", capital),
        (
            "a count of 2.5",
            sweep("grid", "base", vec![count("cells_x", CELLS_X, &[2.5])]),
        ),
    ];
    for (what, definition) in &bad {
        for out in [
            sweep_plan(&path, definition),
            sweep_create(&path, definition),
        ] {
            assert_eq!(out.report.class(), ExitClass::StudyInvalid, "{what}");
            assert_eq!(codes(&out.report), ["CFD-SWP-002"], "{what}");
            assert!(!message(&out).is_empty(), "{what}");
        }
    }
    assert_eq!(snapshot(&path), before);
}

#[test]
fn the_direction_parameter_emits_the_rotated_components_as_variants() {
    let (_dir, path) = cavity_document("swp-direction");
    let wind = sweep(
        "wind",
        "base",
        vec![Parameter {
            name: "wind_dir".to_owned(),
            target: Target::Direction {
                setting_x: "/boundary_conditions/lid/wind_x".to_owned(),
                setting_y: "/boundary_conditions/lid/wind_y".to_owned(),
                unit: "m/s".to_owned(),
                base_x: 10.0,
                base_y: 0.0,
            },
            values: Values::Range {
                start: 0.0,
                stop: 270.0,
                step: 90.0,
            },
        }],
    );

    let out = sweep_plan(&path, &wind);
    passed(&out.report);
    let data = sweep_data(&out);
    assert_eq!(data.points.len(), 4);
    let components = |x: f64, y: f64| {
        vec![
            Assignment {
                setting: "/boundary_conditions/lid/wind_x".to_owned(),
                value: json!({ "value": x, "unit": "m/s" }),
            },
            Assignment {
                setting: "/boundary_conditions/lid/wind_y".to_owned(),
                value: json!({ "value": y, "unit": "m/s" }),
            },
        ]
    };
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
}

#[test]
fn a_stored_sweep_survives_a_save_and_a_reopen() {
    let (_dir, path) = cavity_document("swp-reopen");
    let mixed = sweep(
        "mixed",
        "base",
        vec![
            Parameter {
                name: "length".to_owned(),
                target: Target::Quantity {
                    setting: "/domain/length_x".to_owned(),
                    unit: "m".to_owned(),
                },
                values: Values::Range {
                    start: 0.1,
                    stop: 0.3,
                    step: 0.1,
                },
            },
            Parameter {
                name: "wind".to_owned(),
                target: Target::Direction {
                    setting_x: "/a".to_owned(),
                    setting_y: "/b".to_owned(),
                    unit: "m/s".to_owned(),
                    base_x: 3.0,
                    base_y: 4.0,
                },
                values: Values::List {
                    items: vec![0.0, 30.0],
                },
            },
        ],
    );
    passed(&sweep_create(&path, &mixed).report);
    assert_eq!(stored(&path).artifacts.sweeps, vec![mixed]);
}

#[test]
fn creating_a_sweep_on_a_pack_is_refused() {
    let (dir, path) = cavity_document("swp-pack");
    let packed = dir.join("cavity.cfdpack");
    std::fs::copy(&path, &packed).unwrap();
    let before = std::fs::read(&packed).unwrap();

    let out = sweep_create(&packed, &grid());
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&out.report), ["CFD-DOC-009"]);
    assert_eq!(std::fs::read(&packed).unwrap(), before);
}

#[test]
fn the_reports_name_no_path() {
    let (dir, path) = cavity_document("swp-paths");
    let outcomes = [
        sweep_plan(&path, &grid()),
        sweep_create(&path, &grid()),
        sweep_create(&path, &grid()),
        sweep_plan(&path, &cells("big", 201)),
    ];
    let folder = dir.display().to_string();
    for out in &outcomes {
        let json = out.report.to_json();
        assert!(!json.contains(&folder), "{json}");
        assert!(!json.contains("cavity.cfdcraft"), "{json}");
    }
}
