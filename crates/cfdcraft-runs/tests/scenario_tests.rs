use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{
    Change, Outcome, Output, Provenance, RunRecord, RunsError, StageRecipes, StageState,
    ToolIdentity, status, status_for, verify_runs,
};
use cfdcraft_store::ContentHash;

fn cavity() -> Study {
    Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
    ))
    .unwrap()
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

/// The recipes the cavity makes with this lid speed (m/s).
fn recipes(lid: f64) -> StageRecipes {
    let mut resolved = resolve(&cavity()).unwrap();
    resolved
        .boundary_conditions
        .get_mut("lid")
        .unwrap()
        .velocity = Some(lid);
    StageRecipes::from_resolved(&resolved, &tool())
}

/// A record holding the result of `recipes`, and its mesh when `with_mesh` is set.
fn record(
    id: &str,
    recipes: &StageRecipes,
    with_mesh: bool,
    scenario: Option<&str>,
    rerun_of: Option<&str>,
) -> RunRecord {
    let keys = recipes.keys();
    let mesh_hash = ContentHash::of(b"mesh");
    RunRecord {
        id: id.to_owned(),
        rerun_of: rerun_of.map(str::to_owned),
        scenario: scenario.map(str::to_owned),
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

fn stale(since: &str, from: f64, to: f64) -> StageState {
    StageState::Stale {
        since: since.to_owned(),
        changes: vec![Change::BoundaryVelocity {
            boundary: "lid".to_owned(),
            from: Some(from),
            to: Some(to),
        }],
    }
}

/// A one-run log whose only run carries this tag.
fn tagged(tag: &str) -> Vec<RunRecord> {
    vec![record("run-1", &recipes(0.1), true, Some(tag), None)]
}

#[test]
fn a_record_without_a_scenario_reads_and_writes_as_before() {
    let plain = record("run-1", &recipes(0.1), true, None, None);
    let text = serde_json::to_string(&plain).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(value.get("scenario").is_none());
    let back: RunRecord = serde_json::from_str(&text).unwrap();
    assert_eq!(back.scenario, None);
    assert_eq!(back, plain);
}

#[test]
fn a_scenario_tag_round_trips() {
    let made = record("run-1", &recipes(0.1), true, Some("fast-lid"), None);
    let value = serde_json::to_value(&made).unwrap();
    assert_eq!(value["scenario"], "fast-lid");
    let back: RunRecord = serde_json::from_value(value).unwrap();
    assert_eq!(back, made);
}

#[test]
fn a_fresh_stage_is_found_across_scenarios() {
    let runs = [record("run-1", &recipes(0.1), true, None, None)];
    let found = status_for(&runs, &recipes(0.1), Some("fast-lid"));
    assert_eq!(found.mesh, fresh("run-1"));
    assert_eq!(found.result, fresh("run-1"));
}

#[test]
fn a_variant_with_the_same_mesh_but_a_new_result_key_has_a_fresh_mesh_and_no_result() {
    let runs = [record("run-1", &recipes(0.1), true, None, None)];
    let found = status_for(&runs, &recipes(0.2), Some("fast-lid"));
    assert_eq!(found.mesh, fresh("run-1"));
    assert_eq!(found.result, StageState::NeverMade);
}

#[test]
fn stale_is_judged_against_the_same_scenarios_own_runs() {
    let runs = [
        record("run-1", &recipes(0.1), true, None, None),
        record("run-2", &recipes(0.2), false, Some("fast-lid"), None),
    ];
    let own = status_for(&runs, &recipes(0.3), Some("fast-lid"));
    assert_eq!(own.mesh, fresh("run-1"));
    assert_eq!(own.result, stale("run-2", 0.2, 0.3));

    let study = status_for(&runs, &recipes(0.3), None);
    assert_eq!(study.result, stale("run-1", 0.1, 0.3));
}

#[test]
fn the_study_itself_ignores_scenario_runs_when_judging_stale() {
    let runs = [
        record("run-1", &recipes(0.1), true, None, None),
        record("run-2", &recipes(0.2), false, Some("fast-lid"), None),
    ];
    let found = status_for(&runs, &recipes(0.15), None);
    assert_eq!(found.result, stale("run-1", 0.1, 0.15));
}

#[test]
fn status_is_status_for_the_study_itself() {
    let runs = [
        record("run-1", &recipes(0.1), true, None, None),
        record("run-2", &recipes(0.2), false, Some("fast-lid"), None),
        record("run-3", &recipes(0.25), false, None, None),
    ];
    for lid in [0.1, 0.2, 0.25, 0.3] {
        assert_eq!(
            status(&runs, &recipes(lid)),
            status_for(&runs, &recipes(lid), None)
        );
    }
}

#[test]
fn a_rerun_never_makes_a_stage_fresh_for_any_scenario() {
    let runs = [
        record("run-1", &recipes(0.1), true, Some("fast-lid"), None),
        record(
            "run-2",
            &recipes(0.2),
            false,
            Some("fast-lid"),
            Some("run-1"),
        ),
    ];
    let found = status_for(&runs, &recipes(0.2), Some("fast-lid"));
    assert_eq!(found.mesh, fresh("run-1"));
    assert_eq!(found.result, stale("run-1", 0.1, 0.2));
}

#[test]
fn a_scenario_tag_must_be_a_plain_name_and_not_base() {
    let long = "a".repeat(41);
    for bad in ["Fast", "", long.as_str(), "base"] {
        assert_eq!(
            verify_runs(&tagged(bad)),
            Err(RunsError::BadScenario {
                run: "run-1".to_owned(),
                scenario: bad.to_owned(),
            }),
            "tag {bad:?}"
        );
    }
    for good in ["fast-lid", "sweep-007"] {
        assert_eq!(verify_runs(&tagged(good)), Ok(()), "tag {good:?}");
    }
}

#[test]
fn a_rerun_carries_the_tag_of_the_run_it_repeats() {
    let rerun = |first: Option<&str>, second: Option<&str>| {
        verify_runs(&[
            record("run-1", &recipes(0.1), true, first, None),
            record("run-2", &recipes(0.1), false, second, Some("run-1")),
        ])
    };
    let refused = Err(RunsError::RerunScenario {
        run: "run-2".to_owned(),
        of: "run-1".to_owned(),
    });
    assert_eq!(rerun(None, Some("fast-lid")), refused);
    assert_eq!(rerun(Some("fast-lid"), None), refused);
    assert_eq!(rerun(Some("fast-lid"), Some("other")), refused);
    assert_eq!(rerun(Some("fast-lid"), Some("fast-lid")), Ok(()));
    assert_eq!(rerun(None, None), Ok(()));
}

#[test]
fn the_order_of_the_checks_is_kept() {
    let runs = [
        record("run-1", &recipes(0.1), true, None, None),
        record("run-1", &recipes(0.1), true, Some("Fast"), None),
    ];
    assert_eq!(
        verify_runs(&runs),
        Err(RunsError::DuplicateId("run-1".to_owned()))
    );
}
