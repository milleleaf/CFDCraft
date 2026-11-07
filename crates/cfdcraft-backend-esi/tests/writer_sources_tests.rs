//! The tool identity of a stage key carries a fingerprint of the writer's source files. These
//! tests keep that list honest: it must be the real text of the files, and a file added to the
//! crate must be placed on one side of it on purpose. A listed name that starts with `../` is a
//! file of another crate (the dictionary editor), read by its path from `src`.

use cfdcraft_backend_esi::WRITER_SOURCES;
use std::{collections::BTreeSet, fs, path::Path};

/// Files of `src` that do not shape the case that is written or the way it is run: the image and
/// capabilities, the judging of a run, the safety scan and the reader.
const NOT_THE_WRITER: [&str; 4] = ["dialect.rs", "evidence.rs", "guard.rs", "reader.rs"];

fn src() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".rs"))
        .collect()
}

#[test]
fn the_listed_sources_are_the_text_of_the_files() {
    for (name, text) in WRITER_SOURCES {
        assert_eq!(
            fs::read_to_string(src().join(name))
                .unwrap()
                .replace('\r', ""),
            text.replace('\r', ""),
            "{name}"
        );
    }
}

#[test]
fn every_source_file_is_either_part_of_the_writer_or_named_as_not_part_of_it() {
    let listed: BTreeSet<String> = WRITER_SOURCES
        .iter()
        .filter(|(name, _)| !name.starts_with("../"))
        .map(|(name, _)| (*name).to_owned())
        .chain(NOT_THE_WRITER.iter().map(|name| (*name).to_owned()))
        .collect();
    let found = rust_files(&src());
    assert_eq!(
        found, listed,
        "a new source file is neither in WRITER_SOURCES nor in NOT_THE_WRITER"
    );
    for (name, _) in WRITER_SOURCES {
        assert!(!NOT_THE_WRITER.contains(&name), "{name} is on both sides");
    }
}

#[test]
fn the_dictionary_editor_is_listed_and_is_one_file() {
    assert!(WRITER_SOURCES.iter().any(|(name, _)| *name == "raw.rs"));
    assert!(
        WRITER_SOURCES
            .iter()
            .any(|(name, _)| *name == "../../cfdcraft-dictedit/src/lib.rs")
    );
    // The editor is fingerprinted as one file, so a second source file there would escape it.
    let editor = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cfdcraft-dictedit/src");
    assert_eq!(
        rust_files(&editor),
        BTreeSet::from(["lib.rs".to_owned()]),
        "a file added to the editor must be listed in WRITER_SOURCES"
    );
}

#[test]
fn the_writer_sources_list_has_nine_files_including_monitors_rs() {
    assert_eq!(WRITER_SOURCES.len(), 9);
    assert!(
        WRITER_SOURCES
            .iter()
            .any(|(name, _)| *name == "monitors.rs")
    );
    assert!(!NOT_THE_WRITER.contains(&"monitors.rs"));
}
