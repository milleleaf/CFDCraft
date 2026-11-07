//! A pack carries the run records with the study, and can be made without the host labels. The
//! study file the pack is made from always keeps its labels. A fixture runner stands in for the
//! compute host, so these tests run on every platform.

use std::fs;

use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::documents::pack_document_with;
use cfdcraft_ops::{pack_document, solve_document, unpack_document};
use cfdcraft_store::Store;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

/// A solved document `cavity.cfdcraft` in a fresh folder, with the folder.
fn solved(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    (dir, path)
}

#[test]
fn a_pack_without_redaction_carries_the_host_label() {
    let (dir, path) = solved("pack-plain");
    let out = dir.join("plain.cfdpack");
    passed(&pack_document_with(&path, &out, false).report);

    let packed = open(&out).unwrap().document;
    assert_eq!(packed.artifacts.runs.len(), 1);
    assert_eq!(
        packed.artifacts.runs[0].provenance.host.as_deref(),
        Some("fixture-host")
    );

    let unpacked = dir.join("unpacked");
    passed(&unpack_document(&out, &unpacked).report);
    let target = unpacked.join("plain.cfdcraft");
    let store = Store::open(store_dir(&target)).unwrap();
    assert_eq!(packed.artifacts.blobs.len(), 2);
    for blob in &packed.artifacts.blobs {
        store.verify(&blob.hash).unwrap();
    }
}

#[test]
fn a_pack_with_redaction_carries_every_run_and_no_host_label() {
    let (dir, path) = solved("pack-redacted");
    let before = fs::read(&path).unwrap();
    let original = open(&path).unwrap().document;
    let out = dir.join("shared.cfdpack");
    passed(&pack_document_with(&path, &out, true).report);

    let packed = open(&out).unwrap().document;
    assert_eq!(packed.artifacts.blobs, original.artifacts.blobs);
    assert_eq!(packed.artifacts.runs.len(), original.artifacts.runs.len());
    for (kept, source) in packed.artifacts.runs.iter().zip(&original.artifacts.runs) {
        assert_eq!(kept.provenance.host, None);
        // Everything but the label is as it was: keys, blobs, key numbers and checks.
        let mut expected = source.clone();
        expected.provenance.host = None;
        assert_eq!(kept, &expected);
    }

    // The study file the pack was made from still has its label, byte for byte.
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(
        original.artifacts.runs[0].provenance.host.as_deref(),
        Some("fixture-host")
    );
}

#[test]
fn unpacking_a_redacted_pack_gives_a_document_without_hosts() {
    let (dir, path) = solved("unpack-redacted");
    let out = dir.join("shared.cfdpack");
    passed(&pack_document_with(&path, &out, true).report);

    let unpacked = dir.join("unpacked");
    passed(&unpack_document(&out, &unpacked).report);
    let target = unpacked.join("shared.cfdcraft");
    let document = open(&target).unwrap().document;
    assert_eq!(document.artifacts.runs.len(), 1);
    assert!(
        document
            .artifacts
            .runs
            .iter()
            .all(|run| run.provenance.host.is_none())
    );

    let store = Store::open(store_dir(&target)).unwrap();
    for run in &document.artifacts.runs {
        for used in run.blob_uses() {
            store.verify(&used.hash).unwrap();
        }
    }
}

#[test]
fn pack_document_is_pack_document_with_false() {
    let (dir, path) = solved("pack-same");
    let (one, two) = (dir.join("one.cfdpack"), dir.join("two.cfdpack"));
    passed(&pack_document(&path, &one).report);
    passed(&pack_document_with(&path, &two, false).report);
    assert_eq!(fs::read(&one).unwrap(), fs::read(&two).unwrap());
}

#[test]
fn a_document_without_runs_packs_the_same_either_way() {
    let dir = scratch("pack-no-runs");
    let path = new_document(&dir, "cavity", CAVITY);
    let (one, two) = (dir.join("one.cfdpack"), dir.join("two.cfdpack"));
    passed(&pack_document_with(&path, &one, false).report);
    passed(&pack_document_with(&path, &two, true).report);
    assert!(open(&one).unwrap().document.artifacts.runs.is_empty());
    assert_eq!(fs::read(&one).unwrap(), fs::read(&two).unwrap());
}
