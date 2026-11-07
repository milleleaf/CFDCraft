//! The verb that imports an existing OpenFOAM case as a read-only copy with an inventory. The
//! target is named by `--out <name>.cfdcase`: the folder holding it is the workspace, and the
//! name is its stem. The report names the case and never the folder it was read from.

use std::path::Path;

use cfdcraft_ops::{Report, Repro, cases::cli_problem, import_case_path};

use super::{Error, emit_lines};

/// The folder that holds the target and the name of the case, from `--out`; `None` when the name
/// is not `<name>.cfdcase`.
fn target_of(out: &Path) -> Option<(&Path, &str)> {
    let named = out.extension().is_some_and(|e| e == "cfdcase");
    let name = out.file_stem().and_then(|s| s.to_str()).filter(|_| named)?;
    let folder = out.parent().filter(|p| !p.as_os_str().is_empty());
    Some((folder.unwrap_or(Path::new(".")), name))
}

/// Import the case folder `source` as `out`, a name ending in `.cfdcase`.
pub fn execute(source: &Path, out: &Path, json: bool) -> Result<i32, Error> {
    let report = match target_of(out) {
        Some((workspace, name)) => import_case_path(source, workspace, name),
        None => Report::from_diagnostics(
            "import-case",
            vec![cli_problem(
                "The target must be a name that ends in .cfdcase, for example mycase.cfdcase."
                    .to_owned(),
            )],
            None,
        ),
    };
    Ok(emit_lines(&report, json, |data| match data {
        Repro::Import(import) => import.lines(),
        _ => Vec::new(),
    }))
}
