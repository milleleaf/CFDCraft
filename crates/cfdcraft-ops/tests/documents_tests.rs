//! The document operations answer with the same report as the case operations: exit class, findings
//! with stable codes, the study hash and the migration note. These tests use no fake host, so they
//! run on every platform.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use cfdcraft_doc::{BlobKind, BlobRef, Document, Link, open, save, store_dir, write_members};
use cfdcraft_model::{Diagnostic, Severity, Study};
use cfdcraft_ops::{
    ExitClass, Report, check_document_links, link_file, open_document, pack_document, read_study,
    save_document, unpack_document,
};
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_text::{Line, Text};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";

fn cavity() -> Study {
    Study::from_json(CAVITY_V1).unwrap()
}

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("documents-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn shown(path: &Path) -> String {
    path.display().to_string()
}

fn say(line: &Line) -> String {
    Text::english().render(line)
}

fn passed(report: &Report) {
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
}

fn invalid(report: &Report) {
    assert_eq!(
        report.class(),
        ExitClass::StudyInvalid,
        "{}",
        report.render_text()
    );
}

/// The one finding of a report, checked to have the given code.
fn sole<'a>(report: &'a Report, code: &str) -> &'a Diagnostic {
    assert_eq!(report.diagnostics.len(), 1, "{}", report.render_text());
    let found = &report.diagnostics[0];
    assert_eq!(found.code, code, "{}", report.render_text());
    found
}

