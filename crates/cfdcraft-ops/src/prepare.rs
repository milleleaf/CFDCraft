//! The gate chain. A study string goes in; a verified case tree or a classified report comes out.
//!
//! Order: read and migrate, diagnose, resolve, compile, safety scan, read-back check, and only
//! then the tree is handed to a writer. The first stage that finds an error ends the chain.

use crate::report::Report;
use cfdcraft_backend_esi::{
    CaseTree, CompileError, Dialect,
    case::{CaseOptions, compile_with},
    export::export_tree_for,
    guard::{self, Refusal},
    reader::{EsiReader, source_files},
};
use cfdcraft_model::{
    Diagnostic, Location, MigrationNote, ResolvedStudy, Study, canonical_hash, diagnose,
    has_errors, resolve, roundtrip::check_roundtrip,
};
use std::path::Path;

/// A case that passed every gate.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub resolved: ResolvedStudy,
    pub hash: String,
    /// The dictionaries only.
    pub tree: CaseTree,
    /// The dictionaries plus `Allrun` and `README.md`.
    pub export_tree: CaseTree,
    pub migration: Option<MigrationNote>,
    pub warnings: Vec<Diagnostic>,
}

/// A refused construct as a finding: the rule's code, and where it sits in the written file.
pub(crate) fn refusal_diagnostic(r: &Refusal) -> Diagnostic {
    let mut d = Diagnostic::error(
        r.rule.code(),
        "case file",
        "",
        format!("This case file cannot be used: {}.", r.message),
    );
    d.location = Some(Location {
        file: r.file.clone(),
        line: r.line as u64,
        entry: Some(r.entry.clone()),
    });
    d
}

/// A raw override the writer could not apply, as a finding at the override in the study.
fn raw_refused(label: &str, reason: &str) -> Diagnostic {
    Diagnostic::error(
        "CFD-RAW-007",
        format!("raw override {label}"),
        format!("/raw_overrides/{label}"),
        format!("The raw override \"{label}\" cannot be applied: {reason}."),
    )
    .with_hint("Check the file and the block names against the case that was written.")
}

/// A path as `/`-separated text, the form reports use.
pub(crate) fn rel_name(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

// The report is the error on purpose: a failed chain hands back the finished, classified result.
#[allow(clippy::result_large_err)]
pub fn prepare(study_json: &str, dialect: Dialect) -> Result<Prepared, Report> {
    prepare_with_options(study_json, dialect, &CaseOptions::default())
}

/// `prepare` for a case written for the number of processes in `options`. One process gives
/// exactly the trees `prepare` gives; more add the decomposition and a parallel `Allrun`, and the
/// case goes through the same gates.
// The report is the error on purpose: a failed chain hands back the finished, classified result.
#[allow(clippy::result_large_err)]
pub fn prepare_with_options(
    study_json: &str,
    dialect: Dialect,
    options: &CaseOptions,
) -> Result<Prepared, Report> {
    prepare_inner(study_json, dialect, options, |_| {})
}

/// `prepare`, with a closure that edits the compiled tree before the safety scan. It exists so
/// tests can simulate a writer fault; no front end calls it.
#[doc(hidden)]
#[allow(clippy::result_large_err)]
pub fn prepare_with_tamper(
    study_json: &str,
    dialect: Dialect,
    tamper: impl FnOnce(&mut CaseTree),
) -> Result<Prepared, Report> {
    prepare_inner(study_json, dialect, &CaseOptions::default(), tamper)
}

#[allow(clippy::result_large_err)]
fn prepare_inner(
    study_json: &str,
    dialect: Dialect,
    options: &CaseOptions,
    tamper: impl FnOnce(&mut CaseTree),
) -> Result<Prepared, Report> {
    let fail = |found: Vec<Diagnostic>, hash: Option<String>, note: Option<MigrationNote>| {
        Report::from_diagnostics("prepare", found, hash).with_migration(note)
    };
    let (study, migration) = Study::from_json_with_note(study_json)
        .map_err(|e| fail(vec![Diagnostic::from_study_error(&e)], None, None))?;
    let found = diagnose(&study, &dialect.capabilities());
    if has_errors(&found) {
        return Err(fail(found, None, migration));
    }
    let model_refused = |message: String| {
        Diagnostic::error("CFD-MODEL-003", "study", "", message).with_hint(
            "The checks accepted this study but the compiler refused it. Please report it.",
        )
    };
    let resolved = resolve(&study)
        .map_err(|e| fail(vec![model_refused(e.to_string())], None, migration.clone()))?;
    let hash = canonical_hash(&resolved);
    let mut tree = compile_with(&resolved, dialect, options).map_err(|e| {
        let finding = match e {
            CompileError::Raw { label, reason } => raw_refused(&label, &reason),
            other => model_refused(other.to_string()),
        };
        fail(vec![finding], Some(hash.clone()), migration.clone())
    })?;
    tamper(&mut tree);
    let refusals = guard::scan_tree(&tree);
    if !refusals.is_empty() {
        let found = refusals.iter().map(refusal_diagnostic).collect();
        return Err(fail(found, Some(hash), migration));
    }
    let unread = check_roundtrip(&EsiReader, &source_files(&tree), &resolved);
    if has_errors(&unread) {
        return Err(fail(unread, Some(hash), migration));
    }
    let export_tree = export_tree_for(&tree, &hash, options.np);
    Ok(Prepared {
        resolved,
        hash,
        tree,
        export_tree,
        migration,
        warnings: found,
    })
}
