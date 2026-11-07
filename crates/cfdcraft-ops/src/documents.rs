//! The document operations behind the command line: save a study as a file, open it, link a
//! file to it, check its links, pack it with its stored results and unpack it again.
//!
//! Every operation answers with the same [`Report`] the case operations use, so a front end shows
//! the same exit class, the same findings and the same study hash whichever door it came through.
//! The report is the machine answer. The sentence a person reads on success travels beside it as
//! a [`Line`], and every finding's message is rendered from a [`Line`] by the text crate, so the
//! wording lives in one place and goes through the shared translation library.
//!
//! Finding codes of this family: 001 the file cannot be read or is not a study file; 002 written
//! by a newer version; 003 a linked file is missing; 004 a linked file changed; 005 a link cannot
//! be followed or made; 006 a stored result is missing; 007 a stored result does not match its
//! fingerprint; 008 the target already exists; 009 a file cannot be written; 010 linked files are
//! not part of a pack; 011 a pack is incomplete; 012 a pack is damaged. Errors make exit class 2;
//! warnings never change the class.
//!
//! These operations take paths, because the command line is trusted to name its own files.
//! Opening a document only looks whether results and linked files exist; it never reads a linked
//! file.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use cfdcraft_doc::{
    DocError, Document, Link, LinkError, Stamp, check_links, check_links_within, open,
    open_stamped, resolve as follow_link, save_if_unchanged, save_new, store_dir, verify,
};
use cfdcraft_model::{
    Diagnostic, MigrationNote, Severity, Study, canonical_hash, resolve as resolve_study,
};
use cfdcraft_pack::{PackError, PackOptions, pack_with, unpack};
use cfdcraft_store::{ContentHash, Store, StoreError};
use cfdcraft_text::{Line, Text};

use crate::report::Report;

/// The result of a document operation: the machine answer, and the sentence for a person when the
/// operation succeeded (to be rendered by the caller through the text crate).
#[derive(Debug, Clone)]
pub struct DocOutcome {
    pub report: Report,
    pub line: Option<Line>,
}

/// What a finding about the file as a whole is called in the report.
const FILE: &str = "study file";

/// Which way a pack operation was going, so the same error can be told apart.
#[derive(Clone, Copy)]
enum Side {
    Pack,
    Unpack,
}

