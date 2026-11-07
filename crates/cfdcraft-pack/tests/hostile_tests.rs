//! Damaged, incomplete and hostile packs never leave a study pointing at missing results.
//! Every bad pack is built here with the ZIP crate and differs from a good one in one way.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use cfdcraft_doc::{BlobKind, BlobRef, Document, open, save, store_dir, write_members};
use cfdcraft_model::Study;
use cfdcraft_pack::{PackError, pack, unpack};
use cfdcraft_store::{ContentHash, Store};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const STORED: CompressionMethod = CompressionMethod::Stored;

fn cavity() -> Study {
    Study::from_json(CAVITY).unwrap()
}

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("hostile-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn first() -> Vec<u8> {
    (0..64 * 1024u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
        .collect()
}

fn second() -> Vec<u8> {
    (0..2048u32)
        .map(|i| (i.wrapping_mul(40503) >> 8) as u8)
        .collect()
}

/// The cavity document with a store beside it holding both results, saved as `<dir>/x.cfdcraft`.
fn study(dir: &Path) -> PathBuf {
    let path = dir.join("x.cfdcraft");
    let store = Store::create(store_dir(&path)).unwrap();
    let mut doc = Document::new(cavity());
    for (bytes, kind) in [(first(), BlobKind::Mesh), (second(), BlobKind::Result)] {
        doc.artifacts.blobs.push(BlobRef {
            hash: store.put_bytes(&bytes).unwrap(),
            size: bytes.len() as u64,
            kind,
        });
    }
    save(&doc, &path).unwrap();
    path
}

/// The two results with the hash each is listed under.
fn results(doc: &Document) -> Vec<(ContentHash, Vec<u8>)> {
    doc.artifacts
        .blobs
        .iter()
        .zip([first(), second()])
        .map(|(blob, bytes)| (blob.hash, bytes))
        .collect()
}

/// A pack built by hand: the document's members, then the given entries in order.
fn build(doc: &Document, entries: &[(String, Vec<u8>, CompressionMethod)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    write_members(doc, &mut zip).unwrap();
    for (name, bytes, method) in entries {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(*method),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn good_entries(doc: &Document) -> Vec<(String, Vec<u8>, CompressionMethod)> {
    results(doc)
        .into_iter()
        .map(|(hash, bytes)| (format!("blobs/{hash}"), bytes, STORED))
        .collect()
}

fn write_pack(dir: &Path, bytes: &[u8]) -> PathBuf {
    let path = dir.join("x.cfdpack");
    fs::write(&path, bytes).unwrap();
    path
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, base, out);
        } else {
            out.push(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

#[test]
fn packing_refuses_a_result_that_is_missing_or_wrong_and_writes_nothing() {
    for case in ["missing", "flipped", "truncated"] {
        let dir = fresh(case);
        let doc_path = study(&dir);
        let doc = open(&doc_path).unwrap().document;
        let store = Store::open(store_dir(&doc_path)).unwrap();
        let hash = doc.artifacts.blobs[1].hash;
        let blob = store.blob_path(&hash);
        match case {
            "missing" => fs::remove_file(&blob).unwrap(),
            "flipped" => {
                let mut bytes = fs::read(&blob).unwrap();
                bytes[0] ^= 0xff;
                fs::write(&blob, bytes).unwrap();
            }
            _ => {
                let bytes = fs::read(&blob).unwrap();
                fs::write(&blob, &bytes[..bytes.len() - 1]).unwrap();
            }
        }

        let out = dir.join("x.cfdpack");
        let error = pack(&doc_path, &out).unwrap_err();
        match case {
            "missing" => assert!(
                matches!(error, PackError::Missing(h) if h == hash),
                "{error}"
            ),
            "flipped" => assert!(
                matches!(error, PackError::Corrupt(h) if h == hash),
                "{error}"
            ),
            _ => assert!(
                matches!(error, PackError::SizeMismatch(h) if h == hash),
                "{error}"
            ),
        }
        assert!(error.to_string().contains(&hash.to_string()), "{error}");
        assert!(!out.exists(), "{case}: an output file was left");
    }
}

#[test]
fn a_changed_result_in_the_pack_is_damaged_and_no_study_is_written() {
    let dir = fresh("tampered");
    let doc = open(&study(&dir)).unwrap().document;
    let mut entries = good_entries(&doc);
    entries[1].1[0] ^= 0xff;
    let packed = write_pack(&dir, &build(&doc, &entries));
    let hash = doc.artifacts.blobs[1].hash;

    let out = dir.join("out");
    let error = unpack(&packed, &out).unwrap_err();
    assert!(
        matches!(error, PackError::Damaged(h) if h == hash),
        "{error}"
    );
    assert!(!out.join("x.cfdcraft").exists());
    assert!(!out.join("x.cfdstore/blobs").join(hash.to_string()).exists());
}

#[test]
fn a_pack_without_a_listed_result_is_incomplete() {
    let dir = fresh("incomplete");
    let doc = open(&study(&dir)).unwrap().document;
    let mut entries = good_entries(&doc);
    entries.remove(0);
    let packed = write_pack(&dir, &build(&doc, &entries));

    let out = dir.join("out");
    let error = unpack(&packed, &out).unwrap_err();
    let hash = doc.artifacts.blobs[0].hash;
    assert!(
        matches!(error, PackError::Incomplete(h) if h == hash),
        "{error}"
    );
    assert!(!out.join("x.cfdcraft").exists());
}

#[test]
fn entry_names_are_never_used_as_paths() {
    let dir = fresh("names");
    let doc = open(&study(&dir)).unwrap().document;
    let (hash, _) = results(&doc).remove(0);
    let stray = ContentHash::of(b"not in the index");
    let mut entries = good_entries(&doc);
    for name in [
        "blobs/../../evil".to_owned(),
        format!("blobs/{}", hash.to_string().to_uppercase()),
        "/absolute".to_owned(),
        format!("blobs/{stray}"),
        "notes.txt".to_owned(),
    ] {
        entries.push((name, b"hostile".to_vec(), STORED));
    }
    let packed = write_pack(&dir, &build(&doc, &entries));

    let out = dir.join("out");
    let report = unpack(&packed, &out).unwrap();
    assert_eq!(report.ignored_entries, 5);
    assert_eq!(report.blobs, 2);

    let mut found = Vec::new();
    walk(&out, &out, &mut found);
    found.sort();
    let mut expected = vec!["x.cfdcraft".to_owned()];
    expected.extend(
        doc.artifacts
            .blobs
            .iter()
            .map(|b| format!("x.cfdstore/blobs/{}", b.hash)),
    );
    expected.sort();
    assert_eq!(found, expected);
    assert!(!dir.join("evil").exists());
    assert!(!dir.parent().unwrap().join("evil").exists());
    assert!(!dir.join("notes.txt").exists());
    assert!(!Path::new("/absolute").exists());
}

#[test]
fn a_compressed_result_entry_is_refused() {
    let dir = fresh("compressed");
    let doc = open(&study(&dir)).unwrap().document;
    let mut entries = good_entries(&doc);
    entries[0].2 = CompressionMethod::Deflated;
    let packed = write_pack(&dir, &build(&doc, &entries));

    let out = dir.join("out");
    let error = unpack(&packed, &out).unwrap_err();
    let hash = doc.artifacts.blobs[0].hash;
    assert!(
        matches!(error, PackError::EntryNotStored(h) if h == hash),
        "{error}"
    );
    assert!(!out.join("x.cfdcraft").exists());
}

#[test]
fn a_recorded_size_smaller_than_the_entry_writes_nothing_beyond_it() {
    let dir = fresh("lying");
    let mut doc = open(&study(&dir)).unwrap().document;
    let entries = good_entries(&doc);
    doc.artifacts.blobs[0].size -= 10;
    let declared = doc.artifacts.blobs[0].size;
    let packed = write_pack(&dir, &build(&doc, &entries));

    let out = dir.join("out");
    let error = unpack(&packed, &out).unwrap_err();
    assert!(
        matches!(error, PackError::Damaged(_) | PackError::SizeMismatch(_)),
        "{error}"
    );
    assert!(!out.join("x.cfdcraft").exists());
    let blobs = out.join("x.cfdstore/blobs");
    if blobs.exists() {
        for entry in fs::read_dir(blobs).unwrap() {
            assert!(entry.unwrap().metadata().unwrap().len() <= declared);
        }
    }
}

#[test]
fn an_existing_study_file_is_never_replaced() {
    let dir = fresh("exists");
    let doc_path = study(&dir);
    let packed = dir.join("x.cfdpack");
    pack(&doc_path, &packed).unwrap();

    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("x.cfdcraft"), b"mine").unwrap();
    let error = unpack(&packed, &out).unwrap_err();
    assert!(matches!(error, PackError::TargetExists(_)), "{error}");
    assert_eq!(fs::read(out.join("x.cfdcraft")).unwrap(), b"mine");
    assert!(!out.join("x.cfdstore").exists());
}

#[test]
fn unpacking_is_repeatable_and_leaves_a_result_already_in_the_store_alone() {
    let dir = fresh("repeat");
    let doc_path = study(&dir);
    let doc = open(&doc_path).unwrap().document;
    let packed = dir.join("x.cfdpack");
    pack(&doc_path, &packed).unwrap();

    let (one, two) = (dir.join("one"), dir.join("two"));
    unpack(&packed, &one).unwrap();
    unpack(&packed, &two).unwrap();
    for blob in &doc.artifacts.blobs {
        let name = blob.hash.to_string();
        assert_eq!(
            fs::read(one.join("x.cfdstore/blobs").join(&name)).unwrap(),
            fs::read(two.join("x.cfdstore/blobs").join(&name)).unwrap()
        );
    }

    let three = dir.join("three");
    let early = Store::create(three.join("x.cfdstore")).unwrap();
    let kept = early.put_bytes(&first()).unwrap();
    let before = fs::metadata(early.blob_path(&kept))
        .unwrap()
        .modified()
        .unwrap();
    unpack(&packed, &three).unwrap();
    let after = fs::metadata(early.blob_path(&kept))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
    for blob in &doc.artifacts.blobs {
        early.verify(&blob.hash).unwrap();
    }
}

#[test]
fn a_pack_of_an_older_schema_unpacks_as_a_current_file() {
    let dir = fresh("old");
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let plain = SimpleFileOptions::default().compression_method(STORED);
    zip.start_file("manifest.json", plain).unwrap();
    zip.write_all(
        serde_json::json!({
            "format": "cfdcraft-study",
            "schema_version": 0,
            "writer": { "name": "other-tool", "version": "9" }
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    zip.start_file("study.json", plain).unwrap();
    zip.write_all(CAVITY_V0.as_bytes()).unwrap();
    let old = dir.join("old.cfdcraft");
    fs::write(&old, zip.finish().unwrap().into_inner()).unwrap();

    let packed = dir.join("old.cfdpack");
    pack(&old, &packed).unwrap();
    let report = unpack(&packed, &dir.join("out")).unwrap();
    let fresh_file = dir.join("fresh.cfdcraft");
    save(&Document::new(cavity()), &fresh_file).unwrap();
    let current = open(&fresh_file).unwrap().schema_version;
    let opened = open(&report.target).unwrap();
    assert_eq!(opened.schema_version, current);
    assert!(opened.notes.is_empty());
    assert_eq!(opened.document.study, cavity());
}
