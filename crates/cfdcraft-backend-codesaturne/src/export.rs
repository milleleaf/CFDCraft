//! The plain, runnable form of a compiled case: the case files plus an `Allrun` and a `README.md`.

use cfdcraft_backend::{CaseFile, CaseTree, RunPlan};

const HEADER: &str = "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n";

/// The script of a plan: each step is a plain command that writes its own log and stops the script
/// when it fails.
pub fn allrun_script(plan: &RunPlan) -> String {
    let mut text = String::from(HEADER);
    for step in &plan.steps {
        text.push_str(&format!("{} > {} 2>&1 || exit 1\n", step.command, step.log));
    }
    text
}

/// Deterministic: no date, no version, one line per paragraph.
pub fn readme_text(study_hash: &str, np: u32, plan: &RunPlan) -> String {
    let mut lines = vec![
        "# Exported case".to_owned(),
        "This directory is a complete code_saturne case with an `Allrun` script. It runs without CFDCraft.".to_owned(),
        format!("Study hash: `{study_hash}`"),
        "## Run".to_owned(),
        "The case needs a code_saturne 9.2.0 installation on the path. From this directory:".to_owned(),
        "```\nsh ./Allrun\n```".to_owned(),
    ];
    if np > 1 {
        lines.push(format!("This case runs on {np} processes."));
    }
    lines.extend([
        "## Mesh".to_owned(),
        "The mesh is one cell thick, with a symmetry condition on both faces along z, so the case is two-dimensional.".to_owned(),
        "## Judge the result by the logs, not the exit status".to_owned(),
        format!(
            "A run is good only when `{}` ends without an error message, no file whose name starts with `error` is left in the results folder, and the residual history in the results folder holds finite numbers up to the last iteration. The exit status of `Allrun` is a convenience only.",
            plan.solver_log
        ),
        "## Licences".to_owned(),
        "code_saturne is developed by EDF and released under the GNU General Public License, version 2 or later. CFDCraft is not approved or endorsed by EDF.".to_owned(),
    ]);
    lines.join("\n\n") + "\n"
}

/// The compiled files in their original order, then `Allrun` and `README.md`.
pub fn export_tree(tree: &CaseTree, study_hash: &str, np: u32, plan: &RunPlan) -> CaseTree {
    let mut files = tree.files.clone();
    files.push(CaseFile {
        rel_path: "Allrun".into(),
        bytes: allrun_script(plan).into_bytes(),
    });
    files.push(CaseFile {
        rel_path: "README.md".into(),
        bytes: readme_text(study_hash, np, plan).into_bytes(),
    });
    CaseTree { files }
}
