//! A clean synthetic case becomes an original, a working copy and a record.

mod support;

use cfdcraft_import::{
    FORMAT, FindingKind, ImportError, ImportOptions, ImportOutcome, Record, import_case,
    import_case_hooked, verify_import,
};
use std::fs;
use std::io;
use std::path::PathBuf;
use support::{Scratch, file_names, put, snapshot, write_clean_case, write_dialect_case};

fn imported(s: &Scratch) -> Record {
    match import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap() {
        ImportOutcome::Imported(record) => record,
        other => panic!("the case was not imported: {other:?}"),
    }
}

#[test]
fn a_clean_case_is_imported_with_original_and_work() {
    let s = Scratch::new("clean");
    write_clean_case(&s.source());
    let before = snapshot(&s.source());

    let record = imported(&s);
    assert_eq!(record.format, FORMAT);

    let target = s.target();
    assert!(target.join("original").is_dir());
    assert!(target.join("work").is_dir());
    assert!(target.join("import.json").is_file());

    let original = file_names(&target.join("original"));
    assert_eq!(original, file_names(&target.join("work")));
    assert_eq!(original.len(), before.len());
    assert!(original.contains("system/controlDict"));
    assert!(original.contains("0/U"));

    assert_eq!(snapshot(&target.join("original")), before);
    assert_eq!(snapshot(&target.join("work")), before);

    for rel in &original {
        let meta = fs::metadata(target.join("original").join(rel)).unwrap();
        assert!(meta.permissions().readonly(), "{rel} is not read-only");
    }
    assert_eq!(snapshot(&s.source()), before, "the source changed");
}

#[test]
fn the_record_holds_hashes_and_an_inventory() {
    let s = Scratch::new("record");
    write_clean_case(&s.source());
    let written = imported(&s);

    let text = fs::read_to_string(s.target().join("import.json")).unwrap();
    let record: Record = serde_json::from_str(&text).unwrap();
    assert_eq!(record, written);
    assert_eq!(record.source_name, "source");

    let originals = snapshot(&s.target().join("original"));
    assert_eq!(record.files.len(), originals.len());
    let paths: Vec<&str> = record.files.iter().map(|f| f.rel_path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted, "the file list is not sorted");
    for file in &record.files {
        let bytes = &originals[&file.rel_path];
        assert_eq!(file.bytes, bytes.len() as u64);
        assert_eq!(file.blake3, blake3::hash(bytes).to_hex().to_string());
        assert_eq!(file.blake3.len(), 64);
    }

    let inv = &record.inventory;
    assert_eq!(inv.dialect, "foundation-14");
    assert_eq!(
        inv.application_or_solver.as_deref(),
        Some("incompressibleFluid")
    );
    assert_eq!(inv.turbulence_model.as_deref(), Some("RAS"));
    assert_eq!(inv.turbulence_detail.as_deref(), Some("kEpsilon"));
    assert_eq!(inv.mesh_kind, "blockMesh");
    assert_eq!(
        inv.boundary_condition_kinds,
        ["empty", "fixedValue", "noSlip", "zeroGradient"]
    );
    assert_eq!(inv.time_settings.start_from.as_deref(), Some("startTime"));
    assert_eq!(inv.time_settings.start_time.as_deref(), Some("0"));
    assert_eq!(inv.time_settings.stop_at.as_deref(), Some("endTime"));
    assert_eq!(inv.time_settings.end_time.as_deref(), Some("0.5"));
    assert_eq!(inv.time_settings.delta_t.as_deref(), Some("0.005"));
    assert_eq!(inv.time_settings.write_control.as_deref(), Some("timeStep"));
    assert_eq!(inv.time_settings.write_interval.as_deref(), Some("20"));
    assert_eq!(inv.function_objects, ["probes"]);
    assert!(inv.unparsed.is_empty());
    assert!(record.refusals.is_empty());
    assert!(record.scripts.is_empty());
    assert!(record.skipped.is_empty());
}

#[test]
fn results_folders_are_not_copied_by_default() {
    let s = Scratch::new("results");
    write_clean_case(&s.source());
    put(&s.source(), "100/U", "a 1;\n");
    put(&s.source(), "0.5/p", "a 1;\n");
    put(&s.source(), "processor0/0/U", "a 1;\n");
    put(&s.source(), "processor1/0/U", "a 1;\n");
    put(&s.source(), "postProcessing/probes/0/p", "0 1\n");
    put(&s.source(), "log.blockMesh", "End\n");
    put(&s.source(), "log.simpleFoam", "End\n");

    let record = imported(&s);
    let kept = file_names(&s.target().join("original"));
    for rel in &kept {
        assert!(
            !rel.starts_with("100/")
                && !rel.starts_with("0.5/")
                && !rel.starts_with("processor")
                && !rel.starts_with("postProcessing")
                && !rel.starts_with("log."),
            "{rel} was copied"
        );
    }
    assert!(kept.contains("0/U"));
    assert_eq!(file_names(&s.target().join("work")), kept);
    let counts: Vec<(&str, u64)> = record
        .skipped
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(
        counts,
        [
            ("log", 2),
            ("post_processing", 1),
            ("processor", 2),
            ("time", 2)
        ]
    );
}

