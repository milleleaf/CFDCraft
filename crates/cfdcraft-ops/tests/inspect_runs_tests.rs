//! Reading the runs of a document: the list, the checks and key numbers of one run, and what it
//! was made from. All of it is data in the report, and none of it writes.

use cfdcraft_ops::{
    ExitClass, Report, Repro, ResultData, RunsData, list_runs, run_provenance, run_result,
};
use serde_json::Value;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

fn runs_of(report: &Report) -> &RunsData {
    match &report.repro {
        Some(Repro::Runs(data)) => data,
        other => panic!("expected the list of runs, found {other:?}"),
    }
}

fn result_of(report: &Report) -> &ResultData {
    match &report.repro {
        Some(Repro::RunResult(data)) => data,
        other => panic!("expected the result of a run, found {other:?}"),
    }
}

fn is_key(text: &str) -> bool {
    text.len() == 64
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// The report as the JSON a caller reads.
fn json(report: &Report) -> Value {
    serde_json::from_str(&report.to_json()).unwrap()
}

#[test]
fn the_runs_are_listed_oldest_first_with_what_each_recorded() {
    let (path, _) = two_runs("runs-list", 0.2, Vec::new());

    let done = list_runs(&path);
    passed(&done.report);
    assert_eq!(done.report.operation, "runs");
    assert!(done.line.is_none());
    let runs = &runs_of(&done.report).runs;
    assert_eq!(runs.len(), 2);

    let (first, second) = (&runs[0], &runs[1]);
    assert_eq!(first.id, "run-1");
    assert_eq!(first.recorded, ["mesh", "result"]);
    assert!(is_key(first.mesh_key.as_deref().unwrap()));
    assert!(is_key(first.result_key.as_deref().unwrap()));
    assert_eq!(first.host.as_deref(), Some("fixture-host"));
    assert_eq!(first.checks.len(), 1);
    assert_eq!(first.checks[0].name, "evidence");
    assert!(first.checks[0].passed);
    assert!(first.rerun_of.is_none() && first.verdict.is_none());

    assert_eq!(second.id, "run-2");
    assert_eq!(second.recorded, ["result"]);
    assert!(second.mesh_key.is_none());
    assert!(is_key(second.result_key.as_deref().unwrap()));
    assert_ne!(first.result_key, second.result_key);
    assert_eq!(second.host.as_deref(), Some("fixture-host"));
    assert!(second.verdict.is_none());

    let report = json(&done.report);
    assert_eq!(report["repro"]["kind"], "runs");
    assert_eq!(report["repro"]["runs"][1]["id"], "run-2");
}

#[test]
fn the_result_of_the_newest_run_is_the_default() {
    let (path, _) = two_runs("runs-newest", 0.2, Vec::new());

    let done = run_result(&path, None);
    passed(&done.report);
    assert_eq!(done.report.operation, "result");
    let data = result_of(&done.report);
    assert_eq!(data.run, "run-2");
    assert!(is_key(data.result_key.as_deref().unwrap()));
    let evidence = data.checks.iter().find(|c| c.name == "evidence").unwrap();
    assert!(evidence.passed);
    for name in [
        "solver.iterations",
        "residual.p",
        "centreline.vertical.ux.0000",
    ] {
        assert!(data.key_numbers.contains_key(name), "{name}");
    }
    assert!(data.comparison.is_none());

    let report = json(&done.report);
    assert_eq!(report["repro"]["kind"], "result");
    assert_eq!(report["repro"]["run"], "run-2");
    assert!(report["repro"]["key_numbers"]["solver.iterations"].is_number());
}

#[test]
fn a_named_run_is_returned_and_an_unknown_one_is_refused() {
    let (path, _) = two_runs("runs-named", 0.2, Vec::new());

    let first = run_result(&path, Some("run-1"));
    passed(&first.report);
    let newest = run_result(&path, None);
    let (one, two) = (result_of(&first.report), result_of(&newest.report));
    assert_eq!(one.run, "run-1");
    // The lid runs faster in the second run, so its samples sit higher.
    assert_ne!(
        one.key_numbers["centreline.vertical.ux.0000"],
        two.key_numbers["centreline.vertical.ux.0000"]
    );

    for wrong in ["run-9", "../x", "RUN-1", "-run", "run 1", ""] {
        let done = run_result(&path, Some(wrong));
        assert_eq!(done.report.class(), ExitClass::StudyInvalid, "{wrong:?}");
        assert_eq!(done.report.diagnostics.len(), 1, "{wrong:?}");
        assert_eq!(done.report.diagnostics[0].code, "CFD-REPRO-004");
        assert!(done.report.repro.is_none());
        let provenance = run_provenance(&path, Some(wrong));
        assert_eq!(provenance.report.diagnostics[0].code, "CFD-REPRO-004");
    }
    let unknown = run_result(&path, Some("run-9"));
    assert_eq!(
        unknown.report.diagnostics[0].message,
        "There is no run run-9 in this file."
    );
    let long = "a".repeat(41);
    let done = run_result(&path, Some(&long));
    assert_eq!(done.report.diagnostics[0].code, "CFD-REPRO-004");
}

#[test]
fn a_file_without_runs_says_so() {
    let path = new_document(&scratch("runs-none"), "cavity", CAVITY);

    let listed = list_runs(&path);
    passed(&listed.report);
    assert!(runs_of(&listed.report).runs.is_empty());
    assert_eq!(
        listed.line.as_ref().map(say).as_deref(),
        Some("This file has no runs yet.")
    );

    for done in [
        run_result(&path, None),
        run_provenance(&path, None),
        run_result(&path, Some("run-1")),
        run_provenance(&path, Some("run-1")),
    ] {
        assert_eq!(done.report.class(), ExitClass::StudyInvalid);
        assert_eq!(done.report.diagnostics.len(), 1);
        assert_eq!(done.report.diagnostics[0].code, "CFD-REPRO-005");
        assert_eq!(
            done.report.diagnostics[0].message,
            "This file has no runs yet."
        );
    }
}

#[test]
fn a_run_without_a_result_has_provenance_and_no_result() {
    let (path, _) = two_runs("runs-mesh-only", 0.2, Vec::new());
    add_mesh_only_run(&path);

    let listed = list_runs(&path);
    passed(&listed.report);
    let runs = &runs_of(&listed.report).runs;
    assert_eq!(runs[2].id, "run-3");
    assert_eq!(runs[2].recorded, ["mesh"]);
    assert!(runs[2].result_key.is_none());

    let named = run_result(&path, Some("run-3"));
    assert_eq!(named.report.class(), ExitClass::StudyInvalid);
    assert_eq!(named.report.diagnostics[0].code, "CFD-REPRO-005");
    assert_eq!(
        named.report.diagnostics[0].message,
        "run-3 recorded no result, so there is nothing to rerun or compare."
    );
    // The default skips a run with no result.
    assert_eq!(result_of(&run_result(&path, None).report).run, "run-2");
    // Provenance exists for every run, and the default is the newest of any kind.
    let provenance = json(&run_provenance(&path, None).report);
    assert_eq!(provenance["repro"]["run"], "run-3");
    assert!(provenance["repro"]["provenance"]["result_key"].is_null());
}

#[test]
fn the_provenance_is_returned_as_json_data() {
    let (path, _) = two_runs("runs-provenance", 0.2, Vec::new());

    let done = run_provenance(&path, Some("run-1"));
    passed(&done.report);
    assert_eq!(done.report.operation, "provenance");
    let report = json(&done.report);
    assert_eq!(report["repro"]["kind"], "provenance");
    assert_eq!(report["repro"]["run"], "run-1");
    let provenance = &report["repro"]["provenance"];
    for field in [
        "geometry_hash",
        "mesh_key",
        "mesh_hash",
        "result_key",
        "host",
    ] {
        assert!(provenance[field].is_string(), "{field}");
    }
    assert!(provenance["settings"].is_object());
    assert!(
        provenance["tool"]["image_digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    for field in ["dialect", "writer_version"] {
        assert!(provenance["tool"][field].is_string(), "tool.{field}");
    }
    assert_eq!(provenance["host"], "fixture-host");
    assert!(report["study_hash"].is_string());

    // The newest run stands on the same mesh and made another result.
    let newest = json(&run_provenance(&path, None).report);
    assert_eq!(newest["repro"]["run"], "run-2");
    assert_eq!(
        newest["repro"]["provenance"]["mesh_key"],
        provenance["mesh_key"]
    );
    assert_eq!(
        newest["repro"]["provenance"]["mesh_hash"],
        provenance["mesh_hash"]
    );
    assert_ne!(
        newest["repro"]["provenance"]["result_key"],
        provenance["result_key"]
    );
}

#[test]
fn these_operations_never_write() {
    let (path, _) = two_runs("runs-never-writes", 0.2, Vec::new());
    let before = snapshot(&path);

    passed(&list_runs(&path).report);
    passed(&run_result(&path, None).report);
    passed(&run_result(&path, Some("run-1")).report);
    passed(&run_provenance(&path, None).report);
    passed(&run_provenance(&path, Some("run-2")).report);
    assert_ne!(run_result(&path, Some("run-9")).report.exit_class, 0);
    assert_eq!(snapshot(&path), before);
}

#[test]
fn an_inconsistent_log_is_a_class_2_failure_for_every_read() {
    let (path, _) = two_runs("runs-inconsistent", 0.2, Vec::new());
    tamper_with_index(&path, "\"x\": 128,", "\"x\": 129,");
    let before = snapshot(&path);

    for done in [
        list_runs(&path),
        run_result(&path, None),
        run_provenance(&path, None),
    ] {
        assert_eq!(done.report.class(), ExitClass::StudyInvalid);
        assert_eq!(done.report.diagnostics[0].code, "CFD-REPRO-003");
    }
    assert_eq!(snapshot(&path), before);
}
