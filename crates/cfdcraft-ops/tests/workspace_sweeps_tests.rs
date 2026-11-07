//! The sweep, disk use and prune methods of a workspace, addressed by document name: every one
//! judges the name first, shows no path of the server, and changes only the document it names.
//! These tests use no fake host, so they run on every platform.

use std::path::PathBuf;

use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::scenario_ops::scenario_set;
use cfdcraft_ops::{ExitClass, Report, Repro, Workspace, solve_document, solve_scenario};
use cfdcraft_scenarios::Override;
use cfdcraft_store::Store;
use cfdcraft_sweep::{Parameter, Sweep, Target, Values};
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";

fn list(name: &str, target: Target, items: &[f64]) -> Parameter {
    Parameter {
        name: name.to_owned(),
        target,
        values: Values::List {
            items: items.to_vec(),
        },
    }
}

/// Three lid speeds by two cell counts: six points, two different meshes.
fn grid() -> Sweep {
    Sweep {
        id: "grid".to_owned(),
        base: "base".to_owned(),
        parameters: vec![
            list(
                "lid_speed",
                Target::Number {
                    setting: LID.to_owned(),
                },
                &[0.1, 0.2, 0.3],
            ),
            list(
                "cells_x",
                Target::Count {
                    setting: CELLS_X.to_owned(),
                },
                &[20.0, 40.0],
            ),
        ],
    }
}

/// A workspace holding the document `cavity`, its root and the document's file.
fn plain_workspace(tag: &str) -> (Workspace, PathBuf, PathBuf) {
    let dir = scratch(tag);
    let ws = Workspace::open(dir.join("ws")).unwrap();
    let root = dir.join("ws").canonicalize().unwrap();
    let path = new_document(&root.join("documents"), "cavity", CAVITY);
    (ws, root, path)
}

fn points_of(report: &Report) -> usize {
    match &report.repro {
        Some(Repro::Sweep(data)) => data.points.len(),
        other => panic!("expected the points of a sweep, found {other:?}"),
    }
}

#[test]
fn the_workspace_plans_creates_and_checks_a_sweep_by_document_name() {
    let (ws, root, path) = plain_workspace("wsw-sweep");
    let before = snapshot(&path);

    let planned = ws.sweep_plan("cavity", &grid());
    passed(&planned);
    assert_eq!(planned.operation, "sweep_plan");
    assert_eq!(points_of(&planned), 6);
    assert_eq!(snapshot(&path), before, "a plan stores nothing");

    let created = ws.sweep_create("cavity", &grid());
    passed(&created);
    assert_eq!(created.operation, "sweep_create");
    assert_eq!(points_of(&created), 6);
    let stored = cfdcraft_doc::open(&path).unwrap().document.artifacts;
    assert_eq!(stored.sweeps.len(), 1);
    assert_eq!(stored.scenarios.len(), 6);

    let checked = ws.sweep_preflight("cavity", "grid");
    passed(&checked);
    assert_eq!(checked.operation, "sweep_preflight");
    let Some(Repro::Preflight(data)) = &checked.repro else {
        panic!("expected the checks of a sweep: {checked:?}");
    };
    assert_eq!(data.points.len(), 6);
    assert_eq!(data.distinct_meshes, 2);
    assert_eq!(data.failed, 0);

    let shown_root = root.display().to_string();
    for report in [&planned, &created, &checked] {
        let json = serde_json::to_string(report).unwrap();
        assert!(!json.contains(&shown_root), "{json}");
    }
}

#[test]
fn a_bad_document_name_is_cfd_ws_001() {
    let (ws, root, _) = plain_workspace("wsw-names");
    let before = tree(&root);
    for bad in ["../x", "UPPER", "a/b", ""] {
        let scope = vec!["base".to_owned()];
        let answers = [
            ws.sweep_plan(bad, &grid()),
            ws.sweep_create(bad, &grid()),
            ws.sweep_preflight(bad, "grid"),
            ws.disk_use(bad),
            ws.prune(bad, None, false),
            ws.prune(bad, Some(scope.as_slice()), true),
        ];
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
    assert_eq!(tree(&root), before, "nothing was touched");
}

/// Solve the study, then two variants of it (`fast-lid` on the study's mesh, `fine` on its own
/// mesh), so the store holds five different results.
fn solve_three(path: &std::path::Path, dir: &std::path::Path) {
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = vec![0.0, 0.05, 0.1];
    passed(&solve_document(path, &mut runner).report);
    let made = [("fast-lid", LID, json!(0.2)), ("fine", CELLS_X, json!(256))];
    for (id, setting, value) in made {
        let change = Override::Set {
            setting: setting.to_owned(),
            value,
        };
        passed(&scenario_set(path, id, Some("base"), &[change], None).report);
        passed(&solve_scenario(path, Some(id), &mut runner).report);
    }
}

#[test]
fn the_workspace_reads_disk_use_and_prunes_by_document_name() {
    let dir = scratch("wsw-disk");
    let ws = Workspace::open(dir.join("ws")).unwrap();
    let root = dir.join("ws").canonicalize().unwrap();
    let path = new_document(&root.join("documents"), "cavity", CAVITY);
    solve_three(&path, &dir);

    let used = ws.disk_use("cavity");
    passed(&used);
    assert_eq!(used.operation, "disk_use");
    let Some(Repro::Disk(usage)) = &used.repro else {
        panic!("expected the disk use: {used:?}");
    };
    let ids: Vec<&str> = usage
        .scenarios
        .iter()
        .map(|s| s.scenario.as_str())
        .collect();
    assert_eq!(ids, ["base", "fast-lid", "fine"]);

    let before = snapshot(&path);
    let dry = ws.prune("cavity", None, false);
    passed(&dry);
    assert_eq!(dry.operation, "prune");
    let Some(Repro::Prune(plan)) = &dry.repro else {
        panic!("expected the plan of a prune: {dry:?}");
    };
    assert!(!plan.applied);
    assert_eq!(plan.plan.remove.len(), 5);
    assert_eq!(snapshot(&path), before, "a dry run removes nothing");

    let done = ws.prune("cavity", None, true);
    passed(&done);
    let Some(Repro::Prune(plan)) = &done.repro else {
        panic!("expected the data of a prune: {done:?}");
    };
    assert!(plan.applied);
    assert_eq!(plan.plan.remove.len(), 5);
    let store = Store::open(store_dir(&path)).unwrap();
    for item in &plan.plan.remove {
        assert!(!store.contains(&item.hash));
    }
    assert_eq!(open(&path).unwrap().document.artifacts.pruned.len(), 5);

    let shown_root = root.display().to_string();
    for report in [&used, &dry, &done] {
        let json = serde_json::to_string(report).unwrap();
        assert!(!json.contains(&shown_root), "{json}");
    }
}
