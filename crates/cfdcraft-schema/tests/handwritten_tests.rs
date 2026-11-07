use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;

use cfdcraft_doc::{BlobKind, from_bytes};
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::{Study, canonical_hash, resolve};
use cfdcraft_schema::validate;
use serde_json::{Value, json};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const MANIFEST: &[u8] = include_bytes!("fixtures/handwritten/manifest.json");
const STUDY: &[u8] = include_bytes!("fixtures/handwritten/study.json");
const ARTIFACTS: &[u8] = include_bytes!("fixtures/handwritten/artifacts.json");

#[derive(Clone, Copy)]
enum Member {
    Study,
    Manifest,
    Artifacts,
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The published schema file for a member, read from `schema/` as a tool author would get it.
fn published_schema(member: Member) -> Value {
    let kind = match member {
        Member::Study => "study",
        Member::Manifest => "manifest",
        Member::Artifacts => "artifacts",
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schema")
        .join(format!("cfdcraft-{kind}-v{CURRENT_SCHEMA}.json"));
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
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

/// File (relative to `tests/fixtures`), which member it is, whether the schema accepts it, and
/// whether the reader accepts the file it makes. The two columns agree on every row except the
/// unit alias.
const CASES: &[(&str, Member, bool, bool)] = &[
    ("handwritten/manifest.json", Member::Manifest, true, true),
    ("handwritten/study.json", Member::Study, true, true),
    ("handwritten/artifacts.json", Member::Artifacts, true, true),
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
    // The reader accepts the spelling `m²/s` as another way to write the canonical `m2/s`. The
    // schema lists the canonical spellings only, so it is stricter here than the reader.
    ("invalid/study_unit_alias.json", Member::Study, false, true),
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
    };
    (schema_accepts, from_bytes(&whole).is_ok())
}

#[test]
fn schema_and_reader_agree_on_every_fixture_but_the_unit_alias() {
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
fn only_the_unit_alias_is_stricter_in_the_schema() {
    let differ: Vec<&str> = CASES
        .iter()
        .filter(|(_, _, schema_accepts, reader_accepts)| schema_accepts != reader_accepts)
        .map(|(file, ..)| *file)
        .collect();
    assert_eq!(differ, ["invalid/study_unit_alias.json"]);
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