/// A finding's code and the sentence data it is rendered from.
pub(crate) type Finding = (&'static str, Line);

pub(crate) fn say(line: &Line) -> String {
    Text::english().render(line)
}

pub(crate) fn shown(path: &Path) -> String {
    path.display().to_string()
}

pub(crate) fn finding(severity: Severity, setting: &str, (code, line): Finding) -> Diagnostic {
    Diagnostic::new(severity, code, setting, "", say(&line))
}

pub(crate) fn problem(found: Finding) -> Diagnostic {
    finding(Severity::Error, FILE, found)
}

pub(crate) fn refused(operation: &str, diagnostic: Diagnostic) -> DocOutcome {
    DocOutcome {
        report: Report::from_diagnostics(operation, vec![diagnostic], None),
        line: None,
    }
}

pub(crate) fn unreadable(path: &Path, reason: impl ToString) -> Finding {
    (
        "CFD-DOC-001",
        Line::NotAStudyFile {
            path: shown(path),
            reason: reason.to_string(),
        },
    )
}

pub(crate) fn not_written(path: &Path, reason: impl ToString) -> Finding {
    (
        "CFD-DOC-009",
        Line::NotWritten {
            path: shown(path),
            reason: reason.to_string(),
        },
    )
}

/// Saving a document back into a pack would drop the results the pack carries, so the operations
/// that rewrite a file refuse one, by its name or by the results found in it.
pub(crate) fn is_pack_name(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cfdpack"))
}

pub(crate) fn pack_kept(path: &Path) -> Finding {
    not_written(
        path,
        "it is a pack with its stored results, and saving would drop them; unpack it and change the unpacked study",
    )
}

pub(crate) fn taken(path: &Path) -> Finding {
    ("CFD-DOC-008", Line::TargetExists { path: shown(path) })
}

/// Why a study file could not be read.
pub(crate) fn read_failure(error: &DocError, path: &Path) -> Finding {
    match error {
        DocError::NewerSchema { found, supported } => (
            "CFD-DOC-002",
            Line::NewerSchema {
                found: *found,
                supported: *supported,
            },
        ),
        other => unreadable(path, other),
    }
}

/// Why a pack operation failed. `read` is the file being read, `write` what was being written.
fn pack_failure(error: &PackError, side: Side, read: &Path, write: &Path) -> Finding {
    let text = |hash: &ContentHash| hash.to_string();
    match (error, side) {
        (PackError::Doc(DocError::NewerSchema { found, supported }), _) => (
            "CFD-DOC-002",
            Line::NewerSchema {
                found: *found,
                supported: *supported,
            },
        ),
        (PackError::Doc(DocError::TargetExists(path)), _) => taken(path),
        (PackError::Doc(DocError::Io(e)) | PackError::Io(e), Side::Unpack)
            if e.kind() == io::ErrorKind::NotFound =>
        {
            unreadable(read, e)
        }
        (PackError::Doc(DocError::Io(e)) | PackError::Io(e), _) => not_written(write, e),
        (PackError::Doc(other), Side::Unpack) => unreadable(read, other),
        (PackError::Doc(other), Side::Pack) => not_written(write, other),
        (PackError::Missing(hash) | PackError::Store(StoreError::Missing(hash)), _) => {
            ("CFD-DOC-006", Line::PackMissing { hash: text(hash) })
        }
        (PackError::Corrupt(hash) | PackError::Store(StoreError::Corrupt(hash)), _) => {
            ("CFD-DOC-007", Line::ResultCorrupt { hash: text(hash) })
        }
        (PackError::SizeMismatch(hash), Side::Pack) => {
            ("CFD-DOC-007", Line::ResultCorrupt { hash: text(hash) })
        }
        (
            PackError::SizeMismatch(hash)
            | PackError::Damaged(hash)
            | PackError::EntryNotStored(hash),
            _,
        ) => ("CFD-DOC-012", Line::PackDamaged { hash: text(hash) }),
        (PackError::Incomplete(hash), _) => {
            ("CFD-DOC-011", Line::PackIncomplete { hash: text(hash) })
        }
        (PackError::TargetExists(path), _) => taken(path),
        (PackError::Store(other), _) => not_written(write, other),
        (PackError::OverLimit(reason), Side::Unpack) => unreadable(read, reason),
        (PackError::OverLimit(reason), Side::Pack) => not_written(write, reason),
        (PackError::Zip(reason), Side::Unpack) => unreadable(read, reason),
        (PackError::Zip(reason), Side::Pack) => not_written(write, reason),
    }
}

/// A finding about one link. A changed file is always a warning; the other findings are errors
/// when `severe` and warnings otherwise (opening a document must not fail over a link).
fn link_finding(error: &LinkError, severe: bool) -> Diagnostic {
    let missing_or_refused = if severe {
        Severity::Error
    } else {
        Severity::Warning
    };
    let refused_line = |name: &str, reason: String| {
        (
            "CFD-DOC-005",
            Line::LinkRefused {
                name: name.to_owned(),
                reason,
            },
        )
    };
    let (severity, found) = match error {
        LinkError::Missing { name, expected } => (
            missing_or_refused,
            (
                "CFD-DOC-003",
                Line::LinkMissing {
                    name: name.clone(),
                    expected: shown(expected),
                },
            ),
        ),
        LinkError::Changed { name } => (
            Severity::Warning,
            ("CFD-DOC-004", Line::LinkChanged { name: name.clone() }),
        ),
        LinkError::Refused { name, reason } => {
            (missing_or_refused, refused_line(name, reason.clone()))
        }
        LinkError::Io { name, source } => {
            (missing_or_refused, refused_line(name, source.to_string()))
        }
        // Following or checking a link produces none of the other errors.
        other => (missing_or_refused, refused_line("", other.to_string())),
    };
    finding(severity, "linked file", found)
}

pub(crate) fn study_hash(study: &Study) -> Option<String> {
    resolve_study(study)
        .ok()
        .map(|resolved| canonical_hash(&resolved))
}

/// Each distinct stored result of the document once, in index order. A result that was pruned on
/// purpose is no longer stored and is left out.
fn result_hashes(document: &Document) -> Vec<ContentHash> {
    let mut seen: HashSet<ContentHash> = document.artifacts.pruned.iter().copied().collect();
    document
        .artifacts
        .blobs
        .iter()
        .map(|blob| blob.hash)
        .filter(|hash| seen.insert(*hash))
        .collect()
}

/// One note for the whole chain of steps an older file went through, worded for a file that was
/// only read.
fn migration(notes: &[MigrationNote]) -> Option<MigrationNote> {
    migration_with(notes, false)
}

/// The same note, worded for a file that was written back at the current schema when `saved`.
pub(crate) fn migration_with(notes: &[MigrationNote], saved: bool) -> Option<MigrationNote> {
    let (first, last) = (notes.first()?, notes.last()?);
    let summary = notes
        .iter()
        .map(|note| note.summary.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let (from, to) = (first.from, last.to);
    let line = if saved {
        Line::MigratedAndSaved { from, to, summary }
    } else {
        Line::Migrated { from, to, summary }
    };
    Some(MigrationNote {
        from,
        to,
        summary: say(&line),
    })
}

/// The links the pack or unpack could not carry, as a warning.
fn links_left_out(count: usize) -> Option<Diagnostic> {
    (count > 0).then(|| {
        finding(
            Severity::Warning,
            "linked file",
            (
                "CFD-DOC-010",
                Line::LinksNotPacked {
                    count: count as u64,
                },
            ),
        )
    })
}

/// Save the study written as `study_json` to the file `out`.
///
/// An existing `out` is refused unless `replace` is set. A replace keeps the results index, the
/// links and the thumbnail of the study file it replaces and changes only the study; a target that
/// is not a study file (the study's own JSON, say) or is a pack is refused and left alone. Both
/// ways go through the document crate's crash-safe writes, so a failed save leaves the previous
/// file as it was, and a new file never replaces one that appeared meanwhile. A study that cannot
/// be read writes nothing.
pub fn save_document(study_json: &str, out: &Path, replace: bool) -> DocOutcome {
    const OP: &str = "save";
    let (study, note) = match Study::from_json_with_note(study_json) {
        Ok(read) => read,
        Err(e) => return refused(OP, Diagnostic::from_study_error(&e)),
    };
    if is_pack_name(out) {
        return refused(OP, problem(pack_kept(out)));
    }
    let exists = fs::symlink_metadata(out).is_ok();
    if exists && !replace {
        return refused(OP, problem(taken(out)));
    }
    let hash = study_hash(&study);
    let saved = if exists {
        match replacement(study, out) {
            Ok((document, stamp)) => save_if_unchanged(&document, out, stamp),
            Err(found) => return refused(OP, problem(found)),
        }
    } else {
        // Created exclusively, so a file that appears between the check above and the write is
        // not replaced either.
        save_new(&Document::new(study), out)
    };
    match saved {
        Ok(()) => {}
        Err(DocError::TargetExists(path)) => return refused(OP, problem(taken(&path))),
        Err(e) => return refused(OP, problem(not_written(out, e))),
    }
    DocOutcome {
        report: Report::from_diagnostics(OP, Vec::new(), hash)
            .with_migration(migration_with(note.as_slice(), true))
            .with_artifact("document", shown(out)),
        line: Some(Line::Saved { path: shown(out) }),
    }
}

/// The document that replaces the study file `out`: the one in it, with only the study changed,
/// and the stamp of the bytes it was read from.
fn replacement(study: Study, out: &Path) -> Result<(Document, Stamp), Finding> {
    let (opened, stamp) = open_stamped(out).map_err(|e| read_failure(&e, out))?;
    if opened.packed_entries > 0 {
        return Err(pack_kept(out));
    }
    let mut document = opened.document;
    document.study = study;
    Ok((document, stamp))
}

/// Open the study file at `path`.
///
/// Opening never fails over a missing result or a link that cannot be followed: both are warnings.
/// Only the existence of results and linked files is looked at; no linked file is read.
pub fn open_document(path: &Path) -> DocOutcome {
    open_document_in(path, None)
}

/// [`open_document`] for a server: with `confine` set, a link that leads outside that folder is
/// not followed, and the file system is never asked about what lies there.
pub fn open_document_in(path: &Path, confine: Option<&Path>) -> DocOutcome {
    const OP: &str = "open";
    let opened = match open(path) {
        Ok(opened) => opened,
        Err(e) => return refused(OP, problem(read_failure(&e, path))),
    };
    let document = &opened.document;
    let hashes = result_hashes(document);
    let store = Store::open(store_dir(path)).ok();
    let mut found = Vec::new();
    for hash in &hashes {
        if !store.as_ref().is_some_and(|store| store.contains(hash)) {
            found.push(finding(
                Severity::Warning,
                "stored result",
                (
                    "CFD-DOC-006",
                    Line::ResultMissing {
                        hash: hash.to_string(),
                    },
                ),
            ));
        }
    }
    let checks = match confine {
        Some(root) => check_links_within(path, &document.artifacts, root),
        None => check_links(path, &document.artifacts),
    };
    for check in checks {
        if let Err(e) = &check.result {
            found.push(link_finding(e, false));
        }
    }
    let mut report = Report::from_diagnostics(OP, found, study_hash(&document.study))
        .with_migration(migration(&opened.notes))
        .with_artifact("document", shown(path));
    for hash in &hashes {
        report = report.with_artifact("stored_result", hash.to_string());
    }
    for link in &document.artifacts.links {
        report = report.with_artifact("linked_file", link.path.clone());
    }
    DocOutcome {
        report,
        line: Some(Line::Opened { path: shown(path) }),
    }
}

/// The study of the file at `path` as pretty JSON with a trailing newline, or the report of why
/// the file could not be read.
// The report is the error on purpose: a failure hands back the finished, classified result.
#[allow(clippy::result_large_err)]
pub fn read_study(path: &Path) -> Result<String, Report> {
    match open(path) {
        Ok(opened) => {
            let mut text = serde_json::to_string_pretty(&opened.document.study)
                .expect("a study is plain data");
            text.push('\n');
            Ok(text)
        }
        Err(e) => Err(Report::from_diagnostics(
            "read-study",
            vec![problem(read_failure(&e, path))],
            None,
        )),
    }
}

/// Link the file `target` to the study file at `doc_path` under `id`.
///
/// The link is stored relative to the study file's folder. The file is read once, to record its
/// fingerprint. A bad or repeated id, or a target that is not a file, changes nothing, and neither
/// does a pack, which saving would strip of its results. An older file is saved back at the
/// current schema, and the report carries the migration note to say so.
pub fn link_file(doc_path: &Path, target: &Path, id: &str) -> DocOutcome {
    const OP: &str = "link";
    if is_pack_name(doc_path) {
        return refused(OP, problem(pack_kept(doc_path)));
    }
    let (opened, stamp) = match open_stamped(doc_path) {
        Ok(read) => read,
        Err(e) => return refused(OP, problem(read_failure(&e, doc_path))),
    };
    if opened.packed_entries > 0 {
        return refused(OP, problem(pack_kept(doc_path)));
    }
    // An older file is saved back at the current schema, so the report says it was saved.
    let migrated = migration_with(&opened.notes, true);
    let mut document = opened.document;
    let not_added = |e: LinkError| {
        problem((
            "CFD-DOC-005",
            Line::LinkNotAdded {
                name: shown(target),
                reason: e.to_string(),
            },
        ))
    };
    let link = match Link::to_file(doc_path, target, id) {
        Ok(link) => link,
        Err(e) => return refused(OP, not_added(e)),
    };
    let added = Line::LinkAdded {
        name: link.name.clone(),
        path: link.path.clone(),
    };
    let artifact = link.path.clone();
    if let Err(e) = document.add_link(link) {
        return refused(OP, not_added(e));
    }
    if let Err(e) = save_if_unchanged(&document, doc_path, stamp) {
        return refused(OP, problem(not_written(doc_path, e)));
    }
    DocOutcome {
        report: Report::from_diagnostics(OP, Vec::new(), study_hash(&document.study))
            .with_migration(migrated)
            .with_artifact("linked_file", artifact),
        line: Some(added),
    }
}

/// Look at every link of the study file at `doc_path`.
///
/// A missing file or a link that cannot be followed is an error. With `verify_content` each file
/// is also read and compared with the fingerprint recorded when it was linked; a changed file is
/// a warning.
pub fn check_document_links(doc_path: &Path, verify_content: bool) -> DocOutcome {
    const OP: &str = "check-links";
    let document = match open(doc_path) {
        Ok(opened) => opened.document,
        Err(e) => return refused(OP, problem(read_failure(&e, doc_path))),
    };
    let mut found = Vec::new();
    for link in &document.artifacts.links {
        let checked = if verify_content {
            verify(doc_path, link)
        } else {
            follow_link(doc_path, link).map(|_| ())
        };
        if let Err(e) = checked {
            found.push(link_finding(&e, true));
        }
    }
    let mut report = Report::from_diagnostics(OP, found, study_hash(&document.study));
    for link in &document.artifacts.links {
        report = report.with_artifact("linked_file", link.path.clone());
    }
    DocOutcome { report, line: None }
}

/// Pack the study file at `doc_path` and the results its index lists into the new file `out`.
///
/// An existing `out` is never replaced. Linked files are not carried; a warning says how many were
/// left out.
pub fn pack_document(doc_path: &Path, out: &Path) -> DocOutcome {
    pack_document_with(doc_path, out, false)
}

/// [`pack_document`], and with `redact_host` set the pack's copy of every run record has no host
/// label. The study file keeps its labels.
pub fn pack_document_with(doc_path: &Path, out: &Path, redact_host: bool) -> DocOutcome {
    const OP: &str = "pack";
    let document = match open(doc_path) {
        Ok(opened) => opened.document,
        Err(e) => return refused(OP, problem(read_failure(&e, doc_path))),
    };
    if fs::symlink_metadata(out).is_ok() {
        return refused(OP, problem(taken(out)));
    }
    // A store that is not there holds none of the results, so say so by name.
    let store = Store::open(store_dir(doc_path)).ok();
    let absent = result_hashes(&document)
        .into_iter()
        .find(|hash| !store.as_ref().is_some_and(|store| store.contains(hash)));
    if let Some(hash) = absent {
        let found = (
            "CFD-DOC-006",
            Line::PackMissing {
                hash: hash.to_string(),
            },
        );
        return refused(OP, problem(found));
    }
    let options = PackOptions { redact_host };
    let packed = match pack_with(doc_path, out, &options) {
        Ok(packed) => packed,
        Err(e) => return refused(OP, problem(pack_failure(&e, Side::Pack, doc_path, out))),
    };
    DocOutcome {
        report: Report::from_diagnostics(
            OP,
            links_left_out(packed.links_not_included.len())
                .into_iter()
                .collect(),
            study_hash(&document.study),
        )
        .with_artifact("pack", shown(out)),
        line: Some(Line::Packed {
            count: packed.blobs as u64,
            path: shown(out),
        }),
    }
}

/// Unpack `packed` into the folder `dir`: the study file and its store of results.
///
/// A study file of the same name is never replaced, and a pack that is damaged or incomplete
/// leaves no study file behind.
pub fn unpack_document(packed: &Path, dir: &Path) -> DocOutcome {
    const OP: &str = "unpack";
    let done = match unpack(packed, dir) {
        Ok(done) => done,
        Err(e) => return refused(OP, problem(pack_failure(&e, Side::Unpack, packed, dir))),
    };
    let document = match open(&done.target) {
        Ok(opened) => opened.document,
        Err(e) => return refused(OP, problem(read_failure(&e, &done.target))),
    };
    let mut report = Report::from_diagnostics(
        OP,
        links_left_out(done.links.len()).into_iter().collect(),
        study_hash(&document.study),
    )
    .with_artifact("document", shown(&done.target));
    for hash in result_hashes(&document) {
        report = report.with_artifact("stored_result", hash.to_string());
    }
    DocOutcome {
        report,
        line: Some(Line::Unpacked {
            count: done.blobs as u64,
            path: shown(dir),
        }),
    }
}
