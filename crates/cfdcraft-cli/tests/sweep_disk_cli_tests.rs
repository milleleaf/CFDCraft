//! The verbs for sweeps, disk use and pruning, run through the real binary. The solved
//! documents are made with a runner that writes a prepared results folder, so no test needs a
//! compute host, and none of these verbs starts a run.

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cfdcraft_ops::scenario_ops::scenario_set;
use cfdcraft_ops::{solve_document, solve_scenario};
use cfdcraft_scenarios::Override;
use repro_support::{CAVITY, FixtureRunner, new_document, passed, scratch, snapshot, tree};
use serde_json::{Value, json};

const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .output()
        .unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("the binary ended by itself")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn data(o: &Output) -> Value {
    serde_json::from_str(&out(o)).expect("stdout is the JSON data")
}

fn json_out(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).expect("stdout is exactly one JSON document")
}

fn parameter(name: &str, target_kind: &str, setting: &str, items: &[f64]) -> Value {
    json!({
        "name": name,
        "target": { "kind": target_kind, "setting": setting },
        "values": { "mode": "list", "items": items },
    })
}

/// Three lid speeds by two cell counts, six points.
fn grid() -> Value {
    json!({
        "id": "grid",
        "base": "base",
        "parameters": [
            parameter("lid_speed", "number", LID, &[0.1, 0.2, 0.3]),
            parameter("cells_x", "count", CELLS_X, &[20.0, 40.0]),
        ],
    })
}

/// A new document of the cavity, and a sweep file beside it.
fn with_spec(tag: &str, spec: &Value) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let doc = new_document(&dir, "cavity", CAVITY);
    let file = dir.join("sweep.json");
    fs::write(&file, spec.to_string()).unwrap();
    (doc, file)
}

/// A document of the cavity solved three times: the study, `fast-lid` on its mesh, and `fine`
/// with a mesh of its own.
fn solved(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    let doc = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = vec![0.0, 0.05, 0.1];
    passed(&solve_document(&doc, &mut runner).report);
    let made = [("fast-lid", LID, json!(0.2)), ("fine", CELLS_X, json!(256))];
    for (id, setting, value) in made {
        let change = Override::Set {
            setting: setting.to_owned(),
            value,
        };
        passed(&scenario_set(&doc, id, Some("base"), &[change], None).report);
        passed(&solve_scenario(&doc, Some(id), &mut runner).report);
    }
    doc
}

