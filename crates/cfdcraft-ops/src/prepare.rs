//! The gate chain. A study string goes in; a verified case tree or a classified report comes out.
//!
//! Order: read and migrate, diagnose, resolve, compile, safety scan, read-back check, and only
//! then the tree is handed to a writer. The first stage that finds an error ends the chain. The
//! chain runs through the [`Backend`] trait, so every backend passes the same gates in the same
//! order.

use crate::backends::{Row, resolve, row_warnings};
use crate::report::Report;
use cfdcraft_backend::{Backend, CaseTree, CompileFailure, CompileOptions};
use cfdcraft_backend_esi::{Dialect, case::CaseOptions, guard};
use cfdcraft_model::{
    Diagnostic, Location, MigrationNote, ResolvedStudy, Study, canonical_hash, diagnose,
    has_errors, resolve as resolve_study,
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

/// A construct the safety scan of any backend refused, as a finding: the rule's code, and where
/// it sits in the written file.
pub(crate) fn backend_refusal_diagnostic(r: &cfdcraft_backend::Refusal) -> Diagnostic {
    let mut d = Diagnostic::error(
        r.code,
        "case file",
        "",
        format!("This case file cannot be used: {}.", r.message),
    );
    d.location = Some(Location {
        file: r.file.clone(),
        line: u64::from(r.line),
        entry: Some(r.entry.clone()),
    });
    d
}

/// A refused construct of the OpenCFD scan as a finding.
pub(crate) fn refusal_diagnostic(r: &guard::Refusal) -> Diagnostic {
    backend_refusal_diagnostic(&cfdcraft_backend::Refusal {
        code: r.rule.code(),
        file: r.file.clone(),
        line: u32::try_from(r.line).unwrap_or(u32::MAX),
        entry: r.entry.clone(),
        message: r.message.clone(),
    })
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
#[doc(hidden)]
#[allow(clippy::result_large_err)]
pub fn prepare(study_json: &str, dialect: Dialect) -> Result<Prepared, Report> {
    prepare_with_options(study_json, dialect, &CaseOptions::default())
}

/// `prepare` for a case written for the number of processes in `options`. One process gives
/// exactly the trees `prepare` gives; more add the decomposition and a parallel `Allrun`, and the
/// case goes through the same gates.
// The report is the error on purpose: a failed chain hands back the finished, classified result.
#[doc(hidden)]
#[allow(clippy::result_large_err)]
pub fn prepare_with_options(
    study_json: &str,
    dialect: Dialect,
    options: &CaseOptions,
) -> Result<Prepared, Report> {
    prepare_dialect(study_json, dialect, options, |_| {})
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
    prepare_dialect(study_json, dialect, &CaseOptions::default(), tamper)
}

/// The dialect's registry row, then the chain.
// The report is the error on purpose: a failed chain hands back the finished, classified result.
#[allow(clippy::result_large_err)]
fn prepare_dialect(
    study_json: &str,
    dialect: Dialect,
    options: &CaseOptions,
    tamper: impl FnOnce(&mut CaseTree),
) -> Result<Prepared, Report> {
    match resolve(dialect.id()) {
        Ok(row) => chain(
            study_json,
            row.backend,
            &CompileOptions { np: options.np },
            tamper,
        ),
        Err(found) => Err(Report::from_diagnostics("prepare", vec![found], None)),
    }
}

/// The gate chain for any backend: the study is checked against what the backend can run, written
/// by it, scanned, read back and exported through it.
// The report is the error on purpose: a failed chain hands back the finished, classified result.
#[allow(clippy::result_large_err)]
pub fn prepare_for(
    study_json: &str,
    backend: &dyn Backend,
    options: &CompileOptions,
) -> Result<Prepared, Report> {
    chain(study_json, backend, options, |_| {})
}

/// `prepare_for`, with a closure that edits the compiled tree before the safety scan. It exists so
/// tests can simulate a writer fault; no front end calls it.
#[doc(hidden)]
// The report is the error on purpose: a failed chain hands back the finished, classified result.
#[allow(clippy::result_large_err)]
pub fn prepare_for_with_tamper(
    study_json: &str,
    backend: &dyn Backend,
    options: &CompileOptions,
    tamper: impl FnOnce(&mut CaseTree),
) -> Result<Prepared, Report> {
    chain(study_json, backend, options, tamper)
}

/// `prepare_for` a registry row. A candidate backend adds its candidate warning and a preview
/// backend its preview warning, to the case and to a failed report alike, so a result from a row
/// that is not admitted never reads as one that is.
// The report is the error on purpose: a failed chain hands back the finished, classified result.
#[allow(clippy::result_large_err)]
pub fn prepare_row(
    study_json: &str,
    row: &Row,
    options: &CompileOptions,
) -> Result<Prepared, Report> {
    let extra = row_warnings(row);
    match prepare_for(study_json, row.backend, options) {
        Ok(mut prepared) => {
            prepared.warnings.extend(extra);
            Ok(prepared)
        }
        Err(mut report) => {
            report.diagnostics.extend(extra);
            Err(report)
        }
    }
}

// The report is the error on purpose: a failed chain hands back the finished, classified result.
#[allow(clippy::result_large_err)]
fn chain(
    study_json: &str,
    backend: &dyn Backend,
    options: &CompileOptions,
    tamper: impl FnOnce(&mut CaseTree),
) -> Result<Prepared, Report> {
    let fail = |found: Vec<Diagnostic>, hash: Option<String>, note: Option<MigrationNote>| {
        Report::from_diagnostics("prepare", found, hash).with_migration(note)
    };
    let (study, migration) = Study::from_json_with_note(study_json)
        .map_err(|e| fail(vec![Diagnostic::from_study_error(&e)], None, None))?;
    let found = diagnose(&study, &backend.capabilities());
    if has_errors(&found) {
        return Err(fail(found, None, migration));
    }
    let model_refused = |message: String| {
        Diagnostic::error("CFD-MODEL-003", "study", "", message).with_hint(
            "The checks accepted this study but the compiler refused it. Please report it.",
        )
    };
    let resolved = resolve_study(&study)
        .map_err(|e| fail(vec![model_refused(e.to_string())], None, migration.clone()))?;
    let hash = canonical_hash(&resolved);
    let mut tree = backend.compile(&resolved, options).map_err(|e| {
        let finding = match e {
            CompileFailure::Raw { label, reason } => raw_refused(&label, &reason),
            other => model_refused(other.to_string()),
        };
        fail(vec![finding], Some(hash.clone()), migration.clone())
    })?;
    tamper(&mut tree);
    let refusals = backend.scan(&tree);
    if !refusals.is_empty() {
        let found = refusals.iter().map(backend_refusal_diagnostic).collect();
        return Err(fail(found, Some(hash), migration));
    }
    // The values of the written case are read back through the backend's own reader and compared
    // with the study.
    let unread = backend.read_back(&resolved, &tree);
    if has_errors(&unread) {
        return Err(fail(unread, Some(hash), migration));
    }
    let export_tree = backend.export(&resolved, &tree, &hash, options);
    Ok(Prepared {
        resolved,
        hash,
        tree,
        export_tree,
        migration,
        warnings: found,
    })
}
