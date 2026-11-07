//! A study with stored results packs to one file and unpacks elsewhere with the results present.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Barrier;
use std::thread;

use cfdcraft_doc::{BlobKind, BlobRef, DocError, Document, Link, open, save, store_dir};
use cfdcraft_model::{Study, canonical_hash, resolve};
use cfdcraft_pack::{PackError, pack, unpack, unpack_limited};
use cfdcraft_store::{ContentHash, Store};
use zip::{CompressionMethod, ZipArchive};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const PINNED_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";

fn cavity() -> Study {
    Study::from_json(CAVITY).unwrap()
}

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("pack-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn big() -> Vec<u8> {
    (0..3 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
        .collect()
}

fn small() -> Vec<u8> {
    (0..2048u32)
        .map(|i| (i.wrapping_mul(40503) >> 8) as u8)
        .collect()
}

fn link() -> Link {
    Link {
        id: "geometry".to_owned(),
        path: "inputs/body.step".to_owned(),
        name: "body.step".to_owned(),
        hash: ContentHash::of(b"body"),
    }
}

/// The cavity document saved as `<dir>/<stem>.cfdcraft`, with a store beside it holding `blobs`.
fn make(dir: &Path, stem: &str, blobs: &[(Vec<u8>, BlobKind)], links: Vec<Link>) -> PathBuf {
    let path = dir.join(format!("{stem}.cfdcraft"));
    let mut doc = Document::new(cavity());
    if !blobs.is_empty() {
        let store = Store::create(store_dir(&path)).unwrap();
        for (bytes, kind) in blobs {
            doc.artifacts.blobs.push(BlobRef {
                hash: store.put_bytes(bytes).unwrap(),
                size: bytes.len() as u64,
                kind: *kind,
            });
        }
    }
    doc.artifacts.links = links;
    save(&doc, &path).unwrap();
    path
}

fn two_results(dir: &Path) -> PathBuf {
    make(
        dir,
        "cavity",
        &[(big(), BlobKind::Mesh), (small(), BlobKind::Result)],
        Vec::new(),
    )
}

fn entries(bytes: &[u8]) -> Vec<(String, CompressionMethod)> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let file = zip.by_index(i).unwrap();
            (file.name().unwrap().into_owned(), file.compression())
        })
        .collect()
}

#[test]
fn a_study_with_two_results_packs_and_unpacks_into_another_folder() {
    let dir = fresh("tracer");
    let (a, b) = (dir.join("a"), dir.join("b"));
    fs::create_dir_all(&a).unwrap();
    let doc_a = two_results(&a);
    let packed = a.join("cavity.cfdpack");

    let report = pack(&doc_a, &packed).unwrap();
    assert_eq!(report.blobs, 2);

    let unpacked = unpack(&packed, &b).unwrap();
    assert_eq!(unpacked.target, b.join("cavity.cfdcraft"));
    assert_eq!(unpacked.blobs, 2);

    let (from, to) = (open(&doc_a).unwrap(), open(&unpacked.target).unwrap());
    assert_eq!(to.document.study, cavity());
    assert_eq!(
        canonical_hash(&resolve(&to.document.study).unwrap()),
        PINNED_HASH
    );
    assert_eq!(to.document.artifacts, from.document.artifacts);
    assert_eq!(to.document.artifacts.blobs.len(), 2);

    let store_a = Store::open(store_dir(&doc_a)).unwrap();
    let store_b = Store::open(store_dir(&unpacked.target)).unwrap();
    for blob in &to.document.artifacts.blobs {
        assert!(store_b.contains(&blob.hash));
        store_b.verify(&blob.hash).unwrap();
        assert_eq!(
            fs::read(store_b.blob_path(&blob.hash)).unwrap(),
            fs::read(store_a.blob_path(&blob.hash)).unwrap()
        );
    }
}

#[test]
fn the_pack_is_an_ordinary_document_with_stored_result_entries() {
    let dir = fresh("layout");
    let doc = two_results(&dir);
    let packed = dir.join("cavity.cfdpack");
    pack(&doc, &packed).unwrap();

    let listed = entries(&fs::read(&packed).unwrap());
    let names: Vec<&str> = listed.iter().map(|(name, _)| name.as_str()).collect();
    let index = open(&doc).unwrap().document.artifacts;
    let mut expected = vec![
        "manifest.json".to_owned(),
        "study.json".to_owned(),
        "artifacts.json".to_owned(),
        "thumbnail.png".to_owned(),
    ];
    expected.extend(index.blobs.iter().map(|b| format!("blobs/{}", b.hash)));
    assert_eq!(names, expected);
    for (name, method) in &listed[4..] {
        assert_eq!(*method, CompressionMethod::Stored, "{name}");
    }

    let plain = open(&packed).unwrap();
    assert_eq!(plain.document.study, cavity());
    assert_eq!(plain.document.artifacts, index);
}

