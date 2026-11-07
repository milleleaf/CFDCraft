//! The parser against real writer output: every dictionary in the golden case folders.
//! The test only reads, and follows no link.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_foamdict::{Limits, parse};

/// The largest file read, in bytes.
const MAX_FILE: u64 = 16 * 1024 * 1024;

/// The files below a folder, sorted, without following links.
fn files_below(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .expect("the folder is readable")
        .map(|entry| entry.expect("the entry is readable").path())
        .collect();
    paths.sort();
    for path in paths {
        let kind = fs::symlink_metadata(&path)
            .expect("the file has metadata")
            .file_type();
        if kind.is_dir() {
            files_below(&path, out);
        } else if kind.is_file() {
            out.push(path);
        }
    }
}

/// Whether the first text that is not a comment is the `FoamFile` header block.
fn starts_with_header(text: &str) -> bool {
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("//") {
            rest = after.split_once('\n').map_or("", |(_, tail)| tail);
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after.split_once("*/").map_or("", |(_, tail)| tail);
        } else {
            return rest.starts_with("FoamFile");
        }
    }
}

#[test]
fn every_dictionary_the_esi_writer_produces_parses() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cfdcraft-backend-esi/tests/golden");
    let mut folders: Vec<PathBuf> = fs::read_dir(&golden)
        .expect("the golden folder is readable")
        .map(|entry| entry.expect("the entry is readable").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("-v2512"))
        })
        .collect();
    folders.sort();
    assert!(!folders.is_empty(), "no golden case folders were found");

    let mut parsed = 0usize;
    for folder in &folders {
        let mut files = Vec::new();
        files_below(folder, &mut files);
        for file in files {
            let size = fs::symlink_metadata(&file)
                .expect("the file has metadata")
                .len();
            assert!(size <= MAX_FILE, "{} is over the read cap", file.display());
            let text = fs::read_to_string(&file).expect("the file is text");
            if !starts_with_header(&text) {
                continue;
            }
            let doc = parse(&text, &Limits::default())
                .unwrap_or_else(|e| panic!("{} does not parse: {e}", file.display()));
            let header = doc
                .header
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no header", file.display()));
            assert!(
                !header.object.is_empty(),
                "{} has an empty object",
                file.display()
            );
            parsed += 1;
        }
    }
    assert!(parsed >= 30, "only {parsed} dictionaries were parsed");
}
