//! Checking a stored import: the files of `original/` against the hashes of `import.json`. The
//! working copy is meant to change and is never looked at.

use crate::inventory::read_capped;
use crate::record::{FORMAT, Record};
use crate::{ImportError, ImportOptions};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// The largest `import.json` that is read.
const RECORD_MAX: u64 = 64 * 1024 * 1024;

/// What a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    /// The record is missing or unreadable, so the import did not finish.
    Incomplete,
    /// A stored original differs from its recorded hash or size.
    Changed,
    /// A recorded file is missing from `original/` or is not a regular file.
    Missing,
    /// `original/` holds a file, link or special file that the record does not list.
    Extra,
    /// The record names a path that could not be stored safely.
    Unsafe,
    /// A cap was reached while checking.
    Cap,
}

/// One disagreement between the record and the stored original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: FindingKind,
    /// `CFD-IMPORT-007`, or `CFD-IMPORT-006` when a cap stopped the check.
    pub code: &'static str,
    /// The path below `original/`, or `import.json`.
    pub file: String,
    pub message: String,
}

fn finding(kind: FindingKind, file: &str, message: &str) -> Finding {
    Finding {
        kind,
        code: if kind == FindingKind::Cap {
            "CFD-IMPORT-006"
        } else {
            "CFD-IMPORT-007"
        },
        file: file.to_string(),
        message: message.to_string(),
    }
}

fn safe(rel: &str) -> bool {
    !rel.is_empty()
        && rel
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains(['\\', ':']))
        && craft_docpath::resolve(rel, None).is_ok()
}

/// Lists the entries below `dir` without following links: regular files into `files`, anything
/// else into `others`. Stops with `false` at the file cap.
fn list(
    dir: &Path,
    rel: &str,
    max: usize,
    files: &mut BTreeSet<String>,
    others: &mut BTreeSet<String>,
) -> bool {
    let Ok(read) = fs::read_dir(dir) else {
        return true;
    };
    let mut names: Vec<_> = read.filter_map(Result::ok).map(|e| e.file_name()).collect();
    names.sort();
    for name in names {
        let n = name.to_string_lossy();
        let child = if rel.is_empty() {
            n.to_string()
        } else {
            format!("{rel}/{n}")
        };
        let path = dir.join(&name);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            others.insert(child);
            continue;
        };
        if meta.is_dir() {
            if !list(&path, &child, max, files, others) {
                return false;
            }
        } else if meta.is_file() {
            files.insert(child);
            if files.len() > max {
                return false;
            }
        } else {
            others.insert(child);
        }
    }
    true
}

/// Re-hashes `original/` against `import.json` with the caps of `options`. An empty list means
/// the stored original is exactly what was imported.
pub fn verify_import(target: &Path, options: &ImportOptions) -> Vec<Finding> {
    let incomplete = |why: &str| vec![finding(FindingKind::Incomplete, "import.json", why)];
    let bytes = match read_capped(&target.join("import.json"), "import.json", RECORD_MAX) {
        Ok(bytes) => bytes,
        Err(ImportError::Cap { .. }) => {
            return vec![finding(
                FindingKind::Cap,
                "import.json",
                "the record is larger than the cap",
            )];
        }
        Err(_) => return incomplete("the record is missing, so the import did not finish"),
    };
    let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
        return incomplete("the record cannot be read");
    };
    if record.format != FORMAT {
        return incomplete("the record has an unknown format");
    }

    let mut out = Vec::new();
    let original = target.join("original");
    let mut listed = BTreeSet::new();
    for entry in &record.files {
        if !safe(&entry.rel_path) {
            out.push(finding(
                FindingKind::Unsafe,
                &entry.rel_path,
                "the record names a path that is not a safe relative path",
            ));
            continue;
        }
        listed.insert(entry.rel_path.clone());
        let path = original.join(&entry.rel_path);
        match read_capped(&path, &entry.rel_path, options.max_file_bytes) {
            Ok(stored) => {
                if stored.len() as u64 != entry.bytes
                    || blake3::hash(&stored).to_hex().as_str() != entry.blake3
                {
                    out.push(finding(
                        FindingKind::Changed,
                        &entry.rel_path,
                        "the stored original differs from the record",
                    ));
                }
            }
            Err(ImportError::Cap { .. }) => out.push(finding(
                FindingKind::Cap,
                &entry.rel_path,
                "the stored file is larger than the cap",
            )),
            Err(_) => out.push(finding(
                FindingKind::Missing,
                &entry.rel_path,
                "the stored original is missing or is not a regular file",
            )),
        }
    }

    let (mut files, mut others) = (BTreeSet::new(), BTreeSet::new());
    if !list(&original, "", options.max_files, &mut files, &mut others) {
        out.push(finding(
            FindingKind::Cap,
            "original",
            "the original holds more files than the cap",
        ));
        return out;
    }
    for extra in files.difference(&listed).chain(others.iter()) {
        out.push(finding(
            FindingKind::Extra,
            extra,
            "the original holds an entry the record does not list",
        ));
    }
    out
}
