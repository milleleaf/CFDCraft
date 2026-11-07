use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{
    Check, KeyNumbers, MAX_RUNS, Output, Provenance, RunRecord, RunsError, Stage, StageRecipes,
    ToolIdentity, is_plain_name, redact_hosts, verify_runs,
};
use cfdcraft_store::ContentHash;

fn cavity() -> Study {
    Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
    ))
    .unwrap()
}

/// The cavity with a faster lid: the same mesh, another result.
fn faster_lid() -> Study {
    let mut study = cavity();
    study
        .boundary_conditions
        .get_mut("lid")
        .unwrap()
        .velocity
        .as_mut()
        .unwrap()
        .value = 0.2;
    study
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn recipes_of(study: &Study) -> StageRecipes {
    StageRecipes::from_resolved(&resolve(study).unwrap(), &tool())
}

/// A run of `study` that recorded both stages.
fn record_for(id: &str, study: &Study) -> RunRecord {
    let recipes = recipes_of(study);
    let keys = recipes.keys();
    let mesh_hash = ContentHash::of(b"mesh");
    RunRecord {
        id: id.to_owned(),
        rerun_of: None,
        scenario: None,
        mesh: Some(Output {
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
            settings: study.clone(),
            tool: tool(),
            host: Some("box".to_owned()),
        },
        checks: vec![Check {
            name: "residuals".to_owned(),
            passed: true,
            detail: "all below the limit".to_owned(),
        }],
        key_numbers: KeyNumbers::from([("solver.iterations".to_owned(), 120.0)]),
        comparison: None,
    }
}

fn full_record(id: &str) -> RunRecord {
    record_for(id, &cavity())
}

/// A run that recorded only a result, on the mesh an earlier run made.
fn result_only(id: &str, study: &Study) -> RunRecord {
    let mut record = record_for(id, study);
    record.mesh = None;
    record
}

fn rerun_record(id: &str, of: &str) -> RunRecord {
    let mut record = result_only(id, &cavity());
    record.rerun_of = Some(of.to_owned());
    record
}

/// A record that recorded neither stage.
fn bare_record(id: &str) -> RunRecord {
    let mut record = full_record(id);
    record.mesh = None;
    record.result = None;
    record.provenance.result_key = None;
    record
}

#[test]
fn a_consistent_log_verifies() {
    let runs = [
        full_record("run-1"),
        result_only("run-2", &faster_lid()),
        rerun_record("run-3", "run-1"),
    ];
    assert_eq!(verify_runs(&runs), Ok(()));
    assert_eq!(verify_runs(&[]), Ok(()));
}

#[test]
fn a_key_that_does_not_match_its_recipe_is_refused() {
    let mut mesh_edited = full_record("run-1");
    mesh_edited.mesh.as_mut().unwrap().recipe.geometry.length_x += 1.0;
    assert_eq!(
        verify_runs(&[mesh_edited]),
        Err(RunsError::KeyMismatch {
            run: "run-1".to_owned(),
            stage: Stage::Mesh,
        })
    );

    let mut result_edited = full_record("run-1");
    result_edited
        .result
        .as_mut()
        .unwrap()
        .recipe
        .fluid
        .kinematic_viscosity *= 2.0;
    assert_eq!(
        verify_runs(&[result_edited]),
        Err(RunsError::KeyMismatch {
            run: "run-1".to_owned(),
            stage: Stage::Result,
        })
    );
}

#[test]
fn duplicate_ids_are_refused() {
    let runs = [full_record("run-1"), full_record("run-1")];
    assert_eq!(
        verify_runs(&runs),
        Err(RunsError::DuplicateId("run-1".to_owned()))
    );
}

#[test]
fn an_id_that_is_not_a_plain_name_is_refused() {
    let too_long = "a".repeat(41);
    for id in ["../x", "RUN-1", "", "-run", too_long.as_str()] {
        assert_eq!(
            verify_runs(&[bare_record(id)]),
            Err(RunsError::BadId(id.to_owned())),
            "{id:?}"
        );
    }
    assert_eq!(verify_runs(&[bare_record("run-1")]), Ok(()));
}

#[test]
fn one_rule_decides_what_a_plain_name_is() {
    // Run ids, link ids, document names and the names of runs a verb takes all follow it.
    let longest = "a".repeat(40);
    for name in ["run-1", "a", "0", "a-b-c", "9-", longest.as_str()] {
        assert!(is_plain_name(name), "{name:?}");
    }
    let too_long = "a".repeat(41);
    for name in [
        "",
        "-a",
        "A",
        "run_1",
        "run 1",
        "run.1",
        "ä",
        "../x",
        too_long.as_str(),
    ] {
        assert!(!is_plain_name(name), "{name:?}");
    }
}

#[test]
fn run_ids_must_count_up_from_run_1_in_order() {
    let longest = "a".repeat(40);
    // A plain name that is not the one this place in the log has: the next record could never
    // be added to such a log.
    for id in ["run-2", "a", "0", "run-0", "run-01", longest.as_str()] {
        assert_eq!(
            verify_runs(&[bare_record(id)]),
            Err(RunsError::OutOfOrder {
                found: id.to_owned(),
                expected: "run-1".to_owned(),
            }),
            "{id:?}"
        );
    }
    let swapped = [
        bare_record("run-1"),
        bare_record("run-3"),
        bare_record("run-2"),
    ];
    assert_eq!(
        verify_runs(&swapped),
        Err(RunsError::OutOfOrder {
            found: "run-3".to_owned(),
            expected: "run-2".to_owned(),
        })
    );
    let text = RunsError::OutOfOrder {
        found: "run-3".to_owned(),
        expected: "run-2".to_owned(),
    }
    .to_string();
    assert!(text.contains("run-3") && text.contains("run-2"), "{text}");
}

#[test]
fn a_result_must_stand_on_the_mesh_its_provenance_names() {
    let other_mesh_key = {
        let mut study = cavity();
        study.mesh.cells_x = 256;
        recipes_of(&study).keys().mesh
    };
    let mut record = full_record("run-1");
    let result = record.result.as_mut().unwrap();
    result.recipe.mesh_key = other_mesh_key;
    result.key = result.recipe.key();
    record.provenance.result_key = Some(result.key);
    assert_eq!(
        verify_runs(&[record]),
        Err(RunsError::MeshKeyMismatch {
            run: "run-1".to_owned()
        })
    );
}

#[test]
fn a_rerun_must_name_an_earlier_run() {
    let mut names_a_later_run = full_record("run-1");
    names_a_later_run.rerun_of = Some("run-2".to_owned());
    assert_eq!(
        verify_runs(&[names_a_later_run, full_record("run-2")]),
        Err(RunsError::BadRerun {
            run: "run-1".to_owned(),
            of: "run-2".to_owned(),
        })
    );

    assert_eq!(
        verify_runs(&[full_record("run-1"), rerun_record("run-2", "run-9")]),
        Err(RunsError::BadRerun {
            run: "run-2".to_owned(),
            of: "run-9".to_owned(),
        })
    );

    assert_eq!(
        verify_runs(&[rerun_record("run-1", "run-1")]),
        Err(RunsError::BadRerun {
            run: "run-1".to_owned(),
            of: "run-1".to_owned(),
        })
    );
}

#[test]
fn a_run_that_holds_output_must_have_passed_every_check() {
    let failing = Check {
        name: "evidence".to_owned(),
        passed: false,
        detail: "outside the allowed range".to_owned(),
    };
    let mut record = full_record("run-1");
    record.checks.push(failing.clone());
    assert_eq!(
        verify_runs(&[record]),
        Err(RunsError::FailedCheck {
            run: "run-1".to_owned(),
            check: "evidence".to_owned(),
        })
    );

    // A run that stored nothing may say what went wrong.
    let mut bare = bare_record("run-1");
    bare.checks.push(failing);
    assert_eq!(verify_runs(&[bare]), Ok(()));
}

#[test]
fn a_rerun_holds_a_result_only_and_repeats_a_run_that_has_one() {
    let mut with_mesh = rerun_record("run-2", "run-1");
    with_mesh.mesh = full_record("run-2").mesh;
    assert_eq!(
        verify_runs(&[full_record("run-1"), with_mesh]),
        Err(RunsError::RerunHoldsMesh {
            run: "run-2".to_owned()
        })
    );

    let mut mesh_only = full_record("run-1");
    mesh_only.result = None;
    mesh_only.provenance.result_key = None;
    assert_eq!(
        verify_runs(&[mesh_only, rerun_record("run-2", "run-1")]),
        Err(RunsError::RerunOfNoResult {
            run: "run-2".to_owned(),
            of: "run-1".to_owned(),
        })
    );
}

#[test]
fn too_many_runs_are_refused() {
    let base = bare_record("run-1");
    let mut runs: Vec<RunRecord> = (1..=MAX_RUNS)
        .map(|n| {
            let mut record = base.clone();
            record.id = format!("run-{n}");
            record
        })
        .collect();
    assert_eq!(verify_runs(&runs), Ok(()));
    let mut extra = base;
    extra.id = format!("run-{}", MAX_RUNS + 1);
    runs.push(extra);
    assert_eq!(verify_runs(&runs), Err(RunsError::TooMany(MAX_RUNS + 1)));
}

#[test]
fn redacting_hosts_clears_only_the_host() {
    let original = [
        full_record("run-1"),
        result_only("run-2", &faster_lid()),
        rerun_record("run-3", "run-1"),
    ];
    assert!(
        original
            .iter()
            .all(|r| r.provenance.host.as_deref() == Some("box"))
    );
    let mut redacted = original.clone();
    redact_hosts(&mut redacted);
    assert!(redacted.iter().all(|r| r.provenance.host.is_none()));

    let mut expected = original;
    for record in &mut expected {
        record.provenance.host = None;
    }
    assert_eq!(redacted, expected);
    assert_eq!(verify_runs(&redacted), Ok(()));
}

#[test]
fn verification_names_the_first_problem_in_log_order() {
    let mut key_edited = full_record("run-2");
    key_edited.mesh.as_mut().unwrap().recipe.geometry.length_y += 1.0;
    let runs = [full_record("run-1"), key_edited, full_record("run-1")];
    assert_eq!(
        verify_runs(&runs),
        Err(RunsError::KeyMismatch {
            run: "run-2".to_owned(),
            stage: Stage::Mesh,
        })
    );
}

#[test]
fn a_provenance_result_key_must_match_the_result() {
    let other_result_key = recipes_of(&faster_lid()).keys().result;
    let mut wrong_key = full_record("run-1");
    wrong_key.provenance.result_key = Some(other_result_key);
    assert_eq!(
        verify_runs(&[wrong_key]),
        Err(RunsError::ResultKeyMismatch {
            run: "run-1".to_owned()
        })
    );

    let mut no_result = bare_record("run-1");
    no_result.provenance.result_key = Some(other_result_key);
    assert_eq!(
        verify_runs(&[no_result]),
        Err(RunsError::ResultKeyMismatch {
            run: "run-1".to_owned()
        })
    );
}

#[test]
fn an_error_names_the_run_in_plain_words() {
    let error = RunsError::KeyMismatch {
        run: "run-4".to_owned(),
        stage: Stage::Result,
    };
    let text = error.to_string();
    assert!(text.contains("run-4") && text.contains("result"), "{text}");
    assert!(
        RunsError::TooMany(10_001)
            .to_string()
            .contains(&MAX_RUNS.to_string())
    );
}
