//! The document verbs, run through the real binary: save and open a study, an older and a newer
//! file, a pack moved to another folder, and links that survive a moved project folder.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cfdcraft_doc::{BlobKind, BlobRef, Document, open, save, store_dir, write_members};
use cfdcraft_model::Study;
use cfdcraft_store::{ContentHash, Store};
use serde_json::Value;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .output()
        .unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("document-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("the binary ended by itself")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).expect("stdout is exactly one JSON document")
}

fn hash_line() -> String {
    format!("study hash: {CAVITY_HASH}")
}

fn cavity() -> Study {
    Study::from_json(CAVITY_V1).unwrap()
}

/// The cavity study as a JSON file in `dir`.
fn study_json(dir: &Path) -> PathBuf {
    let path = dir.join("cavity.json");
    fs::write(&path, CAVITY_V1).unwrap();
    path
}

/// A ZIP file built straight with the ZIP crate, entry by entry.
fn archive(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
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

#[test]
fn save_then_open_returns_the_cavity_with_the_pinned_hash() {
    let dir = scratch("tracer");
    let study = study_json(&dir);
    let doc = dir.join("out").join("cavity.cfdcraft");
    fs::create_dir_all(doc.parent().unwrap()).unwrap();

    let saved = run(&["save", s(&study), s(&doc)]);
    assert_eq!(code(&saved), 0, "{}", err(&saved));
    assert!(out(&saved).contains(&hash_line()), "{}", out(&saved));
    assert!(doc.is_file());

    let opened = run(&["open", s(&doc)]);
    assert_eq!(code(&opened), 0, "{}", err(&opened));
    assert!(out(&opened).contains(&hash_line()), "{}", out(&opened));
    assert!(err(&opened).is_empty(), "{}", err(&opened));

    let case = dir.join("case");
    let compiled = run(&["compile", s(&study), s(&case)]);
    assert_eq!(code(&compiled), 0, "{}", err(&compiled));
    assert!(out(&compiled).contains(&hash_line()));
}

#[test]
fn the_study_out_file_is_new_and_never_overwritten() {
    let dir = scratch("studyout");
    let doc = dir.join("cavity.cfdcraft");
    assert_eq!(code(&run(&["save", s(&study_json(&dir)), s(&doc)])), 0);
    let written = dir.join("s.json");

    let first = run(&["open", s(&doc), "--study-out", s(&written)]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let text = fs::read_to_string(&written).unwrap();
    assert_eq!(Study::from_json(&text).unwrap(), cavity());

    let again = run(&["open", s(&doc), "--study-out", s(&written)]);
    assert_eq!(code(&again), 2);
    assert!(err(&again).contains("already exists"), "{}", err(&again));
    assert_eq!(fs::read_to_string(written.as_path()).unwrap(), text);

    // With --json the file is still written and standard output stays one document.
    let other = dir.join("t.json");
    let as_json = run(&["open", s(&doc), "--study-out", s(&other), "--json"]);
    assert_eq!(code(&as_json), 0);
    assert_eq!(json(&as_json)["exit_class"], 0);
    assert!(other.is_file());
}

#[test]
fn saving_again_needs_replace() {
    let dir = scratch("replace");
    let study = study_json(&dir);
    let doc = dir.join("cavity.cfdcraft");
    assert_eq!(code(&run(&["save", s(&study), s(&doc)])), 0);
    let before = fs::read(&doc).unwrap();

    let refused = run(&["save", s(&study), s(&doc)]);
    assert_eq!(code(&refused), 2);
    assert!(
        err(&refused).contains("already exists"),
        "{}",
        err(&refused)
    );
    assert_eq!(fs::read(&doc).unwrap(), before);

    let replaced = run(&["save", s(&study), s(&doc), "--replace"]);
    assert_eq!(code(&replaced), 0, "{}", err(&replaced));
}

#[test]
fn an_older_file_opens_and_prints_its_migration_note() {
    let dir = scratch("older");
    let doc = dir.join("old.cfdcraft");
    fs::write(
        &doc,
        archive(&[
            ("manifest.json", manifest(0)),
            ("study.json", CAVITY_V0.as_bytes().to_vec()),
        ]),
    )
    .unwrap();

    let opened = run(&["open", s(&doc)]);
    assert_eq!(code(&opened), 0, "{}", err(&opened));
    let note = err(&opened);
    assert!(note.contains("older version"), "{note}");
    assert!(note.contains("lid") && note.contains("walls"), "{note}");
    assert!(out(&opened).contains(&hash_line()), "{}", out(&opened));
}

#[test]
fn a_newer_file_exits_2_and_json_is_the_only_output() {
    let dir = scratch("newer");
    let doc = dir.join("new.cfdcraft");
    fs::write(
        &doc,
        archive(&[
            ("manifest.json", manifest(9)),
            ("study.json", b"this is not json".to_vec()),
        ]),
    )
    .unwrap();

    let opened = run(&["open", s(&doc)]);
    assert_eq!(code(&opened), 2);
    assert!(err(&opened).contains("newer version"), "{}", err(&opened));
    assert!(out(&opened).is_empty(), "{}", out(&opened));

    let as_json = run(&["open", s(&doc), "--json"]);
    assert_eq!(code(&as_json), 2);
    let report = json(&as_json);
    assert_eq!(report["exit_class"], 2);
    assert_eq!(report["diagnostics"][0]["code"], "CFD-DOC-002");
    assert!(err(&as_json).is_empty(), "{}", err(&as_json));
}

#[test]
fn a_missing_file_and_a_plain_study_are_not_study_files() {
    let dir = scratch("notstudy");
    let missing = dir.join("missing.cfdcraft");
    let gone = run(&["open", s(&missing)]);
    assert_eq!(code(&gone), 2);
    assert!(err(&gone).contains("missing.cfdcraft"), "{}", err(&gone));

    let plain = study_json(&dir);
    let refused = run(&["open", s(&plain)]);
    assert_eq!(code(&refused), 2);
    assert!(
        err(&refused).contains("not a CFDCraft study file"),
        "{}",
        err(&refused)
    );

    let unreadable = run(&[
        "save",
        s(&dir.join("nothing.json")),
        s(&dir.join("x.cfdcraft")),
    ]);
    assert_eq!(code(&unreadable), 2);
    assert!(
        err(&unreadable).contains("nothing.json"),
        "{}",
        err(&unreadable)
    );
}

/// 2 MiB of bytes that are not all the same.
fn result_bytes() -> Vec<u8> {
    (0..2 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect()
}

/// A study file at `path` that lists a stored result, which is in the store beside it.
fn study_with_result(path: &Path, bytes: &[u8]) -> ContentHash {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let store = Store::create(store_dir(path)).unwrap();
    let hash = store.put_bytes(bytes).unwrap();
    let mut document = Document::new(cavity());
    document.artifacts.blobs.push(BlobRef {
        hash,
        size: bytes.len() as u64,
        kind: BlobKind::Result,
    });
    save(&document, path).unwrap();
    hash
}

#[test]
fn a_pack_moves_a_study_with_its_results() {
    let dir = scratch("pack");
    let bytes = result_bytes();
    let doc = dir.join("src").join("cavity.cfdcraft");
    let hash = study_with_result(&doc, &bytes);
    let packed = dir.join("out").join("cavity.cfdpack");
    fs::create_dir_all(packed.parent().unwrap()).unwrap();
    let other = dir.join("other");

    let made = run(&["pack", s(&doc), s(&packed)]);
    assert_eq!(code(&made), 0, "{}", err(&made));
    assert!(
        out(&made).contains("Packed 1 stored result into"),
        "{}",
        out(&made)
    );

    let unpacked = run(&["unpack", s(&packed), s(&other)]);
    assert_eq!(code(&unpacked), 0, "{}", err(&unpacked));
    assert!(
        out(&unpacked).contains("Unpacked 1 stored result into"),
        "{}",
        out(&unpacked)
    );

    let moved = other.join("cavity.cfdcraft");
    let opened = run(&["open", s(&moved)]);
    assert_eq!(code(&opened), 0, "{}", err(&opened));
    assert!(out(&opened).contains(&hash_line()), "{}", out(&opened));
    assert!(!err(&opened).contains("missing"), "{}", err(&opened));
    let store = Store::open(store_dir(&moved)).unwrap();
    assert_eq!(store.read(hash).unwrap(), bytes);
}

#[test]
fn packing_and_unpacking_never_overwrite() {
    let dir = scratch("packtwice");
    let doc = dir.join("cavity.cfdcraft");
    study_with_result(&doc, &result_bytes());
    let packed = dir.join("cavity.cfdpack");
    let other = dir.join("other");
    assert_eq!(code(&run(&["pack", s(&doc), s(&packed)])), 0);
    assert_eq!(code(&run(&["unpack", s(&packed), s(&other)])), 0);
    let kept = fs::read(other.join("cavity.cfdcraft")).unwrap();
    let pack_before = fs::read(&packed).unwrap();

    let again = run(&["unpack", s(&packed), s(&other)]);
    assert_eq!(code(&again), 2);
    assert!(err(&again).contains("already exists"), "{}", err(&again));
    assert_eq!(fs::read(other.join("cavity.cfdcraft")).unwrap(), kept);

    let twice = run(&["pack", s(&doc), s(&packed)]);
    assert_eq!(code(&twice), 2);
    assert_eq!(fs::read(packed.as_path()).unwrap(), pack_before);
}

#[test]
fn a_pack_with_changed_result_bytes_unpacks_to_nothing() {
    let dir = scratch("damaged");
    let bytes = result_bytes();
    let hash = ContentHash::of(&bytes);
    let mut document = Document::new(cavity());
    document.artifacts.blobs.push(BlobRef {
        hash,
        size: bytes.len() as u64,
        kind: BlobKind::Result,
    });
    let mut wrong = bytes.clone();
    wrong[0] ^= 0xff;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    write_members(&document, &mut zip).unwrap();
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file(format!("blobs/{hash}"), stored).unwrap();
    zip.write_all(&wrong).unwrap();
    let packed = dir.join("cavity.cfdpack");
    fs::write(&packed, zip.finish().unwrap().into_inner()).unwrap();
    let other = dir.join("other");

    let refused = run(&["unpack", s(&packed), s(&other)]);
    assert_eq!(code(&refused), 2);
    assert!(
        err(&refused).contains("does not match its fingerprint"),
        "{}",
        err(&refused)
    );
    assert!(!other.join("cavity.cfdcraft").exists());
}

#[test]
fn packing_a_study_whose_result_is_not_in_the_store_creates_nothing() {
    let dir = scratch("packmissing");
    let doc = dir.join("cavity.cfdcraft");
    let hash = study_with_result(&doc, &result_bytes());
    fs::remove_file(Store::open(store_dir(&doc)).unwrap().blob_path(&hash)).unwrap();
    let packed = dir.join("cavity.cfdpack");

    let refused = run(&["pack", s(&doc), s(&packed)]);
    assert_eq!(code(&refused), 2);
    assert!(
        err(&refused).contains(&hash.to_string()),
        "{}",
        err(&refused)
    );
    assert!(!packed.exists());
}

#[test]
fn a_passing_pack_reports_a_pack_artifact_as_json() {
    let dir = scratch("packjson");
    let doc = dir.join("cavity.cfdcraft");
    study_with_result(&doc, &result_bytes());
    let packed = dir.join("cavity.cfdpack");

    let made = run(&["pack", s(&doc), s(&packed), "--json"]);
    assert_eq!(code(&made), 0, "{}", err(&made));
    let report = json(&made);
    assert_eq!(report["exit_class"], 0);
    let kinds: Vec<&str> = report["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"pack"), "{kinds:?}");
    assert!(err(&made).is_empty(), "{}", err(&made));
}

/// A project folder with `studies/cavity.cfdcraft` (made with `save`) and `geometry/room.step`.
fn project(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = scratch(name);
    let proj = base.join("proj");
    fs::create_dir_all(proj.join("studies")).unwrap();
    fs::create_dir_all(proj.join("geometry")).unwrap();
    let step = proj.join("geometry").join("room.step");
    fs::write(&step, b"ISO-10303-21; pretend geometry").unwrap();
    let doc = proj.join("studies").join("cavity.cfdcraft");
    let saved = run(&["save", s(&study_json(&base)), s(&doc)]);
    assert_eq!(code(&saved), 0, "{}", err(&saved));
    (base, doc, step)
}

#[test]
fn links_survive_a_renamed_project_folder() {
    let (base, doc, step) = project("links");

    let linked = run(&["link", s(&doc), s(&step)]);
    assert_eq!(code(&linked), 0, "{}", err(&linked));
    assert!(
        out(&linked).contains("../geometry/room.step"),
        "{}",
        out(&linked)
    );
    assert_eq!(
        open(&doc).unwrap().document.artifacts.links[0].id,
        "room-step"
    );
    assert_eq!(code(&run(&["check-links", s(&doc)])), 0);

    let moved = base.join("moved");
    fs::rename(base.join("proj"), &moved).unwrap();
    let moved_doc = moved.join("studies").join("cavity.cfdcraft");
    let checked = run(&["check-links", s(&moved_doc)]);
    assert_eq!(code(&checked), 0, "{}", err(&checked));

    fs::remove_file(moved.join("geometry").join("room.step")).unwrap();
    let missing = run(&["check-links", s(&moved_doc)]);
    assert_eq!(code(&missing), 2);
    let text = err(&missing);
    let expected = moved.join("geometry").join("room.step");
    assert!(text.contains("room.step"), "{text}");
    assert!(text.contains(s(&expected)), "{text}");

    // Opening the document shows the same problem as a warning, and the class stays 0.
    let opened = run(&["open", s(&moved_doc)]);
    assert_eq!(code(&opened), 0, "{}", err(&opened));
    assert!(err(&opened).contains("warning"), "{}", err(&opened));
    assert!(err(&opened).contains("room.step"), "{}", err(&opened));

    let as_json = run(&["check-links", s(&moved_doc), "--json"]);
    assert_eq!(code(&as_json), 2);
    let report = json(&as_json);
    assert_eq!(report["exit_class"], 2);
    assert_eq!(report["diagnostics"][0]["code"], "CFD-DOC-003");
    assert!(err(&as_json).is_empty(), "{}", err(&as_json));
}

#[test]
fn a_changed_file_is_flagged_only_on_verify() {
    let (_base, doc, step) = project("changed");
    assert_eq!(code(&run(&["link", s(&doc), s(&step)])), 0);
    fs::write(step.as_path(), b"different bytes now").unwrap();

    let plain = run(&["check-links", s(&doc)]);
    assert_eq!(code(&plain), 0, "{}", err(&plain));
    assert!(!err(&plain).contains("changed"), "{}", err(&plain));

    let verified = run(&["check-links", s(&doc), "--verify"]);
    assert_eq!(code(&verified), 0, "{}", err(&verified));
    assert!(err(&verified).contains("has changed"), "{}", err(&verified));
}

#[test]
fn link_ids_are_derived_checked_and_overridable() {
    let (_base, doc, step) = project("ids");
    assert_eq!(code(&run(&["link", s(&doc), s(&step)])), 0);
    let before = fs::read(&doc).unwrap();

    let repeated = run(&["link", s(&doc), s(&step)]);
    assert_eq!(code(&repeated), 2);
    let same_id = run(&["link", s(&doc), s(&step), "--id", "room-step"]);
    assert_eq!(code(&same_id), 2);
    assert_eq!(fs::read(&doc).unwrap(), before);

    let renamed = run(&["link", s(&doc), s(&step), "--id", "room"]);
    assert_eq!(code(&renamed), 0, "{}", err(&renamed));
    let ids: Vec<String> = open(&doc)
        .unwrap()
        .document
        .artifacts
        .links
        .into_iter()
        .map(|l| l.id)
        .collect();
    assert_eq!(ids, ["room-step", "room"]);

    let odd = step.with_file_name("___.___");
    fs::write(&odd, b"x").unwrap();
    let after = fs::read(&doc).unwrap();
    let no_id = run(&["link", s(&doc), s(&odd)]);
    assert_eq!(code(&no_id), 2);
    assert!(err(&no_id).contains("--id"), "{}", err(&no_id));
    assert_eq!(fs::read(doc.as_path()).unwrap(), after);
}
