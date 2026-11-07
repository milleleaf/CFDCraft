//! The plain, runnable form of a compiled case: the case files plus an `Allrun` and a `README.md`.
//!
//! One `Allrun` generator serves every way of running a case, so a hand run of an exported
//! directory is a statement about the same on-disk file that CFDCraft itself executes.

use crate::{CaseFile, CaseTree, dialect::IMAGE};

/// Shown verbatim in public solver-related documentation.
const ACKNOWLEDGEMENT: &str = "CFDCraft is not approved or endorsed by OpenCFD Limited. OPENFOAM® and OpenCFD® are registered trade marks of OpenCFD Limited.";

/// The three fixed steps, each writing its own log. Plain commands on purpose: the tutorial
/// helper functions skip a step whose log already exists, and a stale log would be judged as new.
const STEPS: [(&str, &str); 3] = [
    ("blockMesh", "log.blockMesh"),
    ("checkMesh", "log.checkMesh"),
    ("simpleFoam", "log.simpleFoam"),
];

const HEADER: &str = "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n";

/// The only script a serial case runs. `dirname "$0"` works for `./Allrun` and for `sh Allrun`,
/// where the `${0%/*}` form does not.
pub fn allrun_script() -> String {
    let mut text = String::from(HEADER);
    for (command, log) in STEPS {
        text.push_str(&format!("{command} > {log} 2>&1 || exit 1\n"));
    }
    text
}

/// The script for `np` processes. One process is exactly `allrun_script()`; more decompose after
/// the mesh checks, run the solver in parallel and reconstruct the latest time.
pub fn allrun_script_for(np: u32) -> String {
    if np <= 1 {
        return allrun_script();
    }
    let solver = format!("mpirun -np {np} simpleFoam -parallel");
    let mut text = String::from(HEADER);
    for (command, log) in [
        ("blockMesh", "log.blockMesh"),
        ("checkMesh", "log.checkMesh"),
        ("decomposePar -force", "log.decomposePar"),
        (solver.as_str(), "log.simpleFoam"),
        ("reconstructPar -latestTime", "log.reconstructPar"),
    ] {
        text.push_str(&format!("{command} > {log} 2>&1 || exit 1\n"));
    }
    text
}

/// Deterministic: no date, no version, one line per paragraph.
pub fn readme_text(study_hash: &str) -> String {
    readme_text_for(study_hash, 1)
}

/// The README for a case run on `np` processes; one process is `readme_text`.
pub fn readme_text_for(study_hash: &str, np: u32) -> String {
    let parallel = (np > 1).then(|| format!("This case runs on {np} processes: the container needs `--cpus {np}`, and the results of a parallel run can differ from a serial one in the last digits. The decomposition and reconstruction steps write `log.decomposePar` and `log.reconstructPar`."));
    let mut lines = vec![
        "# Exported case".to_string(),
        "This directory is a complete OpenFOAM case with an `Allrun` script. It runs without CFDCraft.".into(),
        format!("Study hash: `{study_hash}`"),
        "## Image".into(),
        "The case is run in a pinned image, referenced by digest so the solver build cannot change underneath it.".into(),
        format!("```\ndocker pull {IMAGE}\n```"),
        "## Run".into(),
        "From this directory:".into(),
        format!(
            "```\ndocker run --rm --user \"$(id -u):$(id -g)\" --network none -v \"$PWD\":/w {IMAGE} bash -lc 'cd /w && sh ./Allrun'\n```"
        ),
        "The `cd /w` is required: the image entrypoint resets the working directory, so without it the commands run somewhere else.".into(),
        "CFDCraft's own runs add `--cpus 2 --memory 8g --memory-swap 8g --pids-limit 256`; similar limits are sensible on a shared machine.".into(),
        "## Judge the result by the logs, not the exit status".into(),
        "A run is good only when each of the three logs (`log.blockMesh`, `log.checkMesh`, `log.simpleFoam`) ends with `End`, `log.checkMesh` contains `Mesh OK`, no log reports a fatal error or a floating-point exception, and the directory named for the end time in `system/controlDict` holds non-empty `U` and `p`. The start-up banner that says floating-point trapping is enabled is normal.".into(),
        "The exit status of `Allrun` is a convenience only.".into(),
        "## Other CPU architectures".into(),
        "The image is a multi-architecture index, so results on a different CPU architecture can differ in the last printed digits. Judge them against a stated tolerance, not byte equality.".into(),
        "## Trade marks".into(),
        ACKNOWLEDGEMENT.into(),
    ];
    if let Some(paragraph) = parallel {
        lines.insert(9, paragraph);
    }
    lines.join("\n\n") + "\n"
}

/// The compiled files in their original order, then `Allrun` and `README.md`.
pub fn export_tree(tree: &CaseTree, study_hash: &str) -> CaseTree {
    export_tree_for(tree, study_hash, 1)
}

/// `export_tree` for a case written for `np` processes.
pub fn export_tree_for(tree: &CaseTree, study_hash: &str, np: u32) -> CaseTree {
    let mut files = tree.files.clone();
    files.push(CaseFile {
        rel_path: "Allrun".into(),
        bytes: allrun_script_for(np).into_bytes(),
    });
    files.push(CaseFile {
        rel_path: "README.md".into(),
        bytes: readme_text_for(study_hash, np).into_bytes(),
    });
    CaseTree { files }
}
