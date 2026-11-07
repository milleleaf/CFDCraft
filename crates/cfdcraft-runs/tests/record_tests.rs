use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{
    BlobUse, Check, KeyNumbers, Outcome, Output, Provenance, RunRecord, Stage, StageRecipes,
    ToolIdentity, find_run, latest_mesh, latest_result, newest_mesh_output, newest_result_output,
    next_run_id,
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

fn recipes() -> StageRecipes {
    StageRecipes::from_resolved(&resolve(&cavity()).unwrap(), &tool())
}

/// A run that recorded both stages of the cavity.
fn full_record(id: &str) -> RunRecord {
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
        result: Some(Output {
            key: keys.result,
            recipe: recipes.result.clone(),
            blob: ContentHash::of(b"result"),
            size: 6,
        }),
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash,
            result_key: Some(keys.result),
            settings: cavity(),
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
        continued_from: None,
        outcome: Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    }
}

/// A run that repeated `of` and recorded only a new result blob under the same key.
fn rerun_record(id: &str, of: &str) -> RunRecord {
    let mut record = full_record(id);
    record.rerun_of = Some(of.to_owned());
    record.mesh = None;
    if let Some(result) = record.result.as_mut() {
        result.blob = ContentHash::of(b"result again");
    }
    record
}

#[test]
fn a_run_record_round_trips_through_json() {
    let record = full_record("run-1");
    let json = serde_json::to_value(&record).unwrap();
    for key in [
        "id",
        "mesh",
        "result",
        "provenance",
        "checks",
        "key_numbers",
    ] {
        assert!(json.get(key).is_some(), "missing {key}");
    }
    assert!(json.get("rerun_of").is_none());
    assert!(json.get("comparison").is_none());
    let back: RunRecord = serde_json::from_value(json).unwrap();
    assert_eq!(back, record);
    let text = serde_json::to_string(&record).unwrap();
    assert_eq!(serde_json::from_str::<RunRecord>(&text).unwrap(), record);
}

#[test]
fn a_record_that_recorded_one_stage_leaves_the_other_out() {
    let mut record = full_record("run-1");
    record.result = None;
    record.provenance.result_key = None;
    let json = serde_json::to_value(&record).unwrap();
    assert!(json.get("result").is_none());
    assert!(json.get("mesh").is_some());
    let back: RunRecord = serde_json::from_value(json).unwrap();
    assert_eq!(back, record);
}

#[test]
fn run_ids_count_up_from_the_log_length() {
    assert_eq!(next_run_id(&[]), "run-1");
    let runs = [full_record("run-1"), full_record("run-2")];
    assert_eq!(next_run_id(&runs), "run-3");
}

#[test]
fn lookups_find_the_newest_output_with_a_key() {
    let first = full_record("run-1");
    let mut second = full_record("run-2");
    let second_mesh = ContentHash::of(b"mesh again");
    second.mesh.as_mut().unwrap().blob = second_mesh;
    let key = first.mesh.as_ref().unwrap().key;
    let runs = [first, second];

    let (owner, output) = latest_mesh(&runs, key).unwrap();
    assert_eq!(owner.id, "run-2");
    assert_eq!(output.blob, second_mesh);
    assert_eq!(find_run(&runs, "run-1").unwrap().id, "run-1");
    assert!(find_run(&runs, "run-9").is_none());
    assert_eq!(newest_mesh_output(&runs).unwrap().blob, second_mesh);
    assert!(newest_mesh_output(&[]).is_none());
}

#[test]
fn a_rerun_is_never_found_as_the_owner_of_a_result_key() {
    let original = full_record("run-1");
    let key = original.result.as_ref().unwrap().key;
    let original_blob = original.result.as_ref().unwrap().blob;
    let rerun = rerun_record("run-2", "run-1");
    let runs = [original, rerun];

    let (owner, output) = latest_result(&runs, key).unwrap();
    assert_eq!(owner.id, "run-1");
    assert_eq!(output.blob, original_blob);
    assert_eq!(newest_result_output(&runs).unwrap().blob, original_blob);
    assert!(newest_result_output(&runs[1..]).is_none());
    assert!(latest_result(&runs[1..], key).is_none());
}

#[test]
fn blob_uses_lists_every_blob_a_record_names() {
    let record = full_record("run-1");
    assert_eq!(
        record.blob_uses(),
        vec![
            BlobUse {
                hash: ContentHash::of(b"mesh"),
                size: 4,
                stage: Stage::Mesh,
            },
            BlobUse {
                hash: ContentHash::of(b"result"),
                size: 6,
                stage: Stage::Result,
            },
        ]
    );
    let rerun = rerun_record("run-2", "run-1");
    assert_eq!(
        rerun.blob_uses(),
        vec![BlobUse {
            hash: ContentHash::of(b"result again"),
            size: 6,
            stage: Stage::Result,
        }]
    );
}

#[test]
fn the_provenance_holds_what_the_roadmap_lists() {
    let json = serde_json::to_value(full_record("run-1")).unwrap();
    let provenance = json.get("provenance").unwrap();
    for key in [
        "geometry_hash",
        "mesh_key",
        "mesh_hash",
        "result_key",
        "settings",
        "tool",
        "host",
    ] {
        assert!(provenance.get(key).is_some(), "missing {key}");
    }
    assert!(provenance["settings"].get("schema_version").is_some());
    let tool = &provenance["tool"];
    for key in ["image_digest", "dialect", "sidecar_lock", "writer_version"] {
        assert!(tool.get(key).is_some(), "missing {key}");
    }
    assert!(tool["sidecar_lock"].is_null());
    assert_eq!(provenance["host"], "box");
}

#[test]
fn a_rerun_is_never_found_as_the_owner_of_a_mesh_key() {
    let original = full_record("run-1");
    let key = original.mesh.as_ref().unwrap().key;
    // A rerun that carries a mesh with the same key, as a file from elsewhere might.
    let mut rerun = full_record("run-2");
    rerun.rerun_of = Some("run-1".to_owned());
    let runs = [original, rerun];

    assert_eq!(latest_mesh(&runs, key).unwrap().0.id, "run-1");
    assert_eq!(
        newest_mesh_output(&runs).unwrap().blob,
        runs[0].mesh.as_ref().unwrap().blob
    );
}
