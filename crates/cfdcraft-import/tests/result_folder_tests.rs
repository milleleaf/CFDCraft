//! Folders an import leaves out (results, processors, logs) are not read by the guard, so a large
//! or compressed field there does not refuse the case; everything that is copied is still scanned.

mod support;

use cfdcraft_import::{ImportOptions, ImportOutcome, Record, import_case};
use std::collections::BTreeMap;
use std::fs;
use support::{Scratch, file_names, put, write_clean_case};

fn imported(s: &Scratch) -> Record {
    match import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap() {
        ImportOutcome::Imported(record) => record,
        other => panic!("the case was not imported: {other:?}"),
    }
}

fn refused_files(s: &Scratch) -> Vec<(String, String)> {
    match import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap() {
        ImportOutcome::Refused { refusals, .. } => {
            refusals.into_iter().map(|r| (r.file, r.code)).collect()
        }
        other => panic!("the case was not refused: {other:?}"),
    }
}

#[test]
fn large_and_compressed_results_do_not_refuse_the_case() {
    let s = Scratch::new("results-large");
    write_clean_case(&s.source());
    put(&s.source(), "100/U.gz", "not text");
    put(&s.source(), "100/p", "a 1;\n");
    fs::File::create(s.source().join("100/big"))
        .unwrap()
        .set_len(17 * 1024 * 1024)
        .unwrap();
    put(&s.source(), "processor0/100/p.gz", "not text");
    put(
        &s.source(),
        "postProcessing/forces/0/force.dat.gz",
        "not text",
    );
    put(&s.source(), "log.simpleFoam.gz", "not text");

    let record = imported(&s);
    let skipped: BTreeMap<&str, u64> = record
        .skipped
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(
        skipped,
        BTreeMap::from([
            ("log", 1),
            ("post_processing", 1),
            ("processor", 1),
            ("time", 3)
        ])
    );
    let stored = file_names(&s.target().join("original"));
    assert!(
        stored.iter().all(|f| !f.starts_with("100/")
            && !f.starts_with("processor")
            && !f.starts_with("postProcessing")
            && !f.starts_with("log.")),
        "{stored:?}"
    );
    assert!(stored.contains("0/U"));
}

#[test]
fn content_of_a_left_out_folder_is_never_copied_so_it_is_not_scanned() {
    let s = Scratch::new("results-content");
    write_clean_case(&s.source());
    put(&s.source(), "0.5/U", "a 1;\ntype coded;\n");
    let record = imported(&s);
    assert_eq!(record.skipped.get("time"), Some(&1));
    assert!(!file_names(&s.target().join("original")).contains("0.5/U"));
    assert!(!file_names(&s.target().join("work")).contains("0.5/U"));
}

#[test]
fn compressed_files_in_folders_that_are_copied_stay_refused() {
    for rel in [
        "constant/polyMesh/points.gz",
        "constant/transportProperties.gz",
        "system/fvSchemes.gz",
        "0/U.gz",
        "0.orig/p.gz",
        "extra/data.gz",
        "top.gz",
    ] {
        let s = Scratch::new("results-gz");
        write_clean_case(&s.source());
        put(&s.source(), rel, "not text");
        assert_eq!(
            refused_files(&s),
            [(rel.to_string(), "CFD-GUARD-007".to_string())],
            "{rel}"
        );
    }
}

#[test]
fn a_folder_that_is_copied_keeps_its_size_limit() {
    let s = Scratch::new("results-size");
    write_clean_case(&s.source());
    fs::File::create(s.source().join("0/big"))
        .unwrap()
        .set_len(17 * 1024 * 1024)
        .unwrap();
    // The copy cap is raised so the guard's own limit is the one that answers.
    let options = ImportOptions {
        max_file_bytes: 64 * 1024 * 1024,
        ..ImportOptions::default()
    };
    match import_case(&s.source(), &s.target(), &options).unwrap() {
        ImportOutcome::Refused { refusals, .. } => {
            let found: Vec<(&str, &str)> = refusals
                .iter()
                .map(|r| (r.file.as_str(), r.code.as_str()))
                .collect();
            assert_eq!(found, [("0/big", "CFD-GUARD-007")]);
        }
        other => panic!("the case was not refused: {other:?}"),
    }
}

#[test]
fn libraries_and_code_directories_are_refused_in_left_out_folders() {
    let s = Scratch::new("results-unsafe");
    write_clean_case(&s.source());
    put(&s.source(), "100/libx.so", "x");
    put(&s.source(), "processor0/libx.dylib", "x");
    put(&s.source(), "postProcessing/dynamicCode/x", "x");
    assert_eq!(
        refused_files(&s),
        [
            ("100/libx.so".to_string(), "CFD-GUARD-007".to_string()),
            (
                "postProcessing/dynamicCode".to_string(),
                "CFD-GUARD-007".to_string()
            ),
            (
                "processor0/libx.dylib".to_string(),
                "CFD-GUARD-007".to_string()
            ),
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_link_in_a_left_out_folder_is_still_refused() {
    use std::os::unix::fs::symlink;
    for place in [
        "100/link",
        "processor0/deep/link",
        "postProcessing/link",
        "log.link",
    ] {
        let s = Scratch::new("results-link");
        write_clean_case(&s.source());
        put(&s.source(), "100/U", "a 1;\n");
        put(&s.source(), "processor0/deep/U", "a 1;\n");
        put(&s.source(), "postProcessing/x", "a 1;\n");
        symlink("/etc/hostname", s.source().join(place)).unwrap();
        assert_eq!(
            refused_files(&s),
            [(place.to_string(), "CFD-GUARD-007".to_string())],
            "{place}"
        );
    }
}
