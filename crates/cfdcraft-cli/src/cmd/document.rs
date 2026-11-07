//! The verbs that work on study files: save, open, link, check-links, pack and unpack. Each one
//! calls the matching operation and prints its outcome through [`emit_doc`].

use std::io::ErrorKind;
use std::path::Path;

use cfdcraft_model::Diagnostic;
use cfdcraft_ops::{DocOutcome, ExitClass, Report};
use cfdcraft_text::{Line, Text};

use super::{Error, emit_doc};

/// The longest link id the document format accepts.
const ID_MAX: usize = 40;

fn finished(operation: &str, found: Diagnostic) -> DocOutcome {
    DocOutcome {
        report: Report::from_diagnostics(operation, vec![found], None),
        line: None,
    }
}

/// Save a study as a `.cfdcraft` file.
pub fn save(study: &Path, out: &Path, replace: bool, json: bool) -> Result<i32, Error> {
    let outcome = match std::fs::read_to_string(study) {
        Ok(text) => cfdcraft_ops::save_document(&text, out, replace),
        Err(e) => finished(
            "save",
            cfdcraft_ops::cases::cli_problem(format!("cannot read study {}: {e}", study.display())),
        ),
    };
    Ok(emit_doc(&outcome, json))
}

/// Open a `.cfdcraft` file, and with `study_out` also write its study there as a new file.
pub fn open(file: &Path, study_out: Option<&Path>, json: bool) -> Result<i32, Error> {
    let mut outcome = cfdcraft_ops::open_document(file);
    let wanted = study_out.filter(|_| outcome.report.class() == ExitClass::Passed);
    if let Some(failed) = wanted.and_then(|out| write_study(file, out)) {
        outcome = failed;
    }
    Ok(emit_doc(&outcome, json))
}

/// Writes the study of `file` to the new file `out`. `None` when it was written; otherwise the
/// finished, classified outcome that says why not. An existing file is never touched.
fn write_study(file: &Path, out: &Path) -> Option<DocOutcome> {
    const OP: &str = "open";
    let text = match cfdcraft_ops::read_study(file) {
        Ok(text) => text,
        Err(report) => {
            return Some(DocOutcome {
                report: report.for_operation(OP),
                line: None,
            });
        }
    };
    let e = craft_atomic::write_new(out, text.as_bytes()).err()?;
    let path = out.display().to_string();
    let (code, line) = if e.kind() == ErrorKind::AlreadyExists {
        ("CFD-DOC-008", Line::TargetExists { path })
    } else {
        (
            "CFD-DOC-009",
            Line::NotWritten {
                path,
                reason: e.to_string(),
            },
        )
    };
    let message = Text::english().render(&line);
    Some(finished(
        OP,
        Diagnostic::error(code, "study file", "", message),
    ))
}

/// The id a link gets when none is given: the whole file name, lower-case, every character outside
/// a to z and 0 to 9 turned into a hyphen, leading hyphens dropped, cut to the longest id allowed.
fn default_id(target: &Path) -> String {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let hyphenated: String = name
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    hyphenated
        .trim_start_matches('-')
        .chars()
        .take(ID_MAX)
        .collect()
}

/// Record a link from the study file `doc` to `target`.
pub fn link(doc: &Path, target: &Path, id: Option<&str>, json: bool) -> Result<i32, Error> {
    let id = id.map_or_else(|| default_id(target), str::to_owned);
    if id.is_empty() {
        return Err(format!(
            "cannot make a link id from the name of {}; give one with --id",
            target.display()
        )
        .into());
    }
    Ok(emit_doc(&cfdcraft_ops::link_file(doc, target, &id), json))
}

/// Look at every link of the study file `doc`; with `verify` also compare file contents.
pub fn check_links(doc: &Path, verify: bool, json: bool) -> Result<i32, Error> {
    Ok(emit_doc(
        &cfdcraft_ops::check_document_links(doc, verify),
        json,
    ))
}

/// Pack the study file `doc` and its stored results into the new file `out`.
pub fn pack(doc: &Path, out: &Path, json: bool) -> Result<i32, Error> {
    Ok(emit_doc(&cfdcraft_ops::pack_document(doc, out), json))
}

/// Unpack `packed` into the folder `dir`.
pub fn unpack(packed: &Path, dir: &Path, json: bool) -> Result<i32, Error> {
    Ok(emit_doc(&cfdcraft_ops::unpack_document(packed, dir), json))
}