#[test]
fn import_json_is_written_last_and_atomically() {
    let s = Scratch::new("order");
    write_clean_case(&s.source());
    let mut written: Vec<PathBuf> = Vec::new();
    let outcome = import_case_hooked(
        &s.source(),
        &s.target(),
        &ImportOptions::default(),
        &mut |p| {
            written.push(p.to_path_buf());
            Ok(())
        },
    )
    .unwrap();
    assert!(matches!(outcome, ImportOutcome::Imported(_)));
    let last = written.last().unwrap();
    assert!(last.ends_with("import.json"));
    assert_eq!(
        written
            .iter()
            .filter(|p| p.ends_with("import.json"))
            .count(),
        1
    );
    assert!(written[0].starts_with(s.target().join("original")));
    assert!(verify_import(&s.target(), &ImportOptions::default()).is_empty());

    // A write that fails after the first file leaves a folder with no record.
    let s = Scratch::new("halfway");
    write_clean_case(&s.source());
    let mut calls = 0;
    let err = import_case_hooked(
        &s.source(),
        &s.target(),
        &ImportOptions::default(),
        &mut |_| {
            calls += 1;
            if calls == 2 {
                Err(io::Error::other("injected"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert!(matches!(err, ImportError::Write { .. }));
    assert!(s.target().exists());
    assert!(!s.target().join("import.json").exists());
    let findings = verify_import(&s.target(), &ImportOptions::default());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::Incomplete);
}

#[test]
fn an_existing_target_and_a_bad_source_are_refused_with_their_codes() {
    let s = Scratch::new("errors");
    write_clean_case(&s.source());
    fs::create_dir_all(s.target()).unwrap();
    let err = import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap_err();
    assert!(matches!(err, ImportError::TargetExists));
    assert_eq!(err.code(), Some("CFD-IMPORT-003"));
    assert_eq!(fs::read_dir(s.target()).unwrap().count(), 0);

    let missing = s.source().join("nowhere");
    let err = import_case(&missing, &s.target().join("x"), &ImportOptions::default()).unwrap_err();
    assert_eq!(err.code(), Some("CFD-IMPORT-001"));

    let file = s.source().join("system/controlDict");
    let err = import_case(&file, &s.target().join("y"), &ImportOptions::default()).unwrap_err();
    assert_eq!(err.code(), Some("CFD-IMPORT-001"));

    let empty = Scratch::new("empty");
    put(&empty.source(), "notes.txt", "hello\n");
    let err = import_case(&empty.source(), &empty.target(), &ImportOptions::default()).unwrap_err();
    assert!(matches!(err, ImportError::NotACase));
    assert_eq!(err.code(), Some("CFD-IMPORT-004"));
    assert!(!empty.target().exists());
}

#[test]
fn a_dictionary_with_a_header_makes_a_case_without_a_control_dictionary() {
    let s = Scratch::new("header");
    put(
        &s.source(),
        "constant/transportProperties",
        &support::dict("transportProperties", "nu 0.01;"),
    );
    assert!(matches!(
        import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap(),
        ImportOutcome::Imported(_)
    ));
}

#[test]
fn the_dialect_is_detected_from_the_files() {
    let cases: [(&str, &[&str], &str); 5] = [
        (
            "solver",
            &["physicalProperties", "momentumTransport"],
            "foundation-14",
        ),
        (
            "application",
            &["transportProperties", "momentumTransport"],
            "foundation-legacy",
        ),
        (
            "application",
            &["transportProperties", "turbulenceProperties"],
            "esi",
        ),
        ("application", &["transportProperties"], "unknown"),
        (
            "solver",
            &["transportProperties", "turbulenceProperties"],
            "unknown",
        ),
    ];
    for (i, (key, files, expected)) in cases.iter().enumerate() {
        let s = Scratch::new(&format!("dialect{i}"));
        write_dialect_case(&s.source(), key, files);
        let record = imported(&s);
        assert_eq!(record.inventory.dialect, *expected, "case {i}");
        assert_eq!(
            record.inventory.application_or_solver.as_deref(),
            Some("someSolver")
        );
    }
}

#[test]
fn a_dictionary_that_does_not_parse_is_listed_not_fatal() {
    let s = Scratch::new("unparsed");
    write_clean_case(&s.source());
    put(&s.source(), "0/broken", "a { b 1;\n");
    let record = imported(&s);
    assert_eq!(record.inventory.unparsed, ["0/broken"]);
    assert!(file_names(&s.target().join("original")).contains("0/broken"));
}
