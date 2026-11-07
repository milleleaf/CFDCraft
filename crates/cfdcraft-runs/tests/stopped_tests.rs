use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{
    BlobUse, Checkpoint, KeyNumbers, Outcome, Output, Provenance, RunRecord, Stage, StageRecipes,
    StageState, ToolIdentity, latest_result, status,
};
use cfdcraft_settle::{Balance, BalanceStatus, Settings, verdict};
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

fn recipes() -> StageRecipes {
    StageRecipes::from_resolved(&resolve(&cavity()).unwrap(), &tool())
}

fn checkpoint() -> Checkpoint {
    Checkpoint {
        time: "1200".to_owned(),
        blob: ContentHash::of(b"checkpoint"),
        size: 8,
    }
}

/// A run that made the mesh and nothing else.
fn base_record(id: &str) -> RunRecord {
    let recipes = recipes();
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
        result: None,
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash,
            result_key: None,
            settings: cavity(),
            tool: tool(),
            host: None,
        },
        checks: Vec::new(),
        key_numbers: KeyNumbers::default(),
        comparison: None,
        continued_from: None,
        outcome: Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    }
}

fn stopped_record(id: &str) -> RunRecord {
    let mut record = base_record(id);
    record.outcome = Outcome::Stopped {
        at_time: "1200".to_owned(),
    };
    record.checkpoint = Some(checkpoint());
    record
}

#[test]
fn a_stopped_record_never_makes_the_result_fresh() {
    let made = recipes();
    let runs = [stopped_record("run-1")];
    let found = status(&runs, &made);
    assert_eq!(
        found.mesh,
        StageState::Fresh {
            run: "run-1".to_owned()
        }
    );
    assert_eq!(found.result, StageState::NeverMade);
    assert!(latest_result(&runs, made.keys().result).is_none());
}

#[test]
fn blob_uses_counts_the_checkpoint_as_a_result_blob() {
    assert_eq!(
        stopped_record("run-1").blob_uses(),
        vec![
            BlobUse {
                hash: ContentHash::of(b"mesh"),
                size: 4,
                stage: Stage::Mesh,
            },
            BlobUse {
                hash: ContentHash::of(b"checkpoint"),
                size: 8,
                stage: Stage::Result,
            },
        ]
    );
}

#[test]
fn a_complete_record_serialises_exactly_as_before() {
    let json = serde_json::to_value(base_record("run-1")).unwrap();
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["checks", "id", "key_numbers", "mesh", "provenance"]);
    for added in ["continued_from", "outcome", "checkpoint", "verdict", "np"] {
        assert!(json.get(added).is_none(), "{added} must not be written");
    }
}

#[test]
fn a_record_written_before_this_change_still_reads() {
    let record = base_record("run-1");
    let mut json = serde_json::to_value(&record).unwrap();
    for added in ["continued_from", "outcome", "checkpoint", "verdict", "np"] {
        json.as_object_mut().unwrap().remove(added);
    }
    let back: RunRecord = serde_json::from_value(json).unwrap();
    assert_eq!(back, record);
    assert_eq!(back.outcome, Outcome::Complete);
}

#[test]
fn a_stopped_outcome_has_a_stable_json_shape() {
    let record = stopped_record("run-1");
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["outcome"], json!({"stopped": {"at_time": "1200"}}));
    assert_eq!(
        json["checkpoint"],
        json!({
            "time": "1200",
            "blob": serde_json::to_value(ContentHash::of(b"checkpoint")).unwrap(),
            "size": 8,
        })
    );
    let back: RunRecord = serde_json::from_value(json).unwrap();
    assert_eq!(back, record);
}

#[test]
fn the_verdict_and_np_round_trip() {
    let balance = Balance {
        status: BalanceStatus::NotApplicable,
        imbalance: None,
        reference: None,
    };
    let mut record = base_record("run-1");
    record.verdict = Some(verdict(
        Vec::new(),
        balance.clone(),
        balance,
        true,
        &Settings::default(),
    ));
    record.np = Some(2);
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["np"], 2);
    assert!(json.get("verdict").is_some());
    let back: RunRecord = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back, record);

    let mut extra = json;
    extra
        .as_object_mut()
        .unwrap()
        .insert("surprise".to_owned(), json!(1));
    assert!(serde_json::from_value::<RunRecord>(extra).is_err());
}
