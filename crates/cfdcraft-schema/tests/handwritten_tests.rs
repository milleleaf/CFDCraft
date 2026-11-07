use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;

use cfdcraft_doc::{BlobKind, from_bytes};
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::{Study, canonical_hash, resolve};
use cfdcraft_queue::{QUEUE_SCHEMA, Queue};
use cfdcraft_schema::validate;
use serde_json::{Value, json};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const MANIFEST: &[u8] = include_bytes!("fixtures/handwritten/manifest.json");
const MANIFEST_V3: &[u8] = include_bytes!("fixtures/handwritten-v3/manifest.json");
const MANIFEST_V1: &[u8] = include_bytes!("fixtures/handwritten-v1/manifest.json");
const MANIFEST_V2: &[u8] = include_bytes!("fixtures/handwritten-v2/manifest.json");
const STUDY: &[u8] = include_bytes!("fixtures/handwritten/study.json");
const ARTIFACTS: &[u8] = include_bytes!("fixtures/handwritten/artifacts.json");
const QUEUE: &str = include_str!("fixtures/handwritten/queue.json");

#[derive(Clone, Copy)]
enum Member {
    Study,
    Manifest,
    Artifacts,
    Queue,
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn kind_of(member: Member) -> &'static str {
    match member {
        Member::Study => "study",
        Member::Manifest => "manifest",
        Member::Artifacts => "artifacts",
        Member::Queue => "queue",
    }
}

fn read_schema(path: PathBuf) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// The published schema file for a member, read from `schema/` as a tool author would get it. The
/// study and the queue have their own numbers; the manifest and the artifact index carry the number
/// of the file.
fn published_schema(member: Member) -> Value {
    let version = match member {
        Member::Study => Study::SCHEMA_VERSION,
        Member::Manifest | Member::Artifacts => CURRENT_SCHEMA,
        Member::Queue => QUEUE_SCHEMA,
    };
    read_schema(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../schema")
            .join(format!("cfdcraft-{}-v{version}.json", kind_of(member))),
    )
}

/// The frozen copy of an older published file.
fn frozen_schema(member: Member, version: u32) -> Value {
    read_schema(
        fixtures()
            .join("frozen")
            .join(format!("cfdcraft-{}-v{version}.json", kind_of(member))),
    )
}

