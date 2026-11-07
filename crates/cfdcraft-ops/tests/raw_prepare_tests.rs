//! A raw override through the gate chain: it reaches the written case, the safety scan and the
//! read-back check still run over it, one that cannot be applied is a finding, and the stage keys
//! follow it.

use cfdcraft_backend_esi::{CaseTree, Dialect, WRITER_SOURCES};
use cfdcraft_ops::{
    ExitClass, Report, prepare, prepare::prepare_with_tamper, stage_keys, tool_identity,
};
use serde_json::{Value, json};
use std::path::Path;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

/// The cavity with these raw overrides, given as (label, backend, file, entry, value).
fn cavity_with(overrides: &[(&str, &str, &str, &str, &str)]) -> String {
    let mut study: Value = serde_json::from_str(CAVITY).unwrap();
    let mut map = serde_json::Map::new();
    for (label, backend, file, entry, value) in overrides {
        map.insert(
            (*label).to_owned(),
            json!({ "backend": backend, "file": file, "entry": entry, "value": value }),
        );
    }
    study["raw_overrides"] = Value::Object(map);
    study.to_string()
}

fn relax_u() -> String {
    cavity_with(&[(
        "relax-u",
        "esi",
        "system/fvSolution",
        "relaxationFactors/equations/U",
        "0.5",
    )])
}

fn failed(result: Result<cfdcraft_ops::Prepared, Report>) -> Report {
    result.expect_err("the chain should have stopped")
}

fn text<'a>(tree: &'a CaseTree, name: &str) -> &'a str {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    std::str::from_utf8(&file.bytes).unwrap()
}

fn artifact(report: &Report, kind: &str) -> String {
    report
        .artifacts
        .iter()
        .find(|a| a.kind == kind)
        .unwrap_or_else(|| panic!("no {kind} in {}", report.render_text()))
        .name
        .clone()
}

fn codes(report: &Report) -> Vec<&str> {
    report.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn prepare_applies_a_raw_override_and_still_scans_and_reads_the_case() {
    let plain = prepare(CAVITY, Dialect::EsiV2512).unwrap();
    let with = prepare(&relax_u(), Dialect::EsiV2512).unwrap();
    assert!(text(&with.tree, "system/fvSolution").contains("U 0.5;"));
    assert!(text(&with.export_tree, "system/fvSolution").contains("U 0.5;"));
    assert_ne!(with.hash, plain.hash);

    // The scan still runs over the edited case: a refused construct added after the override is
    // found, so an override is no way round it.
    let report = failed(prepare_with_tamper(&relax_u(), Dialect::EsiV2512, |tree| {
        let file = tree
            .files
            .iter_mut()
            .find(|f| f.rel_path == Path::new("system/controlDict"))
            .unwrap();
        let edited = String::from_utf8(file.bytes.clone()).unwrap().replace(
            "application simpleFoam;",
            "application simpleFoam;\nx #codeStream;",
        );
        file.bytes = edited.into_bytes();
    }));
    assert_eq!(report.class(), ExitClass::GuardRefused);
    assert_eq!(report.diagnostics[0].code, "CFD-GUARD-001");
}

#[test]
fn an_override_that_cannot_be_applied_is_cfd_raw_007() {
    let study = cavity_with(&[("ghost", "esi", "system/fvOptions", "limits/x", "1")]);
    let report = failed(prepare(&study, Dialect::EsiV2512));
    assert_eq!(report.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&report), ["CFD-RAW-007"]);
    let finding = &report.diagnostics[0];
    assert_eq!(finding.pointer, "/raw_overrides/ghost");
    assert!(finding.message.contains("ghost"), "{}", finding.message);
    assert!(
        finding.message.contains("system/fvOptions"),
        "{}",
        finding.message
    );
    assert!(finding.hint.is_some());
    assert!(report.artifacts.is_empty());

    // A block the file does not have is the same finding.
    let study = cavity_with(&[("lost", "esi", "system/fvSolution", "nope/x", "1")]);
    let report = failed(prepare(&study, Dialect::EsiV2512));
    assert_eq!(codes(&report), ["CFD-RAW-007"]);
    assert_eq!(report.diagnostics[0].pointer, "/raw_overrides/lost");
}

#[test]
fn an_override_tagged_for_the_other_backend_is_cfd_raw_005() {
    let study = cavity_with(&[(
        "other",
        "foundation",
        "system/fvSolution",
        "SIMPLE/consistent",
        "yes",
    )]);
    let report = failed(prepare(&study, Dialect::EsiV2512));
    assert_eq!(report.class(), ExitClass::StudyInvalid);
    assert!(
        codes(&report).contains(&"CFD-RAW-005"),
        "{:?}",
        codes(&report)
    );
    assert!(!codes(&report).contains(&"CFD-RAW-007"));
}

#[test]
fn a_bad_value_is_refused_before_compile() {
    let study = cavity_with(&[(
        "sneaky",
        "esi",
        "system/fvSolution",
        "SIMPLE/consistent",
        "yes #include",
    )]);
    let report = failed(prepare(&study, Dialect::EsiV2512));
    assert_eq!(report.class(), ExitClass::StudyInvalid);
    assert!(
        codes(&report).contains(&"CFD-RAW-004"),
        "{:?}",
        codes(&report)
    );
    assert!(!codes(&report).contains(&"CFD-RAW-007"));
}

#[test]
fn the_keys_of_a_study_with_an_override_differ() {
    let plain = stage_keys(CAVITY);
    let relaxed = stage_keys(&relax_u());
    assert_eq!(
        relaxed.class(),
        ExitClass::Passed,
        "{}",
        relaxed.render_text()
    );
    assert_eq!(artifact(&plain, "mesh_key"), artifact(&relaxed, "mesh_key"));
    assert_ne!(
        artifact(&plain, "result_key"),
        artifact(&relaxed, "result_key")
    );
    assert_eq!(
        artifact(&plain, "geometry_hash"),
        artifact(&relaxed, "geometry_hash")
    );

    // A mesh dictionary moves the mesh key too.
    let meshed = stage_keys(&cavity_with(&[(
        "merge",
        "esi",
        "system/blockMeshDict",
        "mergePatchPairs",
        "( )",
    )]));
    assert_eq!(
        meshed.class(),
        ExitClass::Passed,
        "{}",
        meshed.render_text()
    );
    assert_ne!(artifact(&plain, "mesh_key"), artifact(&meshed, "mesh_key"));
    assert_ne!(
        artifact(&plain, "result_key"),
        artifact(&meshed, "result_key")
    );
    assert_eq!(
        artifact(&plain, "geometry_hash"),
        artifact(&meshed, "geometry_hash")
    );
}

#[test]
fn the_writer_version_moves_with_the_new_sources() {
    let tool = tool_identity(Dialect::EsiV2512);
    let (version, fingerprint) = tool.writer_version.split_once('+').unwrap();
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert_eq!(fingerprint.len(), 12);
    // The fingerprint covers the override step and the dictionary editor, not only the six files
    // the writer was made of before them. (The comparison with a fingerprint over the earlier six
    // is a unit test next to the function that makes it, in the keys module.)
    let names: Vec<&str> = WRITER_SOURCES.iter().map(|(name, _)| *name).collect();
    assert_eq!(names.len(), 8);
    assert!(names.contains(&"raw.rs"));
    assert!(names.contains(&"../../cfdcraft-dictedit/src/lib.rs"));
}
