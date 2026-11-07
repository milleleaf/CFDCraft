//! A result the index lists as pruned was removed on purpose: a pack leaves it out and unpacking
//! does not look for it, while a result that is simply missing is still an error.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cfdcraft_doc::{BlobKind, BlobRef, Document, open, save, store_dir};
use cfdcraft_model::Study;
use cfdcraft_pack::{MAX_RESULTS, PackError, pack, unpack};
use cfdcraft_store::{ContentHash, Store};
use zip::ZipArchive;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("pruned-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn kept_bytes() -> Vec<u8> {
    (0..2048u32)
        .map(|i| (i.wrapping_mul(40503) >> 8) as u8)
        .collect()
}

fn pruned_bytes() -> Vec<u8> {
    (0..4096u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
        .collect()
}

/// A document listing a stored result `a` and a result `b` that is not in the store. `b` is on the
/// pruned list when `prune_b` is set.
fn make(dir: &Path, prune_b: bool) -> (PathBuf, ContentHash, ContentHash) {
    let path = dir.join("cavity.cfdcraft");
    let store = Store::create(store_dir(&path)).unwrap();
    let (kept, gone) = (kept_bytes(), pruned_bytes());
    let a = store.put_bytes(&kept).unwrap();
    let b = ContentHash::of(&gone);
    let mut document = Document::new(Study::from_json(CAVITY).unwrap());
    document.artifacts.blobs = vec![
        BlobRef {
            hash: a,
            size: kept.len() as u64,
            kind: BlobKind::Result,
        },
        BlobRef {
            hash: b,
            size: gone.len() as u64,
            kind: BlobKind::Result,
        },
    ];
    if prune_b {
        document.artifacts.pruned = vec![b];
    }
    save(&document, &path).unwrap();
    (path, a, b)
}

fn entry_names(packed: &Path) -> Vec<String> {
    let mut zip = ZipArchive::new(Cursor::new(fs::read(packed).unwrap())).unwrap();
    (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
        .collect()
}

#[test]
fn a_document_with_a_pruned_result_packs_without_it() {
    let dir = fresh("packs");
    let (path, a, b) = make(&dir, true);
    let packed = dir.join("cavity.cfdpack");

    let report = pack(&path, &packed).unwrap();
    assert_eq!(report.blobs, 1);
    assert_eq!(report.bytes, kept_bytes().len() as u64);

    let names = entry_names(&packed);
    assert!(names.contains(&format!("blobs/{a}")));
    assert!(!names.contains(&format!("blobs/{b}")));
}

#[test]
fn the_pack_unpacks_to_a_consistent_document() {
    let dir = fresh("unpacks");
    let (path, a, b) = make(&dir, true);
    let packed = dir.join("cavity.cfdpack");
    pack(&path, &packed).unwrap();

    let there = dir.join("elsewhere");
    let done = unpack(&packed, &there).unwrap();
    assert_eq!(done.blobs, 1);
    assert_eq!(done.ignored_entries, 0);

    let document = open(&done.target).unwrap().document;
    assert_eq!(document.artifacts.pruned, vec![b]);
    assert_eq!(document.artifacts.blobs.len(), 2);
    let store = Store::open(store_dir(&done.target)).unwrap();
    assert!(store.contains(&a));
    store.verify(&a).unwrap();
    assert!(!store.contains(&b));
}

#[test]
fn a_missing_result_that_is_not_pruned_is_still_an_error() {
    let dir = fresh("missing");
    let (path, _, b) = make(&dir, false);
    let packed = dir.join("cavity.cfdpack");

    assert!(matches!(
        pack(&path, &packed),
        Err(PackError::Missing(hash)) if hash == b
    ));
    assert!(!packed.exists());
}

#[test]
fn a_pruned_hash_is_not_counted_by_the_distinct_blob_limit() {
    let dir = fresh("limit");
    let path = dir.join("cavity.cfdcraft");
    let blobs: Vec<BlobRef> = (0..=MAX_RESULTS as u32)
        .map(|n| BlobRef {
            hash: ContentHash::of(&n.to_le_bytes()),
            size: 1,
            kind: BlobKind::Result,
        })
        .collect();
    assert_eq!(blobs.len(), MAX_RESULTS + 1);
    let mut document = Document::new(Study::from_json(CAVITY).unwrap());
    document.artifacts.blobs = blobs;
    save(&document, &path).unwrap();
    let packed = dir.join("cavity.cfdpack");

    // One result too many: refused before anything is looked up in the store.
    assert!(matches!(pack(&path, &packed), Err(PackError::OverLimit(_))));

    // With one of them pruned the count is at the limit, so the limit no longer applies; the
    // pack then stops at the store, which this document does not have.
    document.artifacts.pruned = vec![document.artifacts.blobs[0].hash];
    save(&document, &path).unwrap();
    let refused = pack(&path, &packed).unwrap_err();
    assert!(!matches!(refused, PackError::OverLimit(_)), "{refused}");
}
