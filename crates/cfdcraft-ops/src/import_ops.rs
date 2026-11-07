//! Importing an existing OpenFOAM case into the workspace, and checking an imported case.
//!
//! An import is a raw, read-only copy with an inventory (see the import crate). These functions
//! resolve the names a caller gives, run the import and turn what happened into a [`Report`]:
//! the codes `CFD-IMPORT-001` (the source cannot be read), `002` (a refused construct, one
//! finding per entry), `003` (the target exists), `004` (not a case), `005` (no backend runs the
//! dialect), `006` (a cap was reached) and `007` (the stored original and its record disagree).
//!
//! A report names a case by its name only: never the folder it was read from and never an
//! absolute path. The source of the agent-facing function is a name below the workspace root,
//! resolved with plain relative rules; the command line may also give a folder.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use cfdcraft_import::{
    ImportError, ImportOptions, ImportOutcome, Record, image_id_for_dialect, verify_import,
};
use cfdcraft_model::{Diagnostic, Severity};
use cfdcraft_text::Line;

use crate::backends::{bounded, by_id};
use crate::cases::cli_problem;
use crate::documents::say;
use crate::report::{ExitClass, Report};
use crate::repro::{ImportData, InventoryItem, Repro};

const IMPORT: &str = "import-case";
const RUNNABLE: &str = "check-runnable";
const VERIFY: &str = "verify-case";

/// The largest `import.json` that is read.
const RECORD_MAX: u64 = 64 * 1024 * 1024;

/// The extension of an imported case folder.
const CASE_EXTENSION: &str = "cfdcase";

/// One fact as a sentence: a label and a value. The label is a stable word or phrase, never text a
/// person typed.
pub(crate) fn fact(key: &str, value: &str) -> String {
    say(&Line::ImportInventory {
        key: key.to_owned(),
        value: value.to_owned(),
    })
}

fn error(code: &str, key: &str, value: &str) -> Diagnostic {
    Diagnostic::error(code, "case", "", fact(key, value))
}

fn refuse(operation: &str, found: Diagnostic) -> Report {
    Report::from_diagnostics(operation, vec![found], None)
}

/// The folder of an imported case, or the refusal for a name that is not a plain name.
#[allow(clippy::result_large_err)] // the refusal is the whole answer, built once
fn case_dir(workspace: &Path, name: &str, operation: &str) -> Result<PathBuf, Report> {
    if !cfdcraft_runs::is_plain_name(name) {
        let found = cli_problem(fact(
            "a case name is 1 to 40 lower-case letters, digits and hyphens",
            &bounded(name),
        ));
        return Err(refuse(operation, found));
    }
    Ok(workspace.join(format!("{name}.{CASE_EXTENSION}")))
}

/// The folder a workspace-relative source name stands for. Only plain parts are accepted: no
/// absolute name, no drive, no `..`, no backslash or colon, no control character, and no part that
/// is a link, so the name cannot lead outside the workspace.
fn workspace_source(workspace: &Path, source: &str) -> Option<PathBuf> {
    if source.is_empty() || source.contains(['\\', ':']) || source.chars().any(char::is_control) {
        return None;
    }
    let mut path = workspace.to_path_buf();
    for part in Path::new(source).components() {
        match part {
            Component::Normal(name) => path.push(name),
            Component::CurDir => continue,
            _ => return None,
        }
        let meta = fs::symlink_metadata(&path).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
    }
    (path != workspace).then_some(path)
}

/// The finding for an import that did not happen. `shown` is the name of the source folder and
/// `case` the name of the target.
fn failure(error_found: &ImportError, shown: &str, case: &str) -> Diagnostic {
    match error_found {
        ImportError::Source { .. } => error(
            "CFD-IMPORT-001",
            "the source folder cannot be read or is not a folder",
            shown,
        ),
        ImportError::TargetExists => {
            error("CFD-IMPORT-003", "the target folder already exists", case)
        }
        ImportError::NotACase => error(
            "CFD-IMPORT-004",
            "the folder is not an OpenFOAM case",
            shown,
        ),
        ImportError::Cap { what, limit, at } => {
            let value = format!("{what} limit {limit} at {}", bounded(at));
            error(
                "CFD-IMPORT-006",
                "a cap was reached: file count, file size or depth",
                &value,
            )
        }
        ImportError::Write { .. } => cli_problem(fact("the import could not be written", case)),
    }
}

