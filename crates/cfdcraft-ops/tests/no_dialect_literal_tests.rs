//! Nothing that runs a study names the ESI dialect: the operations, the queue runner and the
//! command line ask the backend registry for a row. Tests and the thin wrappers they use may
//! still name it; the text of a file from its first `#[cfg(test)]` line to the end is not read.

use std::fs;
use std::path::{Path, PathBuf};

/// The text no non-test source of the three crates may hold.
const LITERAL: &str = "Dialect::EsiV2512";

/// The folders read, each below the `crates` folder.
const FOLDERS: [&str; 3] = ["cfdcraft-ops/src", "cfdcraft-drain/src", "cfdcraft-cli/src"];

/// The line numbers (from 1) at which the literal stands before the first `#[cfg(test)]` line.
fn violations(text: &str) -> Vec<usize> {
    text.lines()
        .take_while(|line| !line.trim_start().starts_with("#[cfg(test)]"))
        .enumerate()
        .filter(|(_, line)| line.contains(LITERAL))
        .map(|(at, _)| at + 1)
        .collect()
}

/// Every `.rs` file below `dir`. A link is not followed: it is neither read nor entered.
fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    for entry in listing.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            rust_files(&path, found);
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// The crates folder: the parent of this crate's own folder.
fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate folder has a parent")
        .to_path_buf()
}

/// The files of each folder, in the order of `FOLDERS`.
fn files_by_folder() -> Vec<(&'static str, Vec<PathBuf>)> {
    let root = crates_dir();
    FOLDERS
        .iter()
        .map(|folder| {
            let mut found = Vec::new();
            rust_files(&root.join(folder), &mut found);
            found.sort();
            (*folder, found)
        })
        .collect()
}

#[test]
fn no_non_test_source_names_the_esi_dialect() {
    let mut problems = Vec::new();
    for (_, files) in files_by_folder() {
        for file in files {
            let Ok(text) = fs::read_to_string(&file) else {
                problems.push(format!("{}: the file cannot be read", file.display()));
                continue;
            };
            for line in violations(&text) {
                problems.push(format!("{}:{line}", file.display()));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{LITERAL} appears outside tests; ask the backend registry instead:\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_gate_sees_a_planted_literal() {
    let planted =
        format!("fn a() {{ {LITERAL} }}\n\nfn b() {{}}\n#[cfg(test)]\nmod tests {{ {LITERAL} }}\n");
    // Only the first, which stands on line 1, is reported; the one after the marker is not.
    assert_eq!(violations(&planted), [1]);
    assert!(violations("fn clean() {}\n#[cfg(test)]\nfn t() {}\n").is_empty());
    let late = format!("fn a() {{}}\nfn b() {{}}\nlet x = {LITERAL};\n");
    assert_eq!(violations(&late), [3]);
}

#[test]
fn the_gate_covers_the_three_crates() {
    for (folder, files) in files_by_folder() {
        assert!(!files.is_empty(), "no source file was found in {folder}");
    }
}
