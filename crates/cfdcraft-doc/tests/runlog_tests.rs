use cfdcraft_doc::{ArtifactIndex, BlobKind, BlobRef, Document, RunLogError, check_runs};
use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{
    Check, KeyNumbers, Output, Provenance, RunRecord, RunsError, Stage, StageRecipes, ToolIdentity,
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

fn mesh_hash() -> ContentHash {
    ContentHash::of(b"mesh")
}

/// A run of `study` that recorded both stages; its result blob is named after the run.
fn record_for(id: &str, study: &Study) -> RunRecord {
    let recipes = StageRecipes::from_resolved(&resolve(study).unwrap(), &tool());
    let keys = recipes.keys();
    RunRecord {
        id: id.to_owned(),
        rerun_of: None,
        scenario: None,
        mesh: Some(Output {
            key: keys.mesh,
            recipe: recipes.mesh.clone(),
            blob: mesh_hash(),
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
            mesh_hash: mesh_hash(),
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

/// A run that recorded only a result, on the mesh an earlier run made.
fn result_only(id: &str, study: &Study) -> RunRecord {
    let mut record = record_for(id, study);
    record.mesh = None;
    record
}

fn document() -> Document {
    Document::new(cavity())
}

#[test]
fn a_run_is_added_with_the_blobs_it_names() {
    let mut doc = document();
    doc.add_run(record_for("run-1", &cavity())).unwrap();

    assert_eq!(doc.artifacts.runs.len(), 1);
    assert_eq!(
        doc.artifacts.blobs,
        vec![
            BlobRef {
                hash: mesh_hash(),
                size: 4,
                kind: BlobKind::Mesh,
            },
            BlobRef {
                hash: ContentHash::of(b"run-1"),
                size: 6,
                kind: BlobKind::Result,
            },
        ]
    );
}

#[test]
fn a_second_run_adds_only_new_blobs() {
    let mut doc = document();
    doc.add_run(record_for("run-1", &cavity())).unwrap();
    doc.add_run(result_only("run-2", &faster_lid())).unwrap();
    assert_eq!(doc.artifacts.blobs.len(), 3);

    // A third run that names a result the file already lists, with the same size and kind.
    let mut third = result_only("run-3", &cavity());
    third.result.as_mut().unwrap().blob = ContentHash::of(b"run-1");
    doc.add_run(third).unwrap();
    assert_eq!(doc.artifacts.runs.len(), 3);
    assert_eq!(doc.artifacts.blobs.len(), 3);
}

#[test]
fn a_wrong_id_is_refused_and_nothing_changes() {
    let mut doc = document();
    let before = doc.clone();
    let error = doc.add_run(record_for("run-9", &cavity())).unwrap_err();
    match error {
        RunLogError::WrongId { expected, found } => {
            assert_eq!(expected, "run-1");
            assert_eq!(found, "run-9");
        }
        other => panic!("{other}"),
    }
    assert_eq!(doc, before);
}

#[test]
fn a_record_that_fails_verification_is_refused_and_nothing_changes() {
    let mut doc = document();
    let before = doc.clone();
    let mut record = record_for("run-1", &cavity());
    record.mesh.as_mut().unwrap().recipe.geometry.length_x += 1.0;
    let error = doc.add_run(record).unwrap_err();
    assert_eq!(
        error,
        RunLogError::Runs(RunsError::KeyMismatch {
            run: "run-1".to_owned(),
            stage: Stage::Mesh,
        })
    );
    assert_eq!(doc, before);
}

#[test]
fn a_blob_listed_with_another_size_is_a_conflict() {
    let mut doc = document();
    doc.artifacts.blobs.push(BlobRef {
        hash: mesh_hash(),
        size: 10,
        kind: BlobKind::Mesh,
    });
    let before = doc.clone();
    let error = doc.add_run(record_for("run-1", &cavity())).unwrap_err();
    assert_eq!(error, RunLogError::BlobConflict(mesh_hash()));
    assert_eq!(doc, before);
}

#[test]
fn redacting_hosts_clears_every_host() {
    let mut doc = document();
    doc.add_run(record_for("run-1", &cavity())).unwrap();
    doc.add_run(result_only("run-2", &faster_lid())).unwrap();
    let mut expected = doc.artifacts.clone();
    for run in &mut expected.runs {
        run.provenance.host = None;
    }

    doc.artifacts.redact_hosts();

    assert!(
        doc.artifacts
            .runs
            .iter()
            .all(|r| r.provenance.host.is_none())
    );
    assert_eq!(doc.artifacts, expected);
}

#[test]
fn check_runs_names_a_blob_that_is_not_listed() {
    let index = ArtifactIndex {
        runs: vec![record_for("run-1", &cavity())],
        ..ArtifactIndex::default()
    };
    let reason = check_runs(&index).unwrap_err();
    assert!(reason.contains(&mesh_hash().to_string()), "{reason}");

    let mut doc = document();
    doc.add_run(record_for("run-1", &cavity())).unwrap();
    assert_eq!(check_runs(&doc.artifacts), Ok(()));

    // A listed blob with another size is refused as well.
    doc.artifacts.blobs[0].size = 5;
    assert!(check_runs(&doc.artifacts).is_err());
}

#[test]
fn an_index_without_a_runs_entry_reads_as_having_none() {
    let index: ArtifactIndex = serde_json::from_str("{\"blobs\":[],\"links\":[]}").unwrap();
    assert!(index.runs.is_empty());
    assert_eq!(index, ArtifactIndex::default());

    let mut doc = document();
    doc.add_run(record_for("run-1", &cavity())).unwrap();
    let text = serde_json::to_string(&doc.artifacts).unwrap();
    assert!(text.contains("\"runs\":[{"), "{text}");
}

#[test]
fn room_for_another_run_is_judged_from_the_size_of_the_index() {
    let mut doc = document();
    assert_eq!(doc.check_room_for_run(), Ok(()));
    doc.add_run(record_for("run-1", &cavity())).unwrap();
    assert_eq!(doc.check_room_for_run(), Ok(()));

    // 31 records that each carry a megabyte of host label: the index is within its 32 MiB limit,
    // but not by the size of one more record like them.
    let label = "x".repeat(1 << 20);
    doc.artifacts.runs[0].provenance.host = Some(label.clone());
    for n in 2..=31 {
        let mut copy = doc.artifacts.runs[0].clone();
        copy.id = format!("run-{n}");
        doc.artifacts.runs.push(copy);
    }
    match doc.check_room_for_run() {
        Err(error @ RunLogError::HistoryFull { runs: 31, .. }) => {
            let text = error.to_string();
            assert!(text.contains("31") && text.contains("no room"), "{text}");
        }
        other => panic!("expected a full history, found {other:?}"),
    }

    doc.artifacts.runs.truncate(8);
    assert_eq!(doc.check_room_for_run(), Ok(()));
}