fn codes(report: &Report) -> Vec<&str> {
    report.diagnostics.iter().map(|d| d.code.as_str()).collect()
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

/// A study file at `path` that lists one stored result per entry of `results`, each of them in the
/// store beside it. Returns the hashes.
fn study_file(path: &Path, results: &[&[u8]]) -> Vec<ContentHash> {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut document = Document::new(cavity());
    let mut hashes = Vec::new();
    if !results.is_empty() {
        let store = Store::create(store_dir(path)).unwrap();
        for bytes in results {
            let hash = store.put_bytes(bytes).unwrap();
            document.artifacts.blobs.push(BlobRef {
                hash,
                size: bytes.len() as u64,
                kind: BlobKind::Result,
            });
            hashes.push(hash);
        }
    }
    save(&document, path).unwrap();
    hashes
}

fn result_bytes() -> Vec<u8> {
    (0..4096u32)
        .map(|i| (i.wrapping_mul(40503) >> 8) as u8)
        .collect()
}

#[test]
fn the_cavity_is_saved_and_opened_again_through_the_product_report() {
    let out = fresh("tracer").join("cavity.cfdcraft");

    let saved = save_document(CAVITY_V1, &out, false);
    passed(&saved.report);
    assert_eq!(saved.line, Some(Line::Saved { path: shown(&out) }));
    assert_eq!(saved.report.study_hash.as_deref(), Some(CAVITY_HASH));

    let opened = open_document(&out);
    passed(&opened.report);
    assert!(opened.report.diagnostics.is_empty());
    assert_eq!(opened.report.study_hash.as_deref(), Some(CAVITY_HASH));
    assert_eq!(opened.report.migration, None);
    assert_eq!(opened.report.artifacts.len(), 1);
    assert_eq!(opened.report.artifacts[0].kind, "document");
    assert_eq!(opened.line, Some(Line::Opened { path: shown(&out) }));
}

#[test]
fn an_older_file_opens_with_its_migration_note() {
    let dir = fresh("older");
    let path = dir.join("old.cfdcraft");
    fs::write(
        &path,
        archive(&[
            ("manifest.json", manifest(0)),
            ("study.json", CAVITY_V0.as_bytes().to_vec()),
        ]),
    )
    .unwrap();

    let opened = open_document(&path);
    passed(&opened.report);
    assert_eq!(opened.report.study_hash.as_deref(), Some(CAVITY_HASH));
    let note = opened.report.migration.as_ref().unwrap();
    assert_eq!((note.from, note.to), (0, 3));
    // The report folds the whole chain into one note: the summaries of the steps, one space apart.
    let steps = open(&path).unwrap().notes;
    assert_eq!(steps.len(), 3);
    let joined = steps
        .iter()
        .map(|n| n.summary.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        note.summary,
        say(&Line::Migrated {
            from: 0,
            to: 3,
            summary: joined
        })
    );
    assert!(note.summary.contains("older version"));
    assert!(note.summary.contains("\"lid\"") && note.summary.contains("\"walls\""));
    // Opening never rewrites the file.
    assert_eq!(open(&path).unwrap().schema_version, 0);
}

#[test]
fn a_newer_file_is_refused_from_the_manifest_alone() {
    let path = fresh("newer").join("new.cfdcraft");
    fs::write(
        &path,
        archive(&[
            ("manifest.json", manifest(9)),
            ("study.json", b"this is not json".to_vec()),
        ]),
    )
    .unwrap();

    let opened = open_document(&path);
    invalid(&opened.report);
    let found = sole(&opened.report, "CFD-DOC-002");
    assert!(found.message.contains("newer version"));
    assert_eq!(
        found.message,
        say(&Line::NewerSchema {
            found: 9,
            supported: 3
        })
    );
    assert_eq!(opened.line, None);
    invalid(&read_study(&path).unwrap_err());
}

#[test]
fn files_that_are_not_study_files_say_so_and_name_the_path() {
    let dir = fresh("notstudy");
    let junk = dir.join("junk.cfdcraft");
    fs::write(&junk, b"this is not a zip file at all").unwrap();
    let folder = dir.join("folder.cfdcraft");
    fs::create_dir_all(&folder).unwrap();
    let missing = dir.join("missing.cfdcraft");

    for path in [&junk, &folder, &missing] {
        let opened = open_document(path);
        invalid(&opened.report);
        let found = sole(&opened.report, "CFD-DOC-001");
        assert!(
            found
                .message
                .starts_with(&format!("{} is not a CFDCraft study file:", shown(path))),
            "{}",
            found.message
        );
        invalid(&read_study(path).unwrap_err());
    }
}

#[test]
fn saving_never_overwrites_unless_asked_and_a_bad_study_writes_nothing() {
    let dir = fresh("save");
    let out = dir.join("a.cfdcraft");
    fs::write(&out, b"keep these bytes").unwrap();

    let refused = save_document(CAVITY_V1, &out, false);
    invalid(&refused.report);
    let found = sole(&refused.report, "CFD-DOC-008");
    assert_eq!(
        found.message,
        say(&Line::TargetExists { path: shown(&out) })
    );
    assert_eq!(fs::read(&out).unwrap(), b"keep these bytes");

    // A replace only ever replaces a study file: these bytes are not one, so they stay.
    let not_a_study = save_document(CAVITY_V1, &out, true);
    invalid(&not_a_study.report);
    sole(&not_a_study.report, "CFD-DOC-001");
    assert_eq!(fs::read(&out).unwrap(), b"keep these bytes");

    let existing = dir.join("b.cfdcraft");
    passed(&save_document(CAVITY_V1, &existing, false).report);
    let replaced = save_document(CAVITY_V1, &existing, true);
    passed(&replaced.report);
    passed(&open_document(&existing).report);

    let broken = dir.join("broken.cfdcraft");
    let bad = save_document("{ not a study", &broken, true);
    invalid(&bad.report);
    assert_eq!(codes(&bad.report), ["CFD-STUDY-001"]);
    assert!(!broken.exists());

    // A file where a folder is needed cannot be written; the previous contents are untouched.
    let under_a_file = out.join("child.cfdcraft");
    let unwritable = save_document(CAVITY_V1, &under_a_file, true);
    invalid(&unwritable.report);
    let found = sole(&unwritable.report, "CFD-DOC-009");
    assert!(
        found
            .message
            .starts_with(&format!("{} could not be written: ", shown(&under_a_file))),
        "{}",
        found.message
    );
}

#[test]
fn a_missing_stored_result_is_a_warning_and_a_present_one_is_not() {
    let path = fresh("results").join("a.cfdcraft");
    let bytes = result_bytes();
    let hashes = study_file(&path, &[&bytes]);
    let hash = hashes[0];
    fs::remove_file(Store::open(store_dir(&path)).unwrap().blob_path(&hash)).unwrap();

    let opened = open_document(&path);
    passed(&opened.report);
    let found = sole(&opened.report, "CFD-DOC-006");
    assert_eq!(found.severity, Severity::Warning);
    assert_eq!(
        found.message,
        say(&Line::ResultMissing {
            hash: hash.to_string()
        })
    );
    assert!(found.message.contains(&hash.to_string()));
    assert_eq!(opened.line, Some(Line::Opened { path: shown(&path) }));

    Store::open(store_dir(&path))
        .unwrap()
        .put_bytes(&bytes)
        .unwrap();
    let healed = open_document(&path);
    passed(&healed.report);
    assert!(healed.report.diagnostics.is_empty());
    assert_eq!(
        healed
            .report
            .artifacts
            .iter()
            .map(|a| a.kind.as_str())
            .collect::<Vec<_>>(),
        ["document", "stored_result"]
    );
}

#[test]
fn read_study_returns_the_study_as_pretty_json() {
    let path = fresh("read").join("a.cfdcraft");
    passed(&save_document(CAVITY_V1, &path, false).report);

    let text = read_study(&path).unwrap();
    assert!(text.ends_with('\n'));
    assert_eq!(Study::from_json(&text).unwrap(), cavity());
    assert_eq!(
        text,
        serde_json::to_string_pretty(&cavity()).unwrap() + "\n"
    );
}

/// A project folder with `studies/cavity.cfdcraft` (saved) and `geometry/room.step`.
#[test]
fn replacing_keeps_the_results_links_and_thumbnail_and_never_overwrites_the_input() {
    let dir = fresh("replace-keeps");
    let out = dir.join("a.cfdcraft");
    let hashes = study_file(&out, &[&result_bytes()]);
    let mut kept = open(&out).unwrap().document;
    kept.artifacts.links.push(Link {
        id: "room".to_owned(),
        path: "../geometry/room.step".to_owned(),
        name: "room.step".to_owned(),
        hash: ContentHash::of(b"room"),
    });
    kept.thumbnail = Some(b"a picture of this study".to_vec());
    save(&kept, &out).unwrap();

    passed(&save_document(CAVITY_V1, &out, true).report);
    let after = open(&out).unwrap().document;
    assert_eq!(after.artifacts, kept.artifacts);
    assert_eq!(after.artifacts.blobs[0].hash, hashes[0]);
    assert_eq!(after.thumbnail, kept.thumbnail);
    assert_eq!(after.study, cavity());

    // The study's own JSON is not a study file, so saving onto it is refused and it is untouched.
    let input = dir.join("study.json");
    fs::write(&input, CAVITY_V1).unwrap();
    let refused = save_document(CAVITY_V1, &input, true);
    invalid(&refused.report);
    sole(&refused.report, "CFD-DOC-001");
    assert_eq!(fs::read_to_string(&input).unwrap(), CAVITY_V1);
}

#[test]
fn link_and_save_refuse_a_pack_and_leave_its_results_in_it() {
    let dir = fresh("pack-kept");
    let doc = dir.join("a.cfdcraft");
    study_file(&doc, &[&result_bytes()]);
    let packed = dir.join("a.cfdpack");
    passed(&pack_document(&doc, &packed).report);
    let step = dir.join("room.step");
    fs::write(&step, b"pretend geometry").unwrap();
    let before = fs::read(&packed).unwrap();

    let by_name = link_file(&packed, &step, "room");
    invalid(&by_name.report);
    sole(&by_name.report, "CFD-DOC-009");
    assert_eq!(fs::read(&packed).unwrap(), before);

    // Renaming the pack does not hide what is in it.
    let renamed = dir.join("renamed.cfdcraft");
    fs::copy(&packed, &renamed).unwrap();
    invalid(&link_file(&renamed, &step, "room").report);
    let replaced = save_document(CAVITY_V1, &renamed, true);
    invalid(&replaced.report);
    sole(&replaced.report, "CFD-DOC-009");
    assert_eq!(fs::read(&renamed).unwrap(), before);

    let named_pack = dir.join("new.cfdpack");
    invalid(&save_document(CAVITY_V1, &named_pack, false).report);
    assert!(!named_pack.exists());
}

/// A schema 0 file in `dir` called `old.cfdcraft`.
fn old_file(dir: &Path) -> PathBuf {
    let path = dir.join("old.cfdcraft");
    fs::write(
        &path,
        archive(&[
            ("manifest.json", manifest(0)),
            ("study.json", CAVITY_V0.as_bytes().to_vec()),
        ]),
    )
    .unwrap();
    path
}

/// The summaries of the steps joined by a space, the way the report folds a chain of notes.
fn joined(notes: &[cfdcraft_model::MigrationNote]) -> String {
    notes
        .iter()
        .map(|n| n.summary.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn linking_an_older_file_says_it_was_saved() {
    let dir = fresh("link-older");
    let path = old_file(&dir);
    let step = dir.join("room.step");
    fs::write(&step, b"pretend geometry").unwrap();
    let steps = open(&path).unwrap().notes;

    let linked = link_file(&path, &step, "room");
    passed(&linked.report);
    let note = linked.report.migration.as_ref().unwrap();
    assert_eq!((note.from, note.to), (0, 3));
    assert_eq!(
        note.summary,
        say(&Line::MigratedAndSaved {
            from: steps.first().unwrap().from,
            to: steps.last().unwrap().to,
            summary: joined(&steps),
        })
    );
    assert!(note.summary.contains("saved"), "{}", note.summary);
    assert!(!note.summary.contains("unchanged"), "{}", note.summary);
    // The saved file carries the number a freshly made file carries.
    let current = dir.join("current.cfdcraft");
    save(&Document::new(cavity()), &current).unwrap();
    assert_eq!(
        open(&path).unwrap().schema_version,
        open(&current).unwrap().schema_version
    );
}

#[test]
fn opening_an_older_file_still_says_the_file_is_unchanged() {
    let dir = fresh("open-older-unchanged");
    let path = old_file(&dir);
    let steps = open(&path).unwrap().notes;

    let opened = open_document(&path);
    passed(&opened.report);
    let note = opened.report.migration.as_ref().unwrap();
    assert_eq!(
        note.summary,
        say(&Line::Migrated {
            from: steps.first().unwrap().from,
            to: steps.last().unwrap().to,
            summary: joined(&steps),
        })
    );
    assert!(note.summary.contains("unchanged"), "{}", note.summary);
    assert_eq!(open(&path).unwrap().schema_version, 0);
}

#[test]
fn saving_an_older_study_says_it_was_saved() {
    let dir = fresh("save-older");
    let out = dir.join("saved.cfdcraft");
    let (_, note) = Study::from_json_with_note(CAVITY_V0).unwrap();
    let steps: Vec<_> = note.into_iter().collect();
    assert_eq!((steps[0].from, steps[0].to), (0, 1));

    let saved = save_document(CAVITY_V0, &out, false);
    passed(&saved.report);
    let reported = saved.report.migration.as_ref().unwrap();
    assert_eq!((reported.from, reported.to), (0, 1));
    assert_eq!(
        reported.summary,
        say(&Line::MigratedAndSaved {
            from: 0,
            to: 1,
            summary: joined(&steps),
        })
    );
    assert!(
        !reported.summary.contains("unchanged"),
        "{}",
        reported.summary
    );
}

fn project(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = fresh(name);
    let proj = base.join("proj");
    fs::create_dir_all(proj.join("studies")).unwrap();
    fs::create_dir_all(proj.join("geometry")).unwrap();
    let step = proj.join("geometry").join("room.step");
    fs::write(&step, b"ISO-10303-21; pretend geometry").unwrap();
    let doc = proj.join("studies").join("cavity.cfdcraft");
    passed(&save_document(CAVITY_V1, &doc, false).report);
    (base, doc, step)
}

#[test]
fn a_link_survives_moving_the_folder_and_a_missing_file_names_where_it_was_expected() {
    let (base, doc, step) = project("linkmove");

    let linked = link_file(&doc, &step, "geometry-1");
    passed(&linked.report);
    assert_eq!(
        linked.line,
        Some(Line::LinkAdded {
            name: "room.step".to_owned(),
            path: "../geometry/room.step".to_owned()
        })
    );
    assert_eq!(linked.report.study_hash.as_deref(), Some(CAVITY_HASH));
    passed(&check_document_links(&doc, false).report);
    passed(&check_document_links(&doc, true).report);

    let moved = base.join("moved");
    fs::create_dir_all(&moved).unwrap();
    fs::rename(base.join("proj"), moved.join("proj")).unwrap();
    let doc = moved.join("proj").join("studies").join("cavity.cfdcraft");
    passed(&check_document_links(&doc, false).report);
    passed(&open_document(&doc).report);

    fs::remove_file(moved.join("proj").join("geometry").join("room.step")).unwrap();
    let checked = check_document_links(&doc, false);
    invalid(&checked.report);
    let found = sole(&checked.report, "CFD-DOC-003");
    assert!(
        found
            .message
            .starts_with("The linked file room.step is missing. It was expected at "),
        "{}",
        found.message
    );
    assert!(found.message.contains("moved"), "{}", found.message);

    // Opening reports the same thing as a warning only.
    let opened = open_document(&doc);
    passed(&opened.report);
    let found = sole(&opened.report, "CFD-DOC-003");
    assert_eq!(found.severity, Severity::Warning);
}

#[test]
fn a_changed_file_is_a_warning_only_when_asked_to_look_and_a_web_address_is_refused() {
    let (_base, doc, step) = project("linkchange");
    passed(&link_file(&doc, &step, "room").report);
    fs::write(step, b"different content").unwrap();

    let quick = check_document_links(&doc, false);
    passed(&quick.report);
    assert!(quick.report.diagnostics.is_empty());

    let careful = check_document_links(&doc, true);
    passed(&careful.report);
    let found = sole(&careful.report, "CFD-DOC-004");
    assert_eq!(found.severity, Severity::Warning);
    assert_eq!(
        found.message,
        say(&Line::LinkChanged {
            name: "room.step".to_owned()
        })
    );

    // A document edited by hand so that a link points at a web address.
    let mut document = open(&doc).unwrap().document;
    document.artifacts.links.push(Link {
        id: "web".to_owned(),
        path: "https://example.com/other.step".to_owned(),
        name: "other.step".to_owned(),
        hash: ContentHash::of(b"x"),
    });
    save(&document, &doc).unwrap();
    for verify in [false, true] {
        let checked = check_document_links(&doc, verify);
        invalid(&checked.report);
        let refused = checked
            .report
            .diagnostics
            .iter()
            .find(|d| d.code == "CFD-DOC-005")
            .expect("the web address is refused");
        assert_eq!(refused.severity, Severity::Error);
        assert!(
            refused
                .message
                .starts_with("The link to other.step cannot be followed: "),
            "{}",
            refused.message
        );
    }
}

#[test]
fn a_link_that_cannot_be_made_changes_nothing() {
    let (base, doc, step) = project("linkrefuse");
    passed(&link_file(&doc, &step, "room").report);
    let before = fs::read(&doc).unwrap();

    let repeated = link_file(&doc, &step, "room");
    invalid(&repeated.report);
    let found = sole(&repeated.report, "CFD-DOC-005");
    assert!(
        found.message.contains("already exists"),
        "{}",
        found.message
    );
    assert_eq!(repeated.line, None);
    assert_eq!(fs::read(&doc).unwrap(), before);

    let nothing = link_file(
        &doc,
        &base.join("proj").join("geometry").join("none.step"),
        "x",
    );
    invalid(&nothing.report);
    sole(&nothing.report, "CFD-DOC-005");
    assert_eq!(fs::read(&doc).unwrap(), before);

    let bad_id = link_file(&doc, &step, "Not Valid");
    invalid(&bad_id.report);
    sole(&bad_id.report, "CFD-DOC-005");
    assert_eq!(fs::read(&doc).unwrap(), before);
}

#[test]
fn a_study_packs_and_unpacks_and_nothing_is_overwritten() {
    let dir = fresh("roundtrip");
    let doc = dir.join("src").join("cavity.cfdcraft");
    let bytes = result_bytes();
    study_file(&doc, &[&bytes]);
    let out = dir.join("cavity.cfdpack");

    let packed = pack_document(&doc, &out);
    passed(&packed.report);
    assert!(packed.report.diagnostics.is_empty());
    assert_eq!(packed.report.study_hash.as_deref(), Some(CAVITY_HASH));
    assert_eq!(
        packed.line,
        Some(Line::Packed {
            count: 1,
            path: shown(&out)
        })
    );
    assert!(
        packed
            .report
            .artifacts
            .iter()
            .any(|a| a.kind == "pack" && a.name == shown(&out))
    );

    let target = dir.join("elsewhere");
    let unpacked = unpack_document(&out, &target);
    passed(&unpacked.report);
    assert_eq!(
        unpacked.line,
        Some(Line::Unpacked {
            count: 1,
            path: shown(&target)
        })
    );
    assert_eq!(unpacked.report.study_hash.as_deref(), Some(CAVITY_HASH));
    let kinds: Vec<&str> = unpacked
        .report
        .artifacts
        .iter()
        .map(|a| a.kind.as_str())
        .collect();
    assert_eq!(kinds, ["document", "stored_result"]);

    let reopened = open_document(&target.join("cavity.cfdcraft"));
    passed(&reopened.report);
    assert!(reopened.report.diagnostics.is_empty());

    // Packing to the same output, and unpacking into the same folder, are refused.
    let before = fs::read(&out).unwrap();
    let again = pack_document(&doc, &out);
    invalid(&again.report);
    let found = sole(&again.report, "CFD-DOC-008");
    assert_eq!(
        found.message,
        say(&Line::TargetExists { path: shown(&out) })
    );
    assert_eq!(fs::read(&out).unwrap(), before);

    let twice = unpack_document(&out, &target);
    invalid(&twice.report);
    let found = sole(&twice.report, "CFD-DOC-008");
    assert_eq!(
        found.message,
        say(&Line::TargetExists {
            path: shown(&target.join("cavity.cfdcraft"))
        })
    );
}

#[test]
fn packing_refuses_a_missing_or_damaged_result_and_writes_nothing() {
    let bytes = result_bytes();

    // The result is gone from the store.
    let dir = fresh("packmissing");
    let doc = dir.join("a.cfdcraft");
    let hash = study_file(&doc, &[&bytes])[0];
    fs::remove_file(Store::open(store_dir(&doc)).unwrap().blob_path(&hash)).unwrap();
    let out = dir.join("a.cfdpack");
    let missing = pack_document(&doc, &out);
    invalid(&missing.report);
    let found = sole(&missing.report, "CFD-DOC-006");
    assert_eq!(
        found.message,
        say(&Line::PackMissing {
            hash: hash.to_string()
        })
    );
    assert!(!out.exists());

    // There is no store folder at all.
    fs::remove_dir_all(store_dir(&doc)).unwrap();
    sole(&pack_document(&doc, &out).report, "CFD-DOC-006");
    assert!(!out.exists());

    // The result is there but its bytes are wrong.
    let dir = fresh("packdamaged");
    let doc = dir.join("a.cfdcraft");
    let hash = study_file(&doc, &[&bytes])[0];
    let blob = Store::open(store_dir(&doc)).unwrap().blob_path(&hash);
    let mut wrong = fs::read(&blob).unwrap();
    wrong[0] ^= 0xff;
    fs::write(blob, wrong).unwrap();
    let out = dir.join("a.cfdpack");
    let damaged = pack_document(&doc, &out);
    invalid(&damaged.report);
    let found = sole(&damaged.report, "CFD-DOC-007");
    assert_eq!(
        found.message,
        say(&Line::ResultCorrupt {
            hash: hash.to_string()
        })
    );
    assert!(!out.exists());
}

#[test]
fn packing_a_study_with_links_warns_that_they_are_left_out() {
    let (base, doc, step) = project("packlinks");
    passed(&link_file(&doc, &step, "room").report);
    let out = base.join("cavity.cfdpack");

    let packed = pack_document(&doc, &out);
    passed(&packed.report);
    let found = sole(&packed.report, "CFD-DOC-010");
    assert_eq!(found.severity, Severity::Warning);
    assert_eq!(found.message, say(&Line::LinksNotPacked { count: 1 }));
    assert!(out.exists());
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

#[test]
fn unpacking_a_damaged_or_incomplete_pack_leaves_no_study() {
    let bytes = result_bytes();
    let hash = ContentHash::of(&bytes);
    let mut document = Document::new(cavity());
    document.artifacts.blobs.push(BlobRef {
        hash,
        size: bytes.len() as u64,
        kind: BlobKind::Result,
    });
    let entry = format!("blobs/{hash}");

    // One result whose bytes are not the ones the index names, of the same length.
    let dir = fresh("unpackdamaged");
    let packed = dir.join("x.cfdpack");
    let mut wrong = bytes.clone();
    wrong[0] ^= 0xff;
    hand_pack(&packed, &document, &[(entry, wrong)]);
    let target = dir.join("out");
    let damaged = unpack_document(&packed, &target);
    invalid(&damaged.report);
    let found = sole(&damaged.report, "CFD-DOC-012");
    assert_eq!(
        found.message,
        say(&Line::PackDamaged {
            hash: hash.to_string()
        })
    );
    assert!(!target.join("x.cfdcraft").exists());

    // The entry for the result is missing altogether.
    let dir = fresh("unpackincomplete");
    let packed = dir.join("x.cfdpack");
    hand_pack(&packed, &document, &[]);
    let target = dir.join("out");
    let incomplete = unpack_document(&packed, &target);
    invalid(&incomplete.report);
    let found = sole(&incomplete.report, "CFD-DOC-011");
    assert_eq!(
        found.message,
        say(&Line::PackIncomplete {
            hash: hash.to_string()
        })
    );
    assert!(!target.join("x.cfdcraft").exists());

    // Something that is not a pack, and a pack that does not exist.
    let junk = dir.join("junk.cfdpack");
    fs::write(&junk, b"not a zip").unwrap();
    for path in [junk, dir.join("nothing.cfdpack")] {
        let refused = unpack_document(&path, &target);
        invalid(&refused.report);
        let found = sole(&refused.report, "CFD-DOC-001");
        assert!(found.message.contains(&shown(&path)), "{}", found.message);
    }
}

#[test]
fn a_pack_without_stored_results_still_round_trips() {
    let dir = fresh("emptypack");
    let doc = dir.join("a.cfdcraft");
    passed(&save_document(CAVITY_V1, &doc, false).report);
    let out = dir.join("a.cfdpack");
    let packed = pack_document(&doc, &out);
    passed(&packed.report);
    assert_eq!(
        packed.line,
        Some(Line::Packed {
            count: 0,
            path: shown(&out)
        })
    );
    let unpacked = unpack_document(&out, &dir.join("out"));
    passed(&unpacked.report);
    assert_eq!(unpacked.report.study_hash.as_deref(), Some(CAVITY_HASH));
}
