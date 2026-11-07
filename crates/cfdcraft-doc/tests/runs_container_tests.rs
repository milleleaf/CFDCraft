//! The run history in a file: it saves and opens equal, older files are brought forward, and a
//! history that disagrees with the list of stored results is refused on read and on write. Every
//! archive here is built straight with the ZIP crate.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use cfdcraft_doc::{DocError, Document, from_bytes, save, to_bytes};
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{Check, KeyNumbers, Output, Provenance, RunRecord, StageRecipes, ToolIdentity};
use cfdcraft_store::ContentHash;
use serde_json::{Value, json};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const DEFLATE: CompressionMethod = CompressionMethod::Deflated;

fn cavity() -> Study {
    Study::from_json(CAVITY).unwrap()
}

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("doc-runs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn archive(entries: Vec<(&str, Vec<u8>, CompressionMethod)>) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, method) in entries {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn manifest(version: u32) -> Vec<u8> {
    json!({
        "format": "cfdcraft-study",
        "schema_version": version,
        "writer": { "name": "other-tool", "version": "9" }
    })
    .to_string()
    .into_bytes()
}

fn member(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn mesh_hash() -> ContentHash {
    ContentHash::of(b"mesh")
}

/// A run of the cavity that recorded both stages; its result blob is named after the run.
fn record(id: &str) -> RunRecord {
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
            settings: study,
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
        outcome: cfdcraft_runs::Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    }
}

fn document_with_runs() -> Document {
    let mut doc = Document::new(cavity());
    doc.add_run(record("run-1")).unwrap();
    doc.add_run(record("run-2")).unwrap();
    doc
}

/// A file at `version` whose artifact index is `artifacts`.
fn file_with_artifacts(version: u32, artifacts: &Value) -> Vec<u8> {
    archive(vec![
        ("manifest.json", manifest(version), DEFLATE),
        ("study.json", CAVITY.as_bytes().to_vec(), DEFLATE),
        (
            "artifacts.json",
            artifacts.to_string().into_bytes(),
            DEFLATE,
        ),
    ])
}

fn expect_bad_artifacts(error: DocError) -> String {
    match error {
        DocError::BadJson { member, reason } => {
            assert_eq!(member, "artifacts.json");
            reason
        }
        // A history that does not hold together has an error of its own.
        DocError::RunLog(reason) => reason,
        other => panic!("{other}"),
    }
}

#[test]
fn a_document_with_runs_saves_and_opens_equal() {
    let doc = document_with_runs();
    let bytes = to_bytes(&doc).unwrap();

    let opened = from_bytes(&bytes).unwrap();
    assert_eq!(opened.document, doc);
    assert!(opened.notes.is_empty());

    let manifest: Value = serde_json::from_slice(&member(&bytes, "manifest.json")).unwrap();
    assert_eq!(manifest["schema_version"], CURRENT_SCHEMA);
    let artifacts = String::from_utf8(member(&bytes, "artifacts.json")).unwrap();
    assert!(artifacts.contains("\"runs\""), "{artifacts}");
}

#[test]
fn a_schema_1_file_opens_with_four_notes_and_no_runs() {
    let bytes = file_with_artifacts(1, &json!({"blobs": [], "links": []}));
    let opened = from_bytes(&bytes).unwrap();

    assert_eq!(opened.schema_version, 1);
    assert_eq!(opened.notes.len(), 4);
    assert_eq!((opened.notes[0].from, opened.notes[0].to), (1, 2));
    assert_eq!((opened.notes[1].from, opened.notes[1].to), (2, 3));
    assert_eq!((opened.notes[2].from, opened.notes[2].to), (3, 4));
    assert_eq!((opened.notes[3].from, opened.notes[3].to), (4, 5));
    assert!(opened.document.artifacts.runs.is_empty());
    assert_eq!(opened.document.study, cavity());
}

#[test]
fn a_schema_1_file_saved_back_becomes_the_current_schema() {
    let bytes = file_with_artifacts(1, &json!({"blobs": [], "links": []}));
    let opened = from_bytes(&bytes).unwrap();

    let again = from_bytes(&to_bytes(&opened.document).unwrap()).unwrap();
    assert_eq!(again.schema_version, CURRENT_SCHEMA);
    assert!(again.notes.is_empty());
    assert_eq!(again.document, opened.document);
}

#[test]
fn a_schema_0_file_opens_with_five_notes() {
    let bytes = archive(vec![
        ("manifest.json", manifest(0), DEFLATE),
        ("study.json", CAVITY_V0.as_bytes().to_vec(), DEFLATE),
    ]);
    let opened = from_bytes(&bytes).unwrap();

    assert_eq!(opened.schema_version, 0);
    let pairs: Vec<(u32, u32)> = opened.notes.iter().map(|n| (n.from, n.to)).collect();
    assert_eq!(pairs, vec![(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]);
    assert_eq!(opened.document.study, cavity());
}

#[test]
fn an_index_that_names_a_blob_it_does_not_list_is_refused_on_read() {
    let artifacts = json!({
        "blobs": [],
        "links": [],
        "runs": [serde_json::to_value(record("run-1")).unwrap()]
    });
    let error = from_bytes(&file_with_artifacts(CURRENT_SCHEMA, &artifacts)).unwrap_err();
    // A history that does not hold together is told apart by its type, not by its wording.
    assert!(matches!(error, DocError::RunLog(_)), "{error}");
    let reason = expect_bad_artifacts(error);
    assert!(reason.contains(&mesh_hash().to_string()), "{reason}");
}

#[test]
fn an_inconsistent_document_is_refused_on_write() {
    // A run put into the history by hand, with none of the results it names listed.
    let mut bad = Document::new(cavity());
    bad.artifacts.runs.push(record("run-1"));

    let reason = expect_bad_artifacts(to_bytes(&bad).unwrap_err());
    assert!(reason.contains("not listed"), "{reason}");

    // Saving to a path leaves the file that was there as it was.
    let path = fresh("write").join("cavity.cfdcraft");
    save(&document_with_runs(), &path).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(save(&bad, &path).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn a_run_record_with_an_unknown_key_is_refused() {
    let mut artifacts = serde_json::to_value(&document_with_runs().artifacts).unwrap();
    artifacts["runs"][0]["surprise"] = json!(1);
    let error = from_bytes(&file_with_artifacts(CURRENT_SCHEMA, &artifacts)).unwrap_err();
    // A record that is not the right shape is bad JSON, not an inconsistent history.
    assert!(matches!(error, DocError::BadJson { .. }), "{error}");
    expect_bad_artifacts(error);
}
