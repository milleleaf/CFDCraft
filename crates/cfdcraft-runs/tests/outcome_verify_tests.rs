use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{
    Checkpoint, ContinuedFrom, Outcome, Output, Provenance, RunRecord, RunsError, StageRecipes,
    ToolIdentity, verify_runs,
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

/// A run that recorded both stages of the cavity and went to its end.
fn full_record(id: &str) -> RunRecord {
    let recipes = StageRecipes::from_resolved(&resolve(&cavity()).unwrap(), &tool());
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

fn checkpoint_at(time: &str) -> Checkpoint {
    Checkpoint {
        time: time.to_owned(),
        blob: ContentHash::of(b"checkpoint"),
        size: 8,
    }
}

/// A run stopped at `time`: the mesh, the checkpoint and no result.
fn stopped_record(id: &str, time: &str) -> RunRecord {
    let mut record = full_record(id);
    record.result = None;
    record.provenance.result_key = None;
    record.outcome = Outcome::Stopped {
        at_time: time.to_owned(),
    };
    record.checkpoint = Some(checkpoint_at(time));
    record
}

fn continued(id: &str, from: &str, time: &str) -> RunRecord {
    let mut record = full_record(id);
    record.continued_from = Some(ContinuedFrom {
        run: from.to_owned(),
        time: time.to_owned(),
    });
    record
}

/// A rerun of `run-1`, which has to come before it in the log.
fn rerun_record(id: &str) -> RunRecord {
    let mut record = full_record(id);
    record.mesh = None;
    record.rerun_of = Some("run-1".to_owned());
    record
}

fn refused_run(runs: &[RunRecord]) -> String {
    match verify_runs(runs) {
        Err(RunsError::BadOutcome { run, reason }) => {
            assert!(!reason.is_empty());
            run
        }
        other => panic!("expected BadOutcome, got {other:?}"),
    }
}

#[test]
fn a_stopped_record_without_a_checkpoint_is_refused() {
    let mut record = stopped_record("run-1", "1200");
    record.checkpoint = None;
    assert_eq!(refused_run(&[record]), "run-1");
}

#[test]
fn a_stopped_record_with_a_result_is_refused() {
    let mut record = stopped_record("run-1", "1200");
    record.result = full_record("run-1").result;
    record.provenance.result_key = full_record("run-1").provenance.result_key;
    assert_eq!(refused_run(&[record]), "run-1");
}

#[test]
fn a_complete_record_with_a_checkpoint_is_refused() {
    let mut record = full_record("run-1");
    record.checkpoint = Some(checkpoint_at("1200"));
    assert_eq!(refused_run(&[record]), "run-1");
}

#[test]
fn a_stopped_checkpoint_time_must_equal_the_stopped_time() {
    let mut record = stopped_record("run-1", "1200");
    record.checkpoint = Some(checkpoint_at("1300"));
    assert_eq!(refused_run(&[record]), "run-1");
}

#[test]
fn continued_from_must_name_an_earlier_record() {
    // A later run, one that is not in the log, and the record itself.
    assert_eq!(
        refused_run(&[continued("run-1", "run-2", "1200"), full_record("run-2")]),
        "run-1"
    );
    assert_eq!(
        refused_run(&[full_record("run-1"), continued("run-2", "run-9", "1200")]),
        "run-2"
    );
    assert_eq!(
        refused_run(&[full_record("run-1"), continued("run-2", "run-2", "1200")]),
        "run-2"
    );
    // The time is plain decimal text.
    for time in ["", ".", "1/2", "../x", "1.2.3", "1e5", "-3", " 1"] {
        let runs = [
            stopped_record("run-1", "1200"),
            continued("run-2", "run-1", time),
        ];
        assert_eq!(refused_run(&runs), "run-2", "time {time:?}");
    }
    for time in ["1200", "0.5", "12.", ".5"] {
        let runs = [
            stopped_record("run-1", "1200"),
            continued("run-2", "run-1", time),
        ];
        assert_eq!(verify_runs(&runs), Ok(()), "time {time:?}");
    }
}

#[test]
fn np_zero_is_refused_and_np_one_or_more_is_fine() {
    let mut record = full_record("run-1");
    record.np = Some(0);
    assert_eq!(refused_run(&[record.clone()]), "run-1");
    for np in [1, 2, 64] {
        record.np = Some(np);
        assert_eq!(verify_runs(&[record.clone()]), Ok(()));
    }
    record.np = None;
    assert_eq!(verify_runs(&[record]), Ok(()));
}

#[test]
fn a_stopped_record_may_hold_a_mesh() {
    let record = stopped_record("run-1", "1200");
    assert!(record.mesh.is_some());
    assert_eq!(verify_runs(&[record]), Ok(()));
}

#[test]
fn a_rerun_cannot_be_stopped_or_continued() {
    let mut stopped = rerun_record("run-2");
    stopped.result = None;
    stopped.provenance.result_key = None;
    stopped.outcome = Outcome::Stopped {
        at_time: "1200".to_owned(),
    };
    stopped.checkpoint = Some(checkpoint_at("1200"));
    assert_eq!(refused_run(&[full_record("run-1"), stopped]), "run-2");

    let mut with_checkpoint = rerun_record("run-2");
    with_checkpoint.checkpoint = Some(checkpoint_at("1200"));
    assert_eq!(
        refused_run(&[full_record("run-1"), with_checkpoint]),
        "run-2"
    );

    let mut continues = rerun_record("run-2");
    continues.continued_from = Some(ContinuedFrom {
        run: "run-1".to_owned(),
        time: "1200".to_owned(),
    });
    assert_eq!(refused_run(&[full_record("run-1"), continues]), "run-2");

    assert_eq!(
        verify_runs(&[full_record("run-1"), rerun_record("run-2")]),
        Ok(())
    );
}

#[test]
fn a_good_log_with_a_stopped_run_and_its_continuation_passes() {
    let runs = [
        stopped_record("run-1", "1200"),
        continued("run-2", "run-1", "1200"),
    ];
    assert_eq!(verify_runs(&runs), Ok(()));
}
