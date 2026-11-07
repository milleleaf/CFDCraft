//! Older files are brought forward and say so; newer files are refused from the manifest alone.
//! Every archive here is built straight with the ZIP crate, so the tests also show that files
//! written by another tool open.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use cfdcraft_doc::{ArtifactIndex, DocError, Document, from_bytes, open, save, to_bytes};
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::{Study, canonical_hash, resolve};
use serde_json::{Value, json};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const PINNED_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";
const DEFLATE: CompressionMethod = CompressionMethod::Deflated;

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

fn manifest(version: Value) -> Vec<u8> {
    json!({
        "format": "cfdcraft-study",
        "schema_version": version,
        "writer": { "name": "other-tool", "version": "9" }
    })
    .to_string()
    .into_bytes()
}

fn manifest_with_extra_field(version: Value) -> Vec<u8> {
    json!({
        "format": "cfdcraft-study",
        "schema_version": version,
        "writer": { "name": "other-tool", "version": "9" },
        "comment": "a field this version does not know"
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

fn old_file() -> Vec<u8> {
    archive(vec![
        ("manifest.json", manifest(json!(0)), DEFLATE),
        ("study.json", CAVITY_V0.as_bytes().to_vec(), DEFLATE),
    ])
}

#[test]
fn an_older_file_opens_migrated_and_says_what_changed() {
    let opened = from_bytes(&old_file()).unwrap();

    assert_eq!(opened.document.study, Study::from_json(CAVITY).unwrap());
    assert_eq!(
        canonical_hash(&resolve(&opened.document.study).unwrap()),
        PINNED_HASH
    );
    assert_eq!(opened.document.artifacts, ArtifactIndex::default());
    assert_eq!(opened.schema_version, 0);
    assert_eq!(opened.notes.len(), 4);
    let first = &opened.notes[0];
    assert_eq!((first.from, first.to), (0, 1));
    assert!(first.summary.contains("lid") && first.summary.contains("walls"));
    let second = &opened.notes[1];
    assert_eq!((second.from, second.to), (1, 2));
    assert!(second.summary.contains("runs"));
    let third = &opened.notes[2];
    assert_eq!((third.from, third.to), (2, 3));
    assert!(third.summary.contains("variants"));
    let fourth = &opened.notes[3];
    assert_eq!((fourth.from, fourth.to), (3, 4));
    assert!(fourth.summary.contains("stopped and continued"));
}

#[test]
fn a_schema_3_file_opens_with_one_plain_note_and_nothing_else_changes() {
    let bytes = archive(vec![
        ("manifest.json", manifest(json!(3)), DEFLATE),
        ("study.json", CAVITY.as_bytes().to_vec(), DEFLATE),
    ]);
    let opened = from_bytes(&bytes).unwrap();

    assert_eq!(opened.schema_version, 3);
    assert_eq!(opened.notes.len(), 1);
    assert_eq!((opened.notes[0].from, opened.notes[0].to), (3, 4));
    assert_eq!(opened.document.study, Study::from_json(CAVITY).unwrap());
    assert_eq!(opened.document.artifacts, ArtifactIndex::default());
}

#[test]
fn a_current_file_opens_without_notes() {
    let bytes = to_bytes(&Document::new(Study::from_json(CAVITY).unwrap())).unwrap();
    let opened = from_bytes(&bytes).unwrap();
    assert!(opened.notes.is_empty());
    assert_eq!(opened.schema_version, CURRENT_SCHEMA);
}

#[test]
fn a_newer_file_is_refused_before_any_other_member_is_read() {
    let bytes = archive(vec![
        (
            "manifest.json",
            manifest(json!(CURRENT_SCHEMA + 1)),
            DEFLATE,
        ),
        ("study.json", b"\xff\xfe this is not json".to_vec(), DEFLATE),
    ]);
    match from_bytes(&bytes).unwrap_err() {
        DocError::NewerSchema { found, supported } => {
            assert_eq!(found, CURRENT_SCHEMA + 1);
            assert_eq!(supported, CURRENT_SCHEMA);
        }
        other => panic!("{other}"),
    }
}

#[test]
fn the_largest_version_number_is_a_newer_file_and_a_non_integer_is_not_a_study_file() {
    let bytes = archive(vec![("manifest.json", manifest(json!(u32::MAX)), DEFLATE)]);
    assert!(matches!(
        from_bytes(&bytes).unwrap_err(),
        DocError::NewerSchema {
            found: u32::MAX,
            ..
        }
    ));

    for version in [
        json!("two"),
        json!(1.5),
        json!(-1),
        json!(u64::from(u32::MAX) + 1),
        json!(null),
    ] {
        let bytes = archive(vec![
            ("manifest.json", manifest(version.clone()), DEFLATE),
            ("study.json", CAVITY.as_bytes().to_vec(), DEFLATE),
        ]);
        match from_bytes(&bytes).unwrap_err() {
            DocError::NotAStudyFile(reason) => {
                assert!(reason.contains("schema_version"), "{version}: {reason}");
            }
            other => panic!("{version}: {other}"),
        }
    }
}

#[test]
fn an_older_study_that_cannot_be_migrated_returns_no_partial_document() {
    let mut study: Value = serde_json::from_str(CAVITY_V0).unwrap();
    study["bogus"] = json!(1);
    let bytes = archive(vec![
        ("manifest.json", manifest(json!(0)), DEFLATE),
        ("study.json", study.to_string().into_bytes(), DEFLATE),
    ]);
    match from_bytes(&bytes).unwrap_err() {
        DocError::Migrate { from, to, .. } => assert_eq!((from, to), (0, 1)),
        other => panic!("{other}"),
    }
}

#[test]
fn the_manifest_is_a_closed_record_but_a_newer_version_is_judged_first() {
    let bytes = archive(vec![
        (
            "manifest.json",
            manifest_with_extra_field(json!(CURRENT_SCHEMA)),
            DEFLATE,
        ),
        ("study.json", CAVITY.as_bytes().to_vec(), DEFLATE),
    ]);
    match from_bytes(&bytes).unwrap_err() {
        DocError::BadJson { member, .. } => assert_eq!(member, "manifest.json"),
        other => panic!("{other}"),
    }

    let bytes = archive(vec![
        (
            "manifest.json",
            manifest_with_extra_field(json!(CURRENT_SCHEMA + 1)),
            DEFLATE,
        ),
        ("study.json", CAVITY.as_bytes().to_vec(), DEFLATE),
    ]);
    assert!(matches!(
        from_bytes(&bytes).unwrap_err(),
        DocError::NewerSchema { .. }
    ));
}

#[test]
fn an_older_file_that_is_saved_becomes_a_current_file() {
    let opened = from_bytes(&old_file()).unwrap();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("doc-resave-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cavity.cfdcraft");

    save(&opened.document, &path).unwrap();

    let written = fs::read(&path).unwrap();
    let manifest: Value = serde_json::from_slice(&member(&written, "manifest.json")).unwrap();
    assert_eq!(manifest["schema_version"], CURRENT_SCHEMA);
    let study: Value = serde_json::from_slice(&member(&written, "study.json")).unwrap();
    assert_eq!(study["schema_version"], 1);

    let again = open(&path).unwrap();
    assert!(again.notes.is_empty());
    assert_eq!(again.schema_version, CURRENT_SCHEMA);
    assert_eq!(again.document.study, Study::from_json(CAVITY).unwrap());
}
