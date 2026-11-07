//! A run that was stopped, the run that continued it and the verdict on that run survive a save and
//! an open unchanged, and the file says so in plain keys.

use std::fs;
use std::path::PathBuf;

use cfdcraft_doc::{Document, open, save};
use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{
    Check, Checkpoint, ContinuedFrom, KeyNumbers, Outcome, Output, Provenance, RunRecord,
    StageRecipes, ToolIdentity, verify_runs,
};
use cfdcraft_store::ContentHash;
use serde_json::json;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn cavity() -> Study {
    Study::from_json(CAVITY).unwrap()
}

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("doc-stopped-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn mesh_hash() -> ContentHash {
    ContentHash::of(b"mesh")
}

/// A run of the cavity that recorded the mesh and, when `with_result` is true, the result.
fn record(id: &str, with_result: bool) -> RunRecord {
    let study = cavity();
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool());
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
        result: with_result.then(|| Output {
            key: keys.result,
            recipe: recipes.result.clone(),
            blob: ContentHash::of(id.as_bytes()),
            size: 6,
        }),
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash: mesh_hash(),
            result_key: with_result.then_some(keys.result),
            settings: study,
            tool: tool(),
            host: None,
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

/// A stopped run: the mesh, a checkpoint at time 12, and no result.
fn stopped() -> RunRecord {
    let mut run = record("run-1", false);
    run.outcome = Outcome::Stopped {
        at_time: "12".to_owned(),
    };
    run.checkpoint = Some(Checkpoint {
        time: "12".to_owned(),
        blob: ContentHash::of(b"checkpoint"),
        size: 10,
    });
    run.np = Some(1);
    run
}

/// A complete run that continued the stopped one, with a converged verdict made from one settled
/// quantity.
fn continued() -> RunRecord {
    let mut run = record("run-2", true);
    run.continued_from = Some(ContinuedFrom {
        run: "run-1".to_owned(),
        time: "12".to_owned(),
    });
    run.verdict = Some(
        serde_json::from_value(json!({
            "state": "converged",
            "reasons": [],
            "quantities": [{
                "name": "probe_1.Ux",
                "last": 0.5,
                "mean": 0.5,
                "spread": 0.25,
                "drift": 0.0,
                "status": "settled",
                "samples": 50
            }],
            "mass": { "status": "not_applicable" },
            "energy": { "status": "not_applicable" },
            "residuals_ok": true
        }))
        .unwrap(),
    );
    run.np = Some(2);
    run
}

#[test]
fn a_stopped_run_with_a_checkpoint_and_a_verdict_survives_save_and_open() {
    let mut doc = Document::new(cavity());
    doc.add_run(stopped()).unwrap();
    doc.add_run(continued()).unwrap();

    let path = fresh("round-trip").join("study.cfdcraft");
    save(&doc, &path).unwrap();
    let opened = open(&path).unwrap();

    assert_eq!(opened.document, doc);
    assert!(opened.notes.is_empty());
    verify_runs(&opened.document.artifacts.runs).unwrap();

    let runs = &opened.document.artifacts.runs;
    assert!(matches!(runs[0].outcome, Outcome::Stopped { ref at_time } if at_time == "12"));
    assert!(runs[0].result.is_none());
    assert_eq!(runs[0].checkpoint.as_ref().map(|c| c.size), Some(10));
    assert_eq!(runs[1].continued_from.as_ref().unwrap().run, "run-1");
    assert!(runs[1].verdict.is_some());
    assert_eq!(runs[1].np, Some(2));

    // The checkpoint is a stored result, so the index lists it beside the other blobs.
    assert!(
        opened
            .document
            .artifacts
            .blobs
            .iter()
            .any(|b| b.hash == ContentHash::of(b"checkpoint"))
    );
}

#[test]
fn a_complete_run_writes_none_of_the_new_members() {
    let mut doc = Document::new(cavity());
    doc.add_run(record("run-1", true)).unwrap();
    let path = fresh("plain").join("study.cfdcraft");
    save(&doc, &path).unwrap();
    let opened = open(&path).unwrap();
    assert_eq!(opened.document, doc);
    assert_eq!(opened.document.artifacts.runs[0].outcome, Outcome::Complete);
    assert!(opened.document.artifacts.runs[0].checkpoint.is_none());
}
