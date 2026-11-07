//! The plain, runnable form of a compiled case: the case files plus an `Allrun` and a `README.md`.
//!
//! One `Allrun` generator serves every way of running a case, so a hand run of an exported folder
//! is a statement about the same file the solver run executes.

use crate::markers::{END_BANNER, HISTORY_FILE, THREAD_FLAG};
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

/// Deterministic: no date, no version, one line per paragraph. `threads` is the number of threads
/// the case runs on.
pub fn readme_text(study_hash: &str, threads: u32, plan: &RunPlan) -> String {
    let threads = threads.max(1);
    let log = &plan.solver_log;
    let mut lines = vec![
        "# Exported case".to_string(),
        "This directory is a complete SU2 case with an `Allrun` script. It runs without CFDCraft.".into(),
        format!("Study hash: `{study_hash}`"),
        "## Image".into(),
        "The case is run with SU2 8.6.0 in an image built from `docker/su2` in the CFDCraft repository. No image is published yet, so there is no reference to pull.".into(),
        "## Run".into(),
        "From this directory, with `IMAGE` standing for the image reference once it exists:".into(),
        "```\ndocker run --rm --user \"$(id -u):$(id -g)\" --network none -v \"$PWD\":/w IMAGE bash -lc 'cd /w && sh ./Allrun'\n```".into(),
        "The `cd /w` is required: the commands must run in the case folder.".into(),
        format!("This case runs on {threads} threads (`{THREAD_FLAG} {threads}`): the container needs `--cpus {threads}`, and the results of a threaded run can differ from a serial one in the last digits."),
        "## Judge the result by the files, not the exit status".into(),
        format!("A run is good only when `{log}` ends with `{END_BANNER}` and `{HISTORY_FILE}` exists and is not empty. The exit status of `Allrun` is a convenience only."),
        "## Licence".into(),
        "SU2 is free software under the GNU Lesser General Public License, version 2.1.".into(),
    ];
    if threads == 1 {
        lines.remove(9);
    }
    lines.join("\n\n") + "\n"
}

/// The compiled files in their original order, then `Allrun` and `README.md`.
pub fn export_tree(tree: &CaseTree, study_hash: &str, threads: u32, plan: &RunPlan) -> CaseTree {
    let mut files = tree.files.clone();
    files.push(CaseFile {
        rel_path: "Allrun".into(),
        bytes: allrun_script(plan).into_bytes(),
    });
    files.push(CaseFile {
        rel_path: "README.md".into(),
        bytes: readme_text(study_hash, threads, plan).into_bytes(),
    });
    CaseTree { files }
}
