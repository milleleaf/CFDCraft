//! The case operations behind the command line: compile, export and check a case directory.

use crate::{
    jobs::row_for,
    prepare::{Prepared, prepare_row, refusal_diagnostic, rel_name},
    report::{ExitClass, Report},
};
use cfdcraft_backend::CompileOptions;
use cfdcraft_backend_esi::{CaseTree, guard, write_to_disk};
use cfdcraft_model::Diagnostic;
use std::path::Path;

/// A problem with a path or file the caller named. Class 2, like an invalid study.
pub fn cli_problem(message: String) -> Diagnostic {
    Diagnostic::error("CFD-CLI-001", "command line", "", message)
}

/// Writing into a directory that already holds anything could carry a stale file into the case.
fn dir_problem(dir: &Path) -> Option<Diagnostic> {
    match std::fs::read_dir(dir).map(|mut entries| entries.next().is_none()) {
        Ok(true) => None,
        Ok(false) => Some(cli_problem(format!(
            "The output directory {} is not empty. Choose a new or empty directory.",
            dir.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(cli_problem(format!(
            "The output directory {} cannot be used: {e}.",
            dir.display()
        ))),
    }
}

/// The gates of the backend `backend` names (the registry default when none), then the chosen
/// tree into an empty `out_dir`. Nothing is written when any step fails, and an id no backend has
/// is `CFD-BACKEND-001`.
fn write_case(
    operation: &str,
    study_json: &str,
    out_dir: &Path,
    backend: Option<&str>,
    pick: impl FnOnce(&Prepared) -> &CaseTree,
) -> Report {
    let row = match row_for(backend) {
        Ok(row) => row,
        Err(found) => return Report::from_diagnostics(operation, vec![found], None),
    };
    let prepared = match prepare_row(study_json, &row, &CompileOptions::default()) {
        Ok(p) => p,
        Err(report) => return report.for_operation(operation),
    };
    let tree = pick(&prepared);
    let mut found = prepared.warnings.clone();
    if let Some(d) = dir_problem(out_dir) {
        found.push(d);
    } else if let Err(e) = write_to_disk(tree, out_dir) {
        found.push(cli_problem(format!("The case could not be written: {e}.")));
    }
    let mut report = Report::from_diagnostics(operation, found, Some(prepared.hash.clone()))
        .with_migration(prepared.migration.clone());
    if report.class() == ExitClass::Passed {
        for file in &tree.files {
            let name = rel_name(&file.rel_path);
            let kind = if name == "Allrun" || name == "README.md" {
                "export_file"
            } else {
                "case_file"
            };
            report = report.with_artifact(kind, name);
        }
    }
    report
}

/// Compile a study into the dictionary tree at `out_dir`.
pub fn compile_study(study_json: &str, out_dir: &Path) -> Report {
    compile_study_on(study_json, out_dir, None)
}

/// [`compile_study`] for the backend with this id; none is the registry default. The case is that
/// backend's own tree, and a candidate or preview backend's warnings are on the report.
pub fn compile_study_on(study_json: &str, out_dir: &Path, backend: Option<&str>) -> Report {
    write_case("compile", study_json, out_dir, backend, |p| &p.tree)
}

/// Compile a study into the runnable form: dictionaries, `Allrun` and `README.md`.
pub fn export_study(study_json: &str, out_dir: &Path) -> Report {
    write_case("export", study_json, out_dir, None, |p| &p.export_tree)
}

/// Scan any case directory with the safety scan. Needs no study.
pub fn check_case(dir: &Path) -> Report {
    let fail =
        |message: String| Report::from_diagnostics("check-case", vec![cli_problem(message)], None);
    if !dir.is_dir() {
        return fail(format!("{} is not a directory.", dir.display()));
    }
    let scan = match guard::scan_dir(dir) {
        Ok(scan) => scan,
        Err(e) => return fail(format!("{} cannot be read: {e}.", dir.display())),
    };
    let found = scan.refusals.iter().map(refusal_diagnostic).collect();
    let mut report = Report::from_diagnostics("check-case", found, None);
    for name in scan.scanned {
        report = report.with_artifact("scanned", name);
    }
    for name in scan.skipped {
        report = report.with_artifact("skipped", name);
    }
    report
}
