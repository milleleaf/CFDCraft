//! A stored import is checked against its record; only the original counts.

mod support;

use cfdcraft_import::{FindingKind, ImportOptions, ImportOutcome, import_case, verify_import};
use std::fs;
use support::{Scratch, make_writable, write_clean_case};

fn stored() -> Scratch {
    let s = Scratch::new("verify");
    write_clean_case(&s.source());
    let outcome = import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap();
    assert!(matches!(outcome, ImportOutcome::Imported(_)));
    s
}

fn check(s: &Scratch) -> Vec<cfdcraft_import::Finding> {
    verify_import(&s.target(), &ImportOptions::default())
}

#[test]
fn verify_finds_a_changed_original() {
    let s = stored();
    assert_eq!(check(&s), vec![]);

    // The working copy is meant to change.
    let work = s.target().join("work/system/controlDict");
    fs::write(&work, "changed\n").unwrap();
    assert_eq!(check(&s), vec![]);

    // One changed byte of the original is a finding that names the file.
    let path = s.target().join("original/system/controlDict");
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] ^= 1;
    make_writable(&path);
    fs::write(&path, bytes).unwrap();
    let findings = check(&s);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].kind, FindingKind::Changed);
    assert_eq!(findings[0].code, "CFD-IMPORT-007");
    assert_eq!(findings[0].file, "system/controlDict");
}

#[test]
fn verify_reports_a_missing_record_as_incomplete() {
    let s = stored();
    fs::remove_file(s.target().join("import.json")).unwrap();
    let findings = check(&s);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::Incomplete);
    assert_eq!(findings[0].code, "CFD-IMPORT-007");
    assert_eq!(findings[0].file, "import.json");

    fs::write(s.target().join("import.json"), "not json").unwrap();
    let findings = check(&s);
    assert_eq!(findings[0].kind, FindingKind::Incomplete);
}

#[test]
fn verify_finds_missing_and_extra_originals() {
    let s = stored();
    let gone = s.target().join("original/0/p");
    make_writable(&gone);
    fs::remove_file(&gone).unwrap();
    fs::write(s.target().join("original/added.txt"), "x\n").unwrap();
    let findings = check(&s);
    let seen: Vec<(FindingKind, &str)> =
        findings.iter().map(|f| (f.kind, f.file.as_str())).collect();
    assert_eq!(
        seen,
        [
            (FindingKind::Missing, "0/p"),
            (FindingKind::Extra, "added.txt")
        ]
    );
}

#[test]
fn verify_refuses_a_record_that_names_an_unsafe_path() {
    let s = stored();
    let path = s.target().join("import.json");
    let text = fs::read_to_string(&path).unwrap();
    let edited = text.replacen("\"system/controlDict\"", "\"../outside\"", 1);
    assert_ne!(text, edited);
    fs::write(&path, edited).unwrap();
    let findings = check(&s);
    assert!(
        findings
            .iter()
            .any(|f| f.kind == FindingKind::Unsafe && f.file == "../outside"),
        "{findings:?}"
    );
}

#[test]
fn verify_reads_with_the_caps() {
    let s = stored();
    let small = ImportOptions {
        max_file_bytes: 60,
        ..ImportOptions::default()
    };
    let findings = verify_import(&s.target(), &small);
    assert!(!findings.is_empty());
    assert!(findings.iter().all(|f| f.kind == FindingKind::Cap));
    assert!(findings.iter().all(|f| f.code == "CFD-IMPORT-006"));

    let few = ImportOptions {
        max_files: 2,
        ..ImportOptions::default()
    };
    let findings = verify_import(&s.target(), &few);
    assert!(findings.iter().any(|f| f.kind == FindingKind::Cap));
}
