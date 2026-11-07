//! A failed unpack removes the empty store folder it created and nothing else: results already
//! stored stay, and a folder that was there before is left alone. Every bad pack is built here with
//! the ZIP crate and differs from a good one in one way.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use cfdcraft_doc::{BlobKind, BlobRef, Document, open, write_members};
use cfdcraft_model::Study;
use cfdcraft_pack::{PackError, unpack};
use cfdcraft_store::{ContentHash, Store};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("cleanup-{name}-{}", std::process::id()));
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

/// A document that lists the given results (no store is needed to describe them).
fn listing(results: &[Vec<u8>]) -> Document {
    let mut doc = Document::new(Study::from_json(CAVITY).unwrap());
    for bytes in results {
        doc.artifacts.blobs.push(BlobRef {
            hash: ContentHash::of(bytes),
            size: bytes.len() as u64,
            kind: BlobKind::Result,
        });
    }
    doc
}

/// A pack of the document with one stored entry per result; the entry at `damaged`, if any, has
/// its first byte changed.
fn write_pack(dir: &Path, results: &[Vec<u8>], damaged: Option<usize>) -> (PathBuf, Document) {
    let doc = listing(results);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    write_members(&doc, &mut zip).unwrap();
    for (n, (blob, bytes)) in doc.artifacts.blobs.iter().zip(results).enumerate() {
        let mut bytes = bytes.clone();
        if damaged == Some(n) {
            bytes[0] ^= 0xff;
        }
        zip.start_file(
            format!("blobs/{}", blob.hash),
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let path = dir.join("x.cfdpack");
    fs::write(&path, zip.finish().unwrap().into_inner()).unwrap();
    (path, doc)
}

#[test]
fn a_pack_whose_first_result_is_damaged_leaves_no_store_folder() {
    let dir = fresh("first-damaged");
    let (packed, doc) = write_pack(&dir, &[first()], Some(0));
    let out = dir.join("out");

    let error = unpack(&packed, &out).unwrap_err();
    assert!(
        matches!(error, PackError::Damaged(h) if h == doc.artifacts.blobs[0].hash),
        "{error}"
    );
    assert!(!out.join("x.cfdcraft").exists());
    assert!(!out.join("x.cfdstore").exists());
}

#[test]
fn a_store_folder_that_was_already_there_is_left_alone() {
    let dir = fresh("store-there");
    let (packed, _) = write_pack(&dir, &[first()], Some(0));
    let out = dir.join("out");
    Store::create(out.join("x.cfdstore")).unwrap();

    assert!(unpack(&packed, &out).is_err());
    assert!(out.join("x.cfdstore/blobs").is_dir());
    assert!(!out.join("x.cfdcraft").exists());
}

#[test]
fn a_failure_after_some_results_were_stored_keeps_them() {
    let dir = fresh("some-stored");
    let (packed, doc) = write_pack(&dir, &[first(), second()], Some(1));
    let out = dir.join("out");

    let error = unpack(&packed, &out).unwrap_err();
    assert!(
        matches!(error, PackError::Damaged(h) if h == doc.artifacts.blobs[1].hash),
        "{error}"
    );
    assert!(!out.join("x.cfdcraft").exists());
    let store = Store::open(out.join("x.cfdstore")).unwrap();
    store.verify(&doc.artifacts.blobs[0].hash).unwrap();
    assert!(!store.contains(&doc.artifacts.blobs[1].hash));
}

#[test]
fn a_second_unpack_that_loses_the_race_keeps_the_winners_results() {
    let dir = fresh("second-loses");
    let (packed, doc) = write_pack(&dir, &[first(), second()], None);
    let out = dir.join("out");
    unpack(&packed, &out).unwrap();

    let error = unpack(&packed, &out).unwrap_err();
    assert!(matches!(error, PackError::TargetExists(_)), "{error}");
    let store = Store::open(out.join("x.cfdstore")).unwrap();
    for blob in &doc.artifacts.blobs {
        store.verify(&blob.hash).unwrap();
    }
    assert_eq!(
        open(&out.join("x.cfdcraft")).unwrap().document.artifacts,
        doc.artifacts
    );
}