/// A file assembled with the ZIP crate alone, so no CFDCraft writer is involved.
fn archive(manifest: &[u8], study: &[u8], artifacts: &[u8]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in [
        ("manifest.json", manifest),
        ("study.json", study),
        ("artifacts.json", artifacts),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn handwritten_members_validate_against_the_published_files() {
    for (member, bytes) in [
        (Member::Manifest, MANIFEST),
        (Member::Study, STUDY),
        (Member::Artifacts, ARTIFACTS),
    ] {
        let instance: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(validate(&published_schema(member), &instance), Ok(()));
    }
}

#[test]
fn a_handwritten_queue_validates_and_opens_through_the_reader() {
    let queue: Value = serde_json::from_str(QUEUE).unwrap();
    assert_eq!(validate(&published_schema(Member::Queue), &queue), Ok(()));
    let opened = Queue::from_json(QUEUE).unwrap();
    assert_eq!(opened.schema_version, QUEUE_SCHEMA);
    assert_eq!(opened.jobs.len(), 2);
    assert_eq!(opened.jobs[0].run_id.as_deref(), Some("cavity-4242-1"));
    assert!(opened.jobs[1].run_id.is_none());
}

#[test]
fn a_file_assembled_by_plain_zip_code_opens_with_the_expected_study() {
    let opened = from_bytes(&archive(MANIFEST, STUDY, ARTIFACTS)).unwrap();
    assert_eq!(opened.schema_version, CURRENT_SCHEMA);
    assert!(opened.notes.is_empty());

    let cavity = Study::from_json(CAVITY).unwrap();
    assert_eq!(
        canonical_hash(&resolve(&opened.document.study).unwrap()),
        canonical_hash(&resolve(&cavity).unwrap())
    );

    let index = &opened.document.artifacts;
    assert_eq!(index.blobs.len(), 1);
    assert_eq!(index.blobs[0].kind, BlobKind::Result);
    assert_eq!(index.blobs[0].size, 1024);
    assert_eq!(
        index.blobs[0].hash.to_string(),
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    );
    assert_eq!(index.links.len(), 1);
    assert_eq!(index.links[0].id, "geometry-1");
    assert_eq!(index.links[0].path, "../geometry/room.step");
    assert_eq!(index.links[0].name, "room.step");
}

#[test]
fn a_handwritten_schema_3_file_validates_against_the_frozen_files_and_opens_with_one_note() {
    let manifest: Value = serde_json::from_slice(MANIFEST_V3).unwrap();
    let study: Value = serde_json::from_slice(STUDY).unwrap();
    let artifacts: Value = serde_json::from_slice(ARTIFACTS).unwrap();
    assert_eq!(
        validate(&frozen_schema(Member::Manifest, 3), &manifest),
        Ok(())
    );
    assert_eq!(validate(&published_schema(Member::Study), &study), Ok(()));
    assert_eq!(
        validate(&frozen_schema(Member::Artifacts, 3), &artifacts),
        Ok(())
    );
    // The current manifest file allows its own number only.
    assert!(validate(&published_schema(Member::Manifest), &manifest).is_err());

    let opened = from_bytes(&archive(MANIFEST_V3, STUDY, ARTIFACTS)).unwrap();
    assert_eq!(opened.schema_version, 3);
    assert_eq!(opened.notes.len(), 1);
    assert_eq!((opened.notes[0].from, opened.notes[0].to), (3, 4));
    assert!(opened.document.artifacts.runs.is_empty());
}

#[test]
fn a_handwritten_schema_2_file_validates_against_the_frozen_files_and_opens_with_two_notes() {
    let manifest: Value = serde_json::from_slice(MANIFEST_V2).unwrap();
    let study: Value = serde_json::from_slice(STUDY).unwrap();
    let artifacts: Value = serde_json::from_slice(ARTIFACTS).unwrap();
    assert_eq!(
        validate(&frozen_schema(Member::Manifest, 2), &manifest),
        Ok(())
    );
    assert_eq!(validate(&published_schema(Member::Study), &study), Ok(()));
    assert_eq!(
        validate(&frozen_schema(Member::Artifacts, 2), &artifacts),
        Ok(())
    );
    // The current manifest file allows its own number only.
    assert!(validate(&published_schema(Member::Manifest), &manifest).is_err());

    let opened = from_bytes(&archive(MANIFEST_V2, STUDY, ARTIFACTS)).unwrap();
    assert_eq!(opened.schema_version, 2);
    assert_eq!(opened.notes.len(), 2);
    assert_eq!((opened.notes[0].from, opened.notes[0].to), (2, 3));
    assert_eq!((opened.notes[1].from, opened.notes[1].to), (3, 4));
    assert!(opened.document.artifacts.scenarios.is_empty());
    assert!(opened.document.artifacts.sweeps.is_empty());
    assert!(opened.document.artifacts.pruned.is_empty());
}

#[test]
fn a_handwritten_schema_1_file_validates_against_the_frozen_files_and_opens_with_three_notes() {
    let manifest: Value = serde_json::from_slice(MANIFEST_V1).unwrap();
    let study: Value = serde_json::from_slice(STUDY).unwrap();
    let artifacts: Value = serde_json::from_slice(ARTIFACTS).unwrap();
    assert_eq!(
        validate(&frozen_schema(Member::Manifest, 1), &manifest),
        Ok(())
    );
    assert_eq!(validate(&published_schema(Member::Study), &study), Ok(()));
    assert_eq!(
        validate(&frozen_schema(Member::Artifacts, 1), &artifacts),
        Ok(())
    );
    // The current manifest file allows its own number only.
    assert!(validate(&published_schema(Member::Manifest), &manifest).is_err());

    let opened = from_bytes(&archive(MANIFEST_V1, STUDY, ARTIFACTS)).unwrap();
    assert_eq!(opened.schema_version, 1);
    assert_eq!(opened.notes.len(), 3);
    let steps: Vec<(u32, u32)> = opened.notes.iter().map(|n| (n.from, n.to)).collect();
    assert_eq!(steps, [(1, 2), (2, 3), (3, 4)]);
    assert!(opened.document.artifacts.runs.is_empty());
}

/// File (relative to `tests/fixtures`), which member it is, whether the schema accepts it, and
/// whether the reader accepts the file it makes. The two columns agree on every row except eight:
/// the unit alias, the repeated link id, the run record whose keys are not those of its recipes,
/// a variant listed before its parent, a pruned hash that the list of stored results lacks, a
/// stopped run without a checkpoint, a run continued from a later run and a queue with two active
/// jobs.
const CASES: &[(&str, Member, bool, bool)] = &[
    ("handwritten/manifest.json", Member::Manifest, true, true),
    ("handwritten/study.json", Member::Study, true, true),
    ("handwritten/artifacts.json", Member::Artifacts, true, true),
    ("handwritten/queue.json", Member::Queue, true, true),
    (
        "invalid/study_extra_top_level_key.json",
        Member::Study,
        false,
        false,
    ),
    (
        "invalid/study_mesh_missing.json",
        Member::Study,
        false,
        false,
    ),
    (
        "invalid/study_cells_x_as_text.json",
        Member::Study,
        false,
        false,
    ),
    (
        "invalid/study_unknown_boundary_kind.json",
        Member::Study,
        false,
        false,
    ),
    (
        "invalid/study_schema_version_2.json",
        Member::Study,
        false,
        false,
    ),
    (
        "invalid/manifest_other_format.json",
        Member::Manifest,
        false,
        false,
    ),
    (
        "invalid/artifacts_uppercase_hash.json",
        Member::Artifacts,
        false,
        false,
    ),
    (
        "invalid/artifacts_unknown_blob_kind.json",
        Member::Artifacts,
        false,
        false,
    ),
    // A link id outside `[a-z0-9][a-z0-9-]{0,39}` is refused by both: the schema carries the rule.
    (
        "invalid/artifacts_bad_link_id.json",
        Member::Artifacts,
        false,
        false,
    ),
    // The reader accepts the spelling `m²/s` as another way to write the canonical `m2/s`. The
    // schema lists the canonical spellings only, so it is stricter here than the reader.
    ("invalid/study_unit_alias.json", Member::Study, false, true),
    // Two links with one id: JSON Schema cannot say that a field is unique among array items, so
    // the schema accepts the file and the reader refuses it. Documented in schema/README.md.
    (
        "invalid/artifacts_repeated_link_id.json",
        Member::Artifacts,
        true,
        false,
    ),
    // A run record is a closed record, so a key it does not define is refused by both.
    (
        "invalid/artifacts_run_unknown_key.json",
        Member::Artifacts,
        false,
        false,
    ),
    // The keys are well-formed hashes, so the schema accepts the record. A key must be the hash of
    // the recipe next to it, which only the reader can compute, and it refuses the file.
    (
        "invalid/artifacts_run_with_foreign_keys.json",
        Member::Artifacts,
        true,
        false,
    ),
    // A variant is a closed record, so a member it does not define is refused by both.
    (
        "invalid/artifacts_scenario_unknown_key.json",
        Member::Artifacts,
        false,
        false,
    ),
    // Both variants are well-formed, so the schema accepts the list. That a parent is listed before
    // its child involves two items, which only the reader checks, and it refuses the file.
    (
        "invalid/artifacts_scenario_parent_later.json",
        Member::Artifacts,
        true,
        false,
    ),
    // The hash is well-formed, so the schema accepts it. That a pruned hash is one the index lists
    // under `blobs` involves two members, which only the reader checks, and it refuses the file.
    (
        "invalid/artifacts_pruned_unlisted.json",
        Member::Artifacts,
        true,
        false,
    ),
    // The record is well formed, so the schema accepts it. That a stopped run holds a checkpoint
    // and no result involves several members of the record, which only the reader checks.
    (
        "invalid/artifacts_stopped_run_without_checkpoint.json",
        Member::Artifacts,
        true,
        false,
    ),
    // Both records are well formed. That a run continues an earlier run involves two records,
    // which only the reader checks.
    (
        "invalid/artifacts_continued_from_later_run.json",
        Member::Artifacts,
        true,
        false,
    ),
    // Both jobs are well formed. That only one job is active involves two jobs, which only the
    // queue reader checks.
    (
        "invalid/queue_two_active_jobs.json",
        Member::Queue,
        true,
        false,
    ),
];

/// The schema verdict and the reader verdict for one fixture. The reader verdict is that of the
/// whole file made by putting the fixture in its place among valid members.
fn verdicts(file: &str, member: Member) -> (bool, bool) {
    let bytes = fs::read(fixtures().join(file)).unwrap();
    let instance: Value = serde_json::from_slice(&bytes).unwrap();
    let schema_accepts = validate(&published_schema(member), &instance).is_ok();
    let whole = match member {
        Member::Manifest => archive(&bytes, STUDY, ARTIFACTS),
        Member::Study => archive(MANIFEST, &bytes, ARTIFACTS),
        Member::Artifacts => archive(MANIFEST, STUDY, &bytes),
        // The queue is not a member of the study file: its own reader gives the verdict.
        Member::Queue => {
            let text = String::from_utf8(bytes).unwrap();
            return (schema_accepts, Queue::from_json(&text).is_ok());
        }
    };
    (schema_accepts, from_bytes(&whole).is_ok())
}

#[test]
fn schema_and_reader_agree_on_every_fixture_but_the_eight_documented_ones() {
    for (file, member, schema_accepts, reader_accepts) in CASES {
        assert_eq!(
            verdicts(file, *member),
            (*schema_accepts, *reader_accepts),
            "{file}"
        );
    }
}

#[test]
fn every_invalid_fixture_is_in_the_table() {
    let listed: BTreeSet<String> = CASES
        .iter()
        .filter_map(|(file, ..)| file.strip_prefix("invalid/"))
        .map(str::to_owned)
        .collect();
    let on_disk: BTreeSet<String> = fs::read_dir(fixtures().join("invalid"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(listed, on_disk);
}

#[test]
fn only_the_listed_rules_differ() {
    let differ: Vec<&str> = CASES
        .iter()
        .filter(|(_, _, schema_accepts, reader_accepts)| schema_accepts != reader_accepts)
        .map(|(file, ..)| *file)
        .collect();
    assert_eq!(
        differ,
        [
            "invalid/study_unit_alias.json",
            "invalid/artifacts_repeated_link_id.json",
            "invalid/artifacts_run_with_foreign_keys.json",
            "invalid/artifacts_scenario_parent_later.json",
            "invalid/artifacts_pruned_unlisted.json",
            "invalid/artifacts_stopped_run_without_checkpoint.json",
            "invalid/artifacts_continued_from_later_run.json",
            "invalid/queue_two_active_jobs.json"
        ]
    );
}

#[test]
fn errors_name_the_instance_path() {
    let mut study: Value = serde_json::from_str(CAVITY).unwrap();
    study["mesh"]["cells_x"] = json!("128");
    let errors = validate(&published_schema(Member::Study), &study).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("/mesh/cells_x:")),
        "{errors:?}"
    );
}
