//! Documents and packs addressed by name inside the workspace: saved, opened, packed and unpacked
//! without any path from the caller, with every path in a finding shown relative to the workspace.
//! These tests use no fake host, so they run on every platform.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use cfdcraft_doc::{BlobKind, BlobRef, Document, save, store_dir, write_members};
use cfdcraft_model::Study;
use cfdcraft_ops::{ExitClass, Report, Workspace};
use cfdcraft_store::{ContentHash, Store};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";
const POLICY: &str = include_str!("../../../DATA-POLICY.md");

fn fresh(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("workspace-documents-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fresh workspace and its folder.
fn open(tag: &str) -> (Workspace, PathBuf) {
    let dir = fresh(tag).join("ws");
    (Workspace::open(&dir).unwrap(), dir)
}

fn cavity_value() -> serde_json::Value {
    serde_json::from_str(CAVITY).unwrap()
}

fn passed(report: &Report) {
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
}

/// The report is an invalid-input answer with exactly this one finding.
fn refused(report: &Report, code: &str) {
    assert_eq!(
        report.class(),
        ExitClass::StudyInvalid,
        "{}",
        report.render_text()
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, [code], "{}", report.render_text());
}

/// A path as text with `/` separators, whatever the platform.
fn slashed(text: &str) -> String {
    text.replace('\\', "/")
}

/// Every piece of text in a report that could carry a path.
fn texts(report: &Report) -> Vec<String> {
    let mut out = Vec::new();
    for found in &report.diagnostics {
        out.push(found.message.clone());
        out.push(found.setting.clone());
        out.extend(found.hint.clone());
        out.extend(found.location.as_ref().map(|at| at.file.clone()));
    }
    out.extend(report.artifacts.iter().map(|a| a.name.clone()));
    out
}

fn result_bytes() -> Vec<u8> {
    (0..4096u32)
        .map(|i| (i.wrapping_mul(40503) >> 8) as u8)
        .collect()
}

/// The cavity as document `name` in the workspace, with one stored result beside it.
fn document_with_result(dir: &Path, name: &str) -> ContentHash {
    let path = dir.join("documents").join(format!("{name}.cfdcraft"));
    let bytes = result_bytes();
    let store = Store::create(store_dir(&path)).unwrap();
    let hash = store.put_bytes(&bytes).unwrap();
    let mut document = Document::new(Study::from_json(CAVITY).unwrap());
    document.artifacts.blobs.push(BlobRef {
        hash,
        size: bytes.len() as u64,
        kind: BlobKind::Result,
    });
    save(&document, &path).unwrap();
    hash
}

/// A pack made by hand: the study's ordinary members, then the given stored entries.
fn hand_pack(path: &Path, document: &Document, entries: &[(String, Vec<u8>)]) {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    write_members(document, &mut zip).unwrap();
    for (name, bytes) in entries {
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file(name.as_str(), stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    fs::write(path, zip.finish().unwrap().into_inner()).unwrap();
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn opening_creates_the_document_and_pack_folders_and_refuses_a_stand_in() {
    let (_, dir) = open("folders");
    for d in [
        "studies",
        "compiled",
        "exports",
        "runs",
        "documents",
        "packs",
    ] {
        assert!(dir.join(d).is_dir(), "{d}");
    }
    for d in ["documents", "packs"] {
        let bad = fresh(&format!("plain-{d}")).join("ws");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join(d), "x").unwrap();
        assert!(Workspace::open(&bad).is_err(), "{d} as a file");
    }
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_in_place_of_the_document_or_pack_folder_is_refused() {
    for d in ["documents", "packs"] {
        let base = fresh(&format!("linked-{d}"));
        let (ws, elsewhere) = (base.join("ws"), base.join("elsewhere"));
        fs::create_dir_all(&ws).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, ws.join(d)).unwrap();
        let err = Workspace::open(&ws).unwrap_err();
        assert!(err.contains("symbolic link"), "{d}: {err}");
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
    }
}

#[test]
fn a_stored_study_is_saved_as_a_document_and_opened_by_name() {
    let (ws, dir) = open("tracer");
    passed(&ws.create_study("cavity", &cavity_value()));

    let saved = ws.save_document("cavity");
    passed(&saved);
    assert_eq!(saved.operation, "save_document");
    assert!(dir.join("documents").join("cavity.cfdcraft").is_file());
    assert_eq!(saved.artifacts.len(), 1);
    assert_eq!(
        slashed(&saved.artifacts[0].name),
        "documents/cavity.cfdcraft"
    );

    let opened = ws.open_document("cavity");
    passed(&opened);
    assert_eq!(opened.operation, "open_document");
    assert_eq!(opened.study_hash.as_deref(), Some(CAVITY_HASH));
    assert!(opened.diagnostics.is_empty());
}

#[test]
fn a_second_save_and_bad_or_unknown_names_are_refused_with_codes() {
    let (ws, dir) = open("refusals");
    passed(&ws.create_study("cavity", &cavity_value()));
    passed(&ws.save_document("cavity"));
    let file = dir.join("documents").join("cavity.cfdcraft");
    let before = fs::read(&file).unwrap();

    refused(&ws.save_document("cavity"), "CFD-DOC-008");
    assert_eq!(
        fs::read(&file).unwrap(),
        before,
        "the document is unchanged"
    );

    for bad in ["../x", "", "A", "a/b", "a\\b", ".hidden"] {
        refused(&ws.save_document(bad), "CFD-WS-001");
        refused(&ws.open_document(bad), "CFD-WS-001");
        refused(&ws.pack_document(bad), "CFD-WS-001");
        refused(&ws.unpack_document(bad), "CFD-WS-001");
    }
    refused(&ws.save_document("nothing"), "CFD-WS-002");
    refused(&ws.open_document("nothing"), "CFD-DOC-001");
}

#[test]
fn findings_show_workspace_relative_paths_and_never_the_server_directory() {
    let (ws, dir) = open("relative");
    let root = dir.canonicalize().unwrap().display().to_string();
    passed(&ws.create_study("cavity", &cavity_value()));
    passed(&ws.save_document("cavity"));

    let again = ws.save_document("cavity");
    let reports = [
        ws.save_document("cavity"),
        ws.open_document("cavity"),
        ws.open_document("nothing"),
        ws.pack_document("nothing"),
        ws.pack_document("cavity"),
        ws.pack_document("cavity"),
        ws.unpack_document("nothing"),
    ];
    for report in std::iter::once(&again).chain(&reports) {
        for text in texts(report) {
            assert!(
                !text.contains(&root),
                "{} names the root: {text}",
                report.operation
            );
        }
    }
    let message = slashed(&again.diagnostics[0].message);
    assert!(message.contains("documents/cavity.cfdcraft"), "{message}");
    let missing = slashed(&reports[2].diagnostics[0].message);
    assert!(missing.contains("documents/nothing.cfdcraft"), "{missing}");
    let taken = slashed(&reports[5].diagnostics[0].message);
    assert!(taken.contains("packs/cavity.cfdpack"), "{taken}");
}

#[test]
fn a_document_is_packed_and_a_pack_the_operator_placed_is_unpacked() {
    let (ws, dir) = open("pack");
    document_with_result(&dir, "cavity");

    let packed = ws.pack_document("cavity");
    passed(&packed);
    assert_eq!(packed.operation, "pack_document");
    let pack = dir.join("packs").join("cavity.cfdpack");
    assert!(pack.is_file());
    assert_eq!(slashed(&packed.artifacts[0].name), "packs/cavity.cfdpack");
    let bytes = fs::read(&pack).unwrap();
    refused(&ws.pack_document("cavity"), "CFD-DOC-008");
    assert_eq!(fs::read(&pack).unwrap(), bytes, "the pack is unchanged");

    // The operator puts the pack in the folder under a new name; the document takes that name.
    fs::copy(&pack, dir.join("packs").join("copy.cfdpack")).unwrap();
    let unpacked = ws.unpack_document("copy");
    passed(&unpacked);
    assert_eq!(unpacked.operation, "unpack_document");
    let document = dir.join("documents").join("copy.cfdcraft");
    assert!(document.is_file());
    assert!(store_dir(&document).is_dir());

    let opened = ws.open_document("copy");
    passed(&opened);
    assert!(opened.diagnostics.is_empty(), "{}", opened.render_text());
    assert_eq!(opened.study_hash.as_deref(), Some(CAVITY_HASH));
    refused(&ws.unpack_document("copy"), "CFD-DOC-008");
}

#[test]
fn a_missing_or_damaged_pack_is_refused_and_writes_no_document() {
    let (ws, dir) = open("unpack");
    let documents = dir.join("documents");
    let before = names_in(&documents);
    refused(&ws.unpack_document("nothing"), "CFD-DOC-001");
    assert_eq!(names_in(&documents), before);

    let bytes = result_bytes();
    let hash = ContentHash::of(&bytes);
    let mut document = Document::new(Study::from_json(CAVITY).unwrap());
    document.artifacts.blobs.push(BlobRef {
        hash,
        size: bytes.len() as u64,
        kind: BlobKind::Result,
    });
    let mut wrong = bytes.clone();
    wrong[0] ^= 0xff;
    hand_pack(
        &dir.join("packs").join("damaged.cfdpack"),
        &document,
        &[(format!("blobs/{hash}"), wrong)],
    );
    refused(&ws.unpack_document("damaged"), "CFD-DOC-012");
    assert!(
        !documents.join("damaged.cfdcraft").exists(),
        "no document was written"
    );
}

#[test]
fn the_data_policy_says_the_document_commands_and_tools_use_local_files_only() {
    let line = POLICY
        .lines()
        .find(|l| l.contains("`unpack`"))
        .expect("the policy covers the document commands");
    for command in ["save", "open", "link", "check-links", "pack", "unpack"] {
        assert!(line.contains(&format!("`{command}`")), "{command}: {line}");
    }
    assert!(line.contains("local files only"), "{line}");
    assert!(line.contains("no network"), "{line}");
}