/// What the dictionaries of a record say, as label and value, in a fixed order.
fn inventory_items(record: &Record) -> Vec<InventoryItem> {
    let inventory = &record.inventory;
    let word = |value: &Option<String>| value.clone().unwrap_or_else(|| "unknown".to_owned());
    let list = |values: &[String]| {
        if values.is_empty() {
            "none".to_owned()
        } else {
            values.join(", ")
        }
    };
    let turbulence = match (&inventory.turbulence_model, &inventory.turbulence_detail) {
        (Some(kind), Some(detail)) => format!("{kind} {detail}"),
        (Some(kind), None) => kind.clone(),
        (None, _) => "unknown".to_owned(),
    };
    let time = &inventory.time_settings;
    let mut items = vec![
        ("solver", word(&inventory.application_or_solver)),
        ("turbulence", turbulence),
        ("mesh", inventory.mesh_kind.clone()),
        (
            "boundary conditions",
            list(&inventory.boundary_condition_kinds),
        ),
        ("end time", word(&time.end_time)),
        ("time step", word(&time.delta_t)),
        ("function objects", list(&inventory.function_objects)),
        ("guard policy", record.guard_policy.clone()),
    ];
    if !record.skipped.is_empty() {
        let skipped: Vec<String> = record
            .skipped
            .iter()
            .map(|(kind, count)| format!("{kind} {count}"))
            .collect();
        items.push(("left out", skipped.join(", ")));
    }
    items
        .into_iter()
        .map(|(key, value)| InventoryItem {
            key: key.to_owned(),
            value,
        })
        .collect()
}

/// Run the import of `folder` into `<workspace>/<name>.cfdcase`. `shown` names the source in the
/// report.
fn run_import(folder: &Path, workspace: &Path, shown: &str, name: &str) -> Report {
    let target = match case_dir(workspace, name, IMPORT) {
        Ok(target) => target,
        Err(report) => return report,
    };
    let case = format!("{name}.{CASE_EXTENSION}");
    match cfdcraft_import::import_case(folder, &target, &ImportOptions::default()) {
        Ok(ImportOutcome::Imported(record)) => {
            let runnable = image_id_for_dialect(&record.inventory.dialect)
                .and_then(by_id)
                .is_some();
            let data = ImportData {
                case: case.clone(),
                files: record.files.len() as u64,
                refused: 0,
                dialect: record.inventory.dialect.clone(),
                inventory: inventory_items(&record),
                runnable,
            };
            let artifacts = [
                ("case", case),
                ("files", data.files.to_string()),
                ("dialect", data.dialect.clone()),
            ];
            let mut report = Report::new(IMPORT).with_repro(Repro::Import(Box::new(data)));
            for (kind, value) in artifacts {
                report = report.with_artifact(kind, value);
            }
            report
        }
        Ok(ImportOutcome::Refused { refusals, .. }) => {
            let found = refusals
                .iter()
                .map(|r| {
                    let line = Line::ImportRefusedEntry {
                        file: bounded_entry(&r.file),
                        line: r.line.to_string(),
                        rule: r.code.clone(),
                    };
                    Diagnostic::error("CFD-IMPORT-002", "case", "", say(&line))
                })
                .collect();
            // The safety scan refused the case, so the class says so.
            let mut report = Report::from_diagnostics(IMPORT, found, None);
            report.set_class(ExitClass::GuardRefused);
            report
        }
        Err(e) => refuse(IMPORT, failure(&e, shown, &case)),
    }
}

/// The last part of a name a caller typed, for a message: never the whole name, which may be an
/// absolute path.
fn shown_name(typed: &str) -> String {
    Path::new(typed)
        .components()
        .rev()
        .find_map(|part| match part {
            Component::Normal(name) => Some(bounded(&name.to_string_lossy())),
            _ => None,
        })
        .unwrap_or_else(|| "source".to_owned())
}

