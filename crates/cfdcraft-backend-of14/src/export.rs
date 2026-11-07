//! The plain, runnable form of a compiled case: the case files plus an `Allrun` and a `README.md`.
//!
//! One `Allrun` generator serves every way of running a case, so a hand run of an exported folder
//! is a statement about the same file the solver run executes.

use cfdcraft_backend::{CaseFile, CaseTree, RunPlan};

const HEADER: &str = "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n";

/// Shown verbatim in public solver-related documentation.
const ACKNOWLEDGEMENT: &str = "CFDCraft is not approved or endorsed by the OpenFOAM Foundation Limited or by OpenCFD Limited. OPENFOAM® is a registered trade mark of OpenCFD Limited.";

/// The script of a plan: each step is a plain command that writes its own log and stops the script
/// when it fails.
pub fn allrun_script(plan: &RunPlan) -> String {
    let mut text = String::from(HEADER);
    for step in &plan.steps {
        text.push_str(&format!("{} > {} 2>&1 || exit 1\n", step.command, step.log));
    }
    text
}

/// The names of some fields as prose: `U`, `U` and `p`, `U`, `p` and `T`.
fn field_list(fields: &[String]) -> String {
    let quoted: Vec<String> = fields.iter().map(|f| format!("`{f}`")).collect();
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// Deterministic: no date, no version, one line per paragraph.
pub fn readme_text(study_hash: &str, np: u32, plan: &RunPlan) -> String {
    let listed = plan
        .judged_logs
        .iter()
        .map(|log| format!("`{log}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let fields = field_list(&plan.final_fields);
    let mut lines = vec![
        "# Exported case".to_string(),
        "This directory is a complete Foundation OpenFOAM 14 case with an `Allrun` script. It runs without CFDCraft.".into(),
        format!("Study hash: `{study_hash}`"),
        "## Image".into(),
        "The case is run with OpenFOAM 14 in an image built from `docker/of14` in the CFDCraft repository. No image is published yet, so there is no reference to pull.".into(),
        "## Run".into(),
        "From this directory, with `IMAGE` standing for the image reference once it exists:".into(),
        "```\ndocker run --rm --user \"$(id -u):$(id -g)\" --network none -v \"$PWD\":/w IMAGE bash -lc 'cd /w && sh ./Allrun'\n```".into(),
        "The `cd /w` is required: the commands must run in the case folder.".into(),
        format!("This case runs on {np} processes: the container needs `--cpus {np}`, and the results of a parallel run can differ from a serial one in the last digits. The decomposition and reconstruction steps write `log.decomposePar` and `log.reconstructPar`."),
        "## Judge the result by the logs, not the exit status".into(),
        format!("A run is good only when each of the logs ({listed}) ends with `End`, `log.checkMesh` contains `Mesh OK`, no log reports a fatal error or a floating-point exception, and the directory named for the end time in `system/controlDict` holds non-empty {fields}. The exit status of `Allrun` is a convenience only."),
        "## Licence".into(),
        "OpenFOAM is free software under the GNU General Public License, version 3.".into(),
        "## Trade marks".into(),
        ACKNOWLEDGEMENT.into(),
    ];
    if np <= 1 {
        lines.remove(9);
    }
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
