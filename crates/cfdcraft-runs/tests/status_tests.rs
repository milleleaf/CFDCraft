use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{
    Axis, Change, Outcome, Output, Provenance, RunRecord, StageRecipes, StageState, ToolIdentity,
    status,
};
use cfdcraft_store::ContentHash;
use serde_json::json;

fn cavity() -> Study {
    Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
    ))
    .unwrap()
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

/// The recipes the cavity makes with this lid speed (m/s) and this cell count along x.
fn recipes(lid: f64, cells_x: u64) -> StageRecipes {
    let mut resolved = resolve(&cavity()).unwrap();
    resolved
        .boundary_conditions
        .get_mut("lid")
        .unwrap()
        .velocity = Some(lid);
    resolved.mesh.cells_x = cells_x;
    StageRecipes::from_resolved(&resolved, &tool())
}

/// A record holding the result of `recipes`, and its mesh when `with_mesh` is set.
fn record(id: &str, recipes: &StageRecipes, with_mesh: bool, rerun_of: Option<&str>) -> RunRecord {
    let keys = recipes.keys();
    let mesh_hash = ContentHash::of(b"mesh");
    RunRecord {
        id: id.to_owned(),
        rerun_of: rerun_of.map(str::to_owned),
        scenario: None,
        mesh: with_mesh.then(|| Output {
            key: keys.mesh,
            recipe: recipes.mesh.clone(),
            blob: mesh_hash,
            size: 4,
        }),
        result: Some(Output {
            key: keys.result,
            recipe: recipes.result.clone(),
            blob: ContentHash::of(id.as_bytes()),
            size: 6,
        }),
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash,
            result_key: Some(keys.result),
            settings: cavity(),
            tool: tool(),
            host: None,
        },
        checks: Vec::new(),
        key_numbers: Default::default(),
        comparison: None,
        continued_from: None,
        outcome: Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    }
}

fn fresh(run: &str) -> StageState {
    StageState::Fresh {
        run: run.to_owned(),
    }
}

fn lid(from: f64, to: f64) -> Change {
    Change::BoundaryVelocity {
        boundary: "lid".to_owned(),
        from: Some(from),
        to: Some(to),
    }
}

#[test]
fn a_document_without_runs_has_nothing_made() {
    let found = status(&[], &recipes(0.1, 128));
    assert_eq!(found.mesh, StageState::NeverMade);
    assert_eq!(found.result, StageState::NeverMade);
}

#[test]
fn the_same_study_is_fresh_in_both_stages() {
    let made = recipes(0.1, 128);
    let found = status(&[record("run-1", &made, true, None)], &made);
    assert_eq!(found.mesh, fresh("run-1"));
    assert_eq!(found.result, fresh("run-1"));
}

#[test]
fn a_lid_speed_change_stales_the_result_and_not_the_mesh() {
    let runs = [record("run-1", &recipes(0.1, 128), true, None)];
    let found = status(&runs, &recipes(0.2, 128));
    assert_eq!(found.mesh, fresh("run-1"));
    assert_eq!(
        found.result,
        StageState::Stale {
            since: "run-1".to_owned(),
            changes: vec![lid(0.1, 0.2)],
        }
    );
}

#[test]
fn a_cell_count_change_stales_both() {
    let runs = [record("run-1", &recipes(0.1, 128), true, None)];
    let found = status(&runs, &recipes(0.1, 256));
    assert_eq!(
        found.mesh,
        StageState::Stale {
            since: "run-1".to_owned(),
            changes: vec![Change::Cells {
                axis: Axis::X,
                from: 128,
                to: 256,
            }],
        }
    );
    assert_eq!(
        found.result,
        StageState::Stale {
            since: "run-1".to_owned(),
            changes: vec![Change::MeshChanged],
        }
    );
}

#[test]
fn going_back_makes_a_stage_fresh_again() {
    let runs = [
        record("run-1", &recipes(0.1, 128), true, None),
        record("run-2", &recipes(0.2, 128), false, None),
    ];
    let back = status(&runs, &recipes(0.1, 128));
    assert_eq!(back.mesh, fresh("run-1"));
    assert_eq!(back.result, fresh("run-1"));

    let newest = status(&runs, &recipes(0.2, 128));
    assert_eq!(newest.mesh, fresh("run-1"));
    assert_eq!(newest.result, fresh("run-2"));

    let other = status(&runs, &recipes(0.3, 128));
    assert_eq!(other.mesh, fresh("run-1"));
    assert_eq!(
        other.result,
        StageState::Stale {
            since: "run-2".to_owned(),
            changes: vec![lid(0.2, 0.3)],
        }
    );
}

#[test]
fn a_rerun_does_not_make_a_stage_fresh() {
    let runs = [
        record("run-1", &recipes(0.1, 128), true, None),
        record("run-2", &recipes(0.2, 128), false, Some("run-1")),
    ];
    let found = status(&runs, &recipes(0.2, 128));
    assert_eq!(found.mesh, fresh("run-1"));
    assert_eq!(
        found.result,
        StageState::Stale {
            since: "run-1".to_owned(),
            changes: vec![lid(0.1, 0.2)],
        }
    );
}

#[test]
fn a_rerun_that_holds_a_mesh_does_not_make_the_mesh_fresh() {
    // This code never writes one, but a file from elsewhere may carry it.
    let runs = [
        record("run-1", &recipes(0.1, 128), true, None),
        record("run-2", &recipes(0.1, 256), true, Some("run-1")),
    ];
    let found = status(&runs, &recipes(0.1, 256));
    match found.mesh {
        StageState::Stale { since, .. } => assert_eq!(since, "run-1"),
        other => panic!("expected stale, found {other:?}"),
    }
}

#[test]
fn stale_names_the_newest_run_that_recorded_the_stage() {
    let runs = [
        record("run-1", &recipes(0.1, 128), true, None),
        record("run-2", &recipes(0.2, 128), false, None),
    ];
    let found = status(&runs, &recipes(0.3, 256));
    match found.result {
        StageState::Stale { since, .. } => assert_eq!(since, "run-2"),
        other => panic!("expected stale, found {other:?}"),
    }
    match found.mesh {
        StageState::Stale { since, .. } => assert_eq!(since, "run-1"),
        other => panic!("expected stale, found {other:?}"),
    }
}

#[test]
fn the_state_serialises_with_stable_names() {
    assert_eq!(
        serde_json::to_value(fresh("run-1")).unwrap(),
        json!({"state": "fresh", "run": "run-1"})
    );
    assert_eq!(
        serde_json::to_value(StageState::NeverMade).unwrap(),
        json!({"state": "never_made"})
    );
    let stale = StageState::Stale {
        since: "run-1".to_owned(),
        changes: vec![lid(0.1, 0.2)],
    };
    let value = serde_json::to_value(&stale).unwrap();
    assert_eq!(value["state"], "stale");
    assert_eq!(value["since"], "run-1");
    assert_eq!(value["changes"][0]["kind"], "boundary_velocity");
    let back: StageState = serde_json::from_value(value).unwrap();
    assert_eq!(back, stale);
}