#[test]
fn sweep_plan_prints_the_points_and_stores_nothing() {
    let (doc, spec) = with_spec("cli-swp-plan", &grid());
    let before = fs::read(&doc).unwrap();
    let o = run(&["sweep", "plan", s(&doc), s(&spec)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = data(&o);
    let ids: Vec<&str> = shown["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|point| point["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "grid-001", "grid-002", "grid-003", "grid-004", "grid-005", "grid-006"
        ]
    );
    assert_eq!(fs::read(&doc).unwrap(), before);
}

#[test]
fn sweep_create_stores_the_variants_and_prints_the_sentence() {
    let (doc, spec) = with_spec("cli-swp-create", &grid());
    let o = run(&["sweep", "create", s(&doc), s(&spec)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Created sweep grid: 6 variants."),
        "{}",
        out(&o)
    );
    let o = run(&["scenario", "list", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let listed = data(&o);
    let ids: Vec<&str> = listed["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "base", "grid-001", "grid-002", "grid-003", "grid-004", "grid-005", "grid-006"
        ]
    );
}

#[test]
fn sweep_preflight_passes_and_counts_meshes() {
    let (doc, spec) = with_spec("cli-swp-pre", &grid());
    let o = run(&["sweep", "create", s(&doc), s(&spec)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let o = run(&["sweep", "preflight", s(&doc), "grid"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = data(&o);
    assert_eq!(shown["distinct_meshes"], 2);
    assert_eq!(shown["failed"], 0);
}

#[test]
fn sweep_preflight_exits_1_and_names_the_failing_point() {
    let cells = json!({
        "id": "cells",
        "base": "base",
        "parameters": [parameter("cells_x", "count", CELLS_X, &[20.0, 0.0, 40.0])],
    });
    let (doc, spec) = with_spec("cli-swp-fail", &cells);
    let o = run(&["sweep", "create", s(&doc), s(&spec)]);
    assert_eq!(code(&o), 0, "{}", err(&o));

    let o = run(&["sweep", "preflight", s(&doc), "cells"]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(err(&o).contains("cells-002"), "{}", err(&o));
    assert!(err(&o).contains("at least one cell"), "{}", err(&o));
    assert!(out(&o).is_empty(), "{}", out(&o));

    let o = run(&["sweep", "preflight", s(&doc), "cells", "--json"]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    let report = json_out(&o);
    assert_eq!(report["repro"]["kind"], "preflight");
    assert_eq!(report["repro"]["failed"], 1);
    assert_eq!(report["repro"]["points"][1]["id"], "cells-002");
    assert_eq!(report["repro"]["points"][1]["ok"], false);
}

#[test]
fn a_spec_that_is_not_a_sweep_is_exit_2() {
    let (doc, _) = with_spec("cli-swp-badspec", &grid());
    let dir = doc.parent().unwrap();
    let unknown = dir.join("unknown.json");
    fs::write(
        &unknown,
        json!({"id": "x", "base": "base", "parameters": [], "extra": 1}).to_string(),
    )
    .unwrap();
    let notjson = dir.join("notjson.json");
    fs::write(&notjson, "this is not json").unwrap();
    let missing = dir.join("missing.json");

    let before = fs::read(&doc).unwrap();
    for spec in [&unknown, &notjson, &missing] {
        for verb in ["plan", "create"] {
            let o = run(&["sweep", verb, s(&doc), s(spec)]);
            assert_eq!(code(&o), 2, "{verb} {}: {}", spec.display(), err(&o));
            assert!(err(&o).contains("sweep file"), "{}", err(&o));
            assert!(out(&o).is_empty(), "{}", out(&o));
        }
    }
    assert_eq!(fs::read(&doc).unwrap(), before);
}

#[test]
fn a_sweep_over_the_limit_is_exit_2_and_says_how_many() {
    let big = json!({
        "id": "grid",
        "base": "base",
        "parameters": [{
            "name": "cells_x",
            "target": { "kind": "count", "setting": CELLS_X },
            "values": { "mode": "steps", "start": 20.0, "stop": 220.0, "count": 201 },
        }],
    });
    let (doc, spec) = with_spec("cli-swp-big", &big);
    let before = fs::read(&doc).unwrap();
    for verb in ["plan", "create"] {
        let o = run(&["sweep", verb, s(&doc), s(&spec)]);
        assert_eq!(code(&o), 2, "{}", err(&o));
        assert!(
            err(&o).contains("Sweep grid would make 201 points, more than the 200 allowed."),
            "{}",
            err(&o)
        );
    }
    assert_eq!(fs::read(&doc).unwrap(), before);
}

#[test]
fn disk_prints_the_use_per_variant() {
    let doc = solved("cli-disk");
    let o = run(&["disk", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = data(&o);
    let names: Vec<&str> = shown["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["scenario"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"base"), "{names:?}");
    assert!(names.contains(&"fast-lid"), "{names:?}");
    assert!(shown["stored_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn prune_is_a_dry_run_unless_apply_is_given() {
    let doc = solved("cli-prune");
    let store = cfdcraft_doc::store_dir(&doc);
    let kept = tree(&store);

    let o = run(&["prune", s(&doc), "--scenario", "nope"]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("CFD-DISK-001"), "{}", err(&o));
    assert_eq!(tree(&store), kept);

    let before = snapshot(&doc);
    let o = run(&["prune", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Nothing was removed"), "{}", out(&o));
    assert_eq!(snapshot(&doc), before);

    let o = run(&["prune", s(&doc), "--apply"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Removed"), "{}", out(&o));
    assert_ne!(tree(&store), kept);
}

#[test]
fn every_new_verb_prints_one_json_report_with_json() {
    let (doc, spec) = with_spec("cli-json", &grid());
    let solved_doc = solved("cli-json-solved");
    let plan = ["sweep", "plan", s(&doc), s(&spec)];
    let create = ["sweep", "create", s(&doc), s(&spec)];
    let preflight = ["sweep", "preflight", s(&doc), "grid"];
    let disk = ["disk", s(&solved_doc)];
    let prune = ["prune", s(&solved_doc)];
    let scenario_list = ["scenario", "list", s(&solved_doc)];
    let scenario_show = ["scenario", "show", s(&solved_doc), "fast-lid"];
    let scenario_diff = ["scenario", "diff", s(&solved_doc), "base", "fast-lid"];
    let scenario_label = ["scenario", "label", s(&solved_doc), "fast-lid", "--pin"];
    let scenario_set = [
        "scenario",
        "set",
        s(&solved_doc),
        "other",
        "--parent",
        "fast-lid",
        "--name",
        "Other",
    ];
    for args in [
        &plan[..],
        &create,
        &preflight,
        &disk,
        &prune,
        &scenario_list,
        &scenario_show,
        &scenario_diff,
        &scenario_label,
        &scenario_set,
    ] {
        let mut all = args.to_vec();
        all.push("--json");
        let o = run(&all);
        assert_eq!(code(&o), 0, "{args:?}: {}", err(&o));
        assert!(err(&o).is_empty(), "{args:?}: {}", err(&o));
        assert!(json_out(&o).is_object(), "{args:?}");
    }
}
