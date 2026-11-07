//! The plain, runnable form of a compiled case: the command stream plus a run script and a README
//! that says the result is a preview.

use crate::plan::{COMMANDS_FILE, LOG_FILE, REPLIES_FILE, allrun_text};
use cfdcraft_backend::{CaseFile, CaseTree};

/// Deterministic: no date, one line per paragraph.
pub fn readme_text(study_hash: &str) -> String {
    let lines = [
        "# Exported case".to_owned(),
        format!("This directory is a case for the FieldCraft lattice-Boltzmann solver, written by CFDCraft. It holds the command stream `{COMMANDS_FILE}` and a run script `Allrun`."),
        format!("Study hash: `{study_hash}`"),
        "## Preview".to_owned(),
        "The result of this solver is a preview, not a gated result. No benchmark is claimed for it, so do not compare it with a gated result as if it had passed the same checks.".to_owned(),
        "## Run".to_owned(),
        "The script runs in an image that holds `fieldcraft-cli` 0.7.5, with the case mounted at `/w` and no network. From this directory:".to_owned(),
        "```\ndocker run --rm --user \"$(id -u):$(id -g)\" --network none -v \"$PWD\":/w <image> bash -lc 'cd /w && sh ./Allrun'\n```".to_owned(),
        "The server reads the stream from standard input and ends when the input ends.".to_owned(),
        "## Judge the result by the replies, not the exit status".to_owned(),
        format!("The solver prints almost nothing by itself. Every reply is one line of `{REPLIES_FILE}`, and the solver's own messages are in `{LOG_FILE}`. A run is good only when each request has a reply, the solve reply reports no error, and the run did not stop as unstable. The exit status of `Allrun` is a convenience only."),
    ];
    lines.join("\n\n") + "\n"
}

/// The compiled files in their original order, then `Allrun` and `README.md`.
pub fn export_tree(tree: &CaseTree, study_hash: &str) -> CaseTree {
    let mut files = tree.files.clone();
    files.push(CaseFile {
        rel_path: "Allrun".into(),
        bytes: allrun_text().into_bytes(),
    });
    files.push(CaseFile {
        rel_path: "README.md".into(),
        bytes: readme_text(study_hash).into_bytes(),
    });
    CaseTree { files }
}