#[test]
fn packing_twice_gives_identical_bytes() {
    let dir = fresh("deterministic");
    let doc = two_results(&dir);
    let (one, two) = (dir.join("one.cfdpack"), dir.join("two.cfdpack"));
    pack(&doc, &one).unwrap();
    pack(&doc, &two).unwrap();
    assert_eq!(fs::read(one).unwrap(), fs::read(two).unwrap());
}

#[test]
fn the_report_counts_results_and_names_the_links_it_left_out() {
    let dir = fresh("report");
    let doc = two_results(&dir);
    let report = pack(&doc, &dir.join("one.cfdpack")).unwrap();
    assert_eq!(report.blobs, 2);
    assert_eq!(report.bytes, (big().len() + small().len()) as u64);
    assert!(report.links_not_included.is_empty());

    let linked = make(&dir, "linked", &[(small(), BlobKind::Result)], vec![link()]);
    let report = pack(&linked, &dir.join("linked.cfdpack")).unwrap();
    assert_eq!(report.links_not_included, vec![link()]);
    let unpacked = unpack(&dir.join("linked.cfdpack"), &dir.join("out")).unwrap();
    assert_eq!(unpacked.links, vec![link()]);
}

#[test]
fn a_result_listed_twice_is_written_once() {
    let dir = fresh("twice");
    let path = make(&dir, "twice", &[(small(), BlobKind::Result)], Vec::new());
    let mut doc = open(&path).unwrap().document;
    let again = doc.artifacts.blobs[0].clone();
    doc.artifacts.blobs.push(again);
    save(&doc, &path).unwrap();

    let packed = dir.join("twice.cfdpack");
    let report = pack(&path, &packed).unwrap();
    assert_eq!(report.blobs, 1);
    let results = entries(&fs::read(&packed).unwrap())
        .iter()
        .filter(|(name, _)| name.starts_with("blobs/"))
        .count();
    assert_eq!(results, 1);

    let unpacked = unpack(&packed, &dir.join("out")).unwrap();
    assert_eq!(unpacked.blobs, 1);
    let opened = open(&unpacked.target).unwrap();
    assert_eq!(opened.document.artifacts, doc.artifacts);
    Store::open(store_dir(&unpacked.target))
        .unwrap()
        .verify(&doc.artifacts.blobs[0].hash)
        .unwrap();
}

#[test]
fn a_study_with_no_results_needs_no_store() {
    let dir = fresh("empty");
    let doc = make(&dir, "bare", &[], Vec::new());
    assert!(!store_dir(&doc).exists());

    let packed = dir.join("bare.cfdpack");
    let report = pack(&doc, &packed).unwrap();
    assert_eq!((report.blobs, report.bytes), (0, 0));

    let unpacked = unpack(&packed, &dir.join("out")).unwrap();
    assert_eq!(unpacked.blobs, 0);
    assert_eq!(open(&unpacked.target).unwrap().document.study, cavity());
    let store = store_dir(&unpacked.target);
    if store.exists() {
        assert_eq!(fs::read_dir(store.join("blobs")).unwrap().count(), 0);
    }
}

/// Run `job` on 6 threads released together; every result comes back.
fn together<T: Send>(job: impl Fn() -> T + Sync) -> Vec<T> {
    let start = Barrier::new(6);
    thread::scope(|scope| {
        let runners: Vec<_> = (0..6)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    job()
                })
            })
            .collect();
        runners.into_iter().map(|r| r.join().unwrap()).collect()
    })
}

#[test]
fn racing_packs_never_replace_each_other() {
    let dir = fresh("pack-race");
    let doc = two_results(&dir);
    let out = dir.join("cavity.cfdpack");
    let results = together(|| pack(&doc, &out));
    assert_eq!(results.iter().flatten().count(), 1);
    for refused in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(matches!(refused, PackError::TargetExists(p) if *p == out));
    }
}

#[test]
fn racing_unpacks_never_replace_the_study_file() {
    let dir = fresh("unpack-race");
    let packed = dir.join("cavity.cfdpack");
    pack(&two_results(&dir), &packed).unwrap();
    let into = dir.join("into");
    let results = together(|| unpack(&packed, &into));
    assert_eq!(results.iter().flatten().count(), 1);
    for refused in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(
            matches!(
                refused,
                PackError::TargetExists(_) | PackError::Doc(DocError::TargetExists(_))
            ),
            "{refused}"
        );
    }
    open(&into.join("cavity.cfdcraft")).unwrap();
}

#[test]
fn unpack_limited_refuses_results_over_the_limit_and_accepts_them_at_it() {
    let dir = fresh("limit");
    let packed = dir.join("cavity.cfdpack");
    pack(&two_results(&dir), &packed).unwrap();
    let total = (big().len() + small().len()) as u64;
    let into = dir.join("into");

    assert!(matches!(
        unpack_limited(&packed, &into, total - 1),
        Err(PackError::OverLimit(_))
    ));
    assert!(!into.exists());

    let done = unpack_limited(&packed, &into, total).unwrap();
    assert_eq!(done.blobs, 2);
}