/// A file name from a case, with control characters replaced and a length bound.
fn bounded_entry(file: &str) -> String {
    file.chars()
        .take(200)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// Import the case folder named `source`, a name below the workspace root, as
/// `<workspace>/<name>.cfdcase`.
///
/// `source` is resolved with plain relative rules: an absolute name, a `..` part, a link and any
/// name that is not a folder are refused with `CFD-IMPORT-001` and nothing is opened. A case that
/// holds a refused construct is refused as a whole (`CFD-IMPORT-002`, one finding per entry) and
/// nothing is written. The report names only the case.
pub fn import_case(workspace: &Path, source: &str, name: &str) -> Report {
    let shown = shown_name(source);
    match workspace_source(workspace, source) {
        Some(folder) => run_import(&folder, workspace, &shown, name),
        None => refuse(
            IMPORT,
            error(
                "CFD-IMPORT-001",
                "the source folder cannot be read or is not a folder",
                &shown,
            ),
        ),
    }
}

/// Import the case folder `source_folder`, which the command line may give as an absolute folder,
/// as `<workspace>/<name>.cfdcase`. The report names the last part of the folder only.
pub fn import_case_path(source_folder: &Path, workspace: &Path, name: &str) -> Report {
    let shown = shown_name(&source_folder.to_string_lossy());
    run_import(source_folder, workspace, &shown, name)
}

/// The record of an imported case, or the finding that says why it cannot be read.
// The finding is the whole answer and is built once, as in the registry's own lookups.
#[allow(clippy::result_large_err)]
fn read_record(dir: &Path) -> Result<Record, Diagnostic> {
    let broken = |why: &str| error("CFD-IMPORT-007", "the import record is", why);
    let file = File::open(dir.join("import.json")).map_err(|_| broken("missing"))?;
    let mut bytes = Vec::new();
    file.take(RECORD_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| broken("unreadable"))?;
    if bytes.len() as u64 > RECORD_MAX {
        return Err(broken("larger than the cap"));
    }
    serde_json::from_slice(&bytes).map_err(|_| broken("unreadable"))
}

/// Whether a registered backend runs the imported case `name`, and which.
///
/// A case whose dialect no backend runs can be inspected but not run: `CFD-IMPORT-005`. Otherwise
/// the report names the backend as an artifact of kind `backend`.
pub fn check_runnable(workspace: &Path, name: &str) -> Report {
    let dir = match case_dir(workspace, name, RUNNABLE) {
        Ok(dir) => dir,
        Err(report) => return report,
    };
    let record = match read_record(&dir) {
        Ok(record) => record,
        Err(found) => return refuse(RUNNABLE, found),
    };
    let dialect = record.inventory.dialect;
    match image_id_for_dialect(&dialect).and_then(by_id) {
        Some(row) => Report::new(RUNNABLE)
            .with_artifact("backend", row.backend.id())
            .with_artifact("dialect", dialect),
        None => {
            let line = Line::ImportNoMatchingBackend { dialect };
            refuse(
                RUNNABLE,
                Diagnostic::new(Severity::Error, "CFD-IMPORT-005", "case", "", say(&line)),
            )
        }
    }
}

/// Check the stored original of the imported case `name` against its record. A changed, missing
/// or extra file is `CFD-IMPORT-007` and names the file; a cap reached while checking is
/// `CFD-IMPORT-006`. The working copy is meant to change and is never looked at.
pub fn verify_case(workspace: &Path, name: &str) -> Report {
    let dir = match case_dir(workspace, name, VERIFY) {
        Ok(dir) => dir,
        Err(report) => return report,
    };
    let findings = verify_import(&dir, &ImportOptions::default());
    let found: Vec<Diagnostic> = findings
        .iter()
        .map(|f| {
            Diagnostic::error(
                f.code,
                "case",
                "",
                fact(&bounded_entry(&f.file), &f.message),
            )
        })
        .collect();
    Report::from_diagnostics(VERIFY, found, None)
        .with_artifact("case", format!("{name}.{CASE_EXTENSION}"))
}
