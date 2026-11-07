use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use cfdcraft_doc::{
    ArtifactIndex, DocError, Document, FORMAT_ID, MANIFEST_MEMBER, PLACEHOLDER_THUMBNAIL,
    STUDY_LIMIT, STUDY_MEMBER, from_bytes, open, save, to_bytes, write_members,
};
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::{Study, canonical_hash, resolve};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const PINNED_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";
const DEFLATE: CompressionMethod = CompressionMethod::Deflated;

fn cavity() -> Study {
    Study::from_json(CAVITY).unwrap()
}

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("doc-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// An archive built straight with the ZIP crate, so the tests do not depend on `to_bytes`.
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
    serde_json::json!({
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

#[test]
fn the_cavity_study_is_saved_and_reopens_unchanged() {
    let study = cavity();
    let doc = Document::new(study.clone());
    let path = fresh("tracer").join("cavity.cfdcraft");
    save(&doc, &path).unwrap();

    let opened = open(&path).unwrap();
    assert_eq!(opened.document.study, study);
    assert_eq!(
        canonical_hash(&resolve(&opened.document.study).unwrap()),
        PINNED_HASH
    );
    assert_eq!(opened.document.artifacts, ArtifactIndex::default());
    assert_eq!(opened.document.artifacts, doc.artifacts);
    assert_eq!(
        opened.document.thumbnail.as_deref(),
        Some(PLACEHOLDER_THUMBNAIL)
    );
    assert_eq!(opened.schema_version, CURRENT_SCHEMA);
    assert!(opened.notes.is_empty());
    assert_eq!(opened.document, doc);
}

#[test]
fn saving_is_byte_deterministic_with_a_fixed_member_order() {
    let doc = Document::new(cavity());
    let bytes = to_bytes(&doc).unwrap();
    assert_eq!(bytes, to_bytes(&doc).unwrap());

    let mut zip = ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
    let seen: Vec<(String, CompressionMethod)> = (0..zip.len())
        .map(|i| {
            let file = zip.by_index(i).unwrap();
            (file.name().unwrap().into_owned(), file.compression())
        })
        .collect();
    let names: Vec<&str> = seen.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "manifest.json",
            "study.json",
            "artifacts.json",
            "thumbnail.png"
        ]
    );
    let methods: Vec<CompressionMethod> = seen.iter().map(|(_, method)| *method).collect();
    assert_eq!(
        methods,
        [
            CompressionMethod::Deflated,
            CompressionMethod::Deflated,
            CompressionMethod::Deflated,
            CompressionMethod::Stored
        ]
    );
}

#[test]
fn the_manifest_and_the_study_are_plain_json() {
    let study = cavity();
    let bytes = to_bytes(&Document::new(study.clone())).unwrap();

    let parsed: serde_json::Value =
        serde_json::from_slice(&member(&bytes, "manifest.json")).unwrap();
    assert_eq!(parsed["format"], FORMAT_ID);
    assert_eq!(parsed["format"], "cfdcraft-study");
    assert_eq!(parsed["schema_version"], CURRENT_SCHEMA);
    assert_eq!(parsed["writer"]["name"], "cfdcraft");
    assert_eq!(parsed["writer"]["version"], env!("CARGO_PKG_VERSION"));

    let text = member(&bytes, "study.json");
    assert_eq!(
        String::from_utf8(text.clone()).unwrap(),
        serde_json::to_string_pretty(&study).unwrap() + "\n"
    );
    assert_eq!(serde_json::from_slice::<Study>(&text).unwrap(), study);
}

#[test]
fn files_that_are_not_study_files_are_told_apart() {
    assert!(matches!(
        from_bytes(b"this is not a zip file at all").unwrap_err(),
        DocError::NotAStudyFile(_)
    ));
    assert!(matches!(
        from_bytes(&[]).unwrap_err(),
        DocError::NotAStudyFile(_)
    ));

    let no_manifest = archive(vec![("study.json", CAVITY.as_bytes().to_vec(), DEFLATE)]);
    match from_bytes(&no_manifest).unwrap_err() {
        DocError::MissingMember(name) => assert_eq!(name, MANIFEST_MEMBER),
        other => panic!("{other}"),
    }

    let no_study = archive(vec![("manifest.json", manifest(1), DEFLATE)]);
    match from_bytes(&no_study).unwrap_err() {
        DocError::MissingMember(name) => assert_eq!(name, STUDY_MEMBER),
        other => panic!("{other}"),
    }

    let bare = archive(vec![
        ("manifest.json", manifest(1), DEFLATE),
        ("study.json", CAVITY.as_bytes().to_vec(), DEFLATE),
    ]);
    let opened = from_bytes(&bare).unwrap();
    assert_eq!(opened.document.study, cavity());
    assert_eq!(opened.document.artifacts, ArtifactIndex::default());
    assert_eq!(opened.document.thumbnail, None);
}

#[test]
fn a_repeated_member_name_is_refused() {
    // Two entries are written under different names of the same length, then one name is
    // renamed in the finished bytes (local header and central directory both), because the
    // writer itself refuses a repeated name.
    let mut bytes = archive(vec![
        ("manifest.json", manifest(1), DEFLATE),
        ("study.jsox", CAVITY.as_bytes().to_vec(), DEFLATE),
        ("study.json", CAVITY.as_bytes().to_vec(), DEFLATE),
    ]);
    let from = b"study.jsox";
    let to = b"study.json";
    let mut hits = 0;
    let mut at = 0;
    while at + from.len() <= bytes.len() {
        if bytes[at..at + from.len()] == from[..] {
            bytes[at..at + from.len()].copy_from_slice(to);
            hits += 1;
            at += from.len();
        } else {
            at += 1;
        }
    }
    assert!(hits >= 2, "the entry name was found {hits} times");
    assert!(matches!(
        from_bytes(&bytes).unwrap_err(),
        DocError::DuplicateMember
    ));
}

#[test]
fn an_oversized_member_is_refused_by_name_and_limit() {
    let huge = format!("{{{}}}", " ".repeat(33 * 1024 * 1024));
    let bytes = archive(vec![
        ("manifest.json", manifest(1), DEFLATE),
        ("study.json", huge.into_bytes(), DEFLATE),
    ]);
    match from_bytes(&bytes).unwrap_err() {
        DocError::TooLarge { name, limit } => {
            assert_eq!(name, STUDY_MEMBER);
            assert_eq!(limit, STUDY_LIMIT);
        }
        other => panic!("{other}"),
    }
}

#[test]
fn unknown_entries_are_ignored_and_never_become_paths() {
    let doc = Document::new(cavity());
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    write_members(&doc, &mut zip).unwrap();
    for name in ["../../evil", "blobs/x"] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(b"junk").unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();

    assert_eq!(from_bytes(&bytes).unwrap().document, doc);
    let outside = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("../../evil");
    assert!(!outside.exists());
}

#[test]
fn a_manifest_of_another_format_is_refused() {
    let other_format = serde_json::json!({
        "format": "something-else",
        "schema_version": 1,
        "writer": { "name": "x", "version": "1" }
    })
    .to_string()
    .into_bytes();
    let bytes = archive(vec![
        ("manifest.json", other_format, DEFLATE),
        ("study.json", CAVITY.as_bytes().to_vec(), DEFLATE),
    ]);
    assert!(matches!(
        from_bytes(&bytes).unwrap_err(),
        DocError::NotAStudyFile(_)
    ));
}

#[test]
fn the_placeholder_thumbnail_is_a_64_by_64_png() {
    let png = PLACEHOLDER_THUMBNAIL;
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 64);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 64);
}
