//! Acceptance of reproducible results: one named test for each claim, through the public
//! operations only, with a fixture runner standing in for the compute host. A reader can follow
//! the claims below to the behaviour that proves them. Nothing here needs a container engine,
//! a network or a clock, so the tests run on every platform.
//!
//! | Claim | Test |
//! |---|---|
//! | 1. Solving the same study twice reuses the mesh and the result and runs once | `sc1_the_same_study_twice_reuses_the_mesh_and_the_result` |
//! | 1. A changed solver image digest gives new keys and a new run | `sc1_a_changed_image_digest_gives_new_keys_and_a_new_run` |
//! | 1. A changed dialect gives new keys and a new run | `sc1_a_changed_dialect_gives_new_keys_and_a_new_run` |
//! | 1. A changed sidecar lock gives new keys and a new run | `sc1_a_changed_sidecar_lock_gives_new_keys_and_a_new_run` |
//! | 1. A changed writer version gives new keys and a new run | `sc1_a_changed_writer_version_gives_new_keys_and_a_new_run` |
//! | 2. The lid speed stales the result, with the reason, and not the mesh | `sc2_the_lid_speed_stales_the_result_with_the_reason_and_not_the_mesh` |
//! | 2. The block count stales both | `sc2_the_block_count_stales_both` |
//! | 3. Editing a solved study creates a new run and keeps the old one | `sc3_editing_a_solved_study_creates_a_new_run_and_keeps_the_old_one` |
//! | 3. Every run keeps its provenance | `sc3_every_run_keeps_its_provenance` |
//! | 4. A rerun reproduces within the stated tolerance | `sc4_a_rerun_reproduces_within_the_stated_tolerance` |
//! | 4. A rerun uses only the provenance | `sc4_a_rerun_uses_only_the_provenance` |
//! | 4. Stale items are updated one, some or all, in the chosen order | `sc4_stale_items_are_updated_one_some_or_all_in_the_chosen_order` |
//! | 4. An order that breaks a dependency is refused before anything runs | `sc4_an_order_that_breaks_a_dependency_is_refused_before_anything_runs` |
//! | 5. Results, checks and provenance are JSON | `sc5_results_checks_and_provenance_are_json` |
//! | The pack carries the history and can drop the host | `the_pack_carries_the_history_and_can_drop_the_host` |

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_backend_esi::Dialect;
use cfdcraft_compare::{Tolerance, Verdict};
use cfdcraft_doc::{open, store_dir};
use cfdcraft_model::Diagnostic;
use cfdcraft_ops::documents::pack_document_with;
use cfdcraft_ops::jobs::plan_job_with_tool;
use cfdcraft_ops::{
    ComparisonData, DocOutcome, ExitClass, JobAction, Report, Repro, StatusData, compare_runs,
    document_status, list_runs, plan_job, run_provenance, run_result, save_document,
    solve_document, tool_identity, unpack_document, update_documents,
};
use cfdcraft_runs::ToolIdentity;
use cfdcraft_store::Store;
use cfdcraft_text::Line;
use serde_json::Value;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn json(report: &Report) -> Value {
    serde_json::from_str(&report.to_json()).unwrap()
}

fn status_of(report: &Report) -> &StatusData {
    match &report.repro {
        Some(Repro::Status(data)) => data,
        other => panic!("expected the status of a document, found {other:?}"),
    }
}

fn comparison_of(report: &Report) -> &ComparisonData {
    match &report.repro {
        Some(Repro::Comparison(data)) => data,
        other => panic!("expected a comparison, found {other:?}"),
    }
}

/// The stale warnings of a report, in order.
fn stale_warnings(report: &Report) -> Vec<&Diagnostic> {
    report
        .diagnostics
        .iter()
        .filter(|d| d.code == "CFD-REPRO-001")
        .collect()
}

fn with_cells_x(cells: u64) -> String {
    edit(CAVITY, |study| {
        study["mesh"]["cells_x"] = Value::from(cells)
    })
}

/// Solve the document with the tools `tool` stands for, as a build with those tools would.
fn solve_under(path: &Path, runner: &mut FixtureRunner, tool: &ToolIdentity) -> DocOutcome {
    match plan_job_with_tool(path, JobAction::Solve, tool.clone()) {
        Ok(job) => job.run_with(runner),
        Err(outcome) => *outcome,
    }
}

/// Plan and carry out a rerun of `run`.
fn rerun(path: &Path, run: &str, runner: &mut FixtureRunner, tolerance: Tolerance) -> DocOutcome {
    let action = JobAction::Rerun {
        run: run.to_owned(),
        tolerance,
        allow_tool_change: false,
    };
    match plan_job(path, action) {
        Ok(job) => job.run_with(runner),
        Err(outcome) => *outcome,
    }
}

/// Solve a document under the current tools, then under tools that differ from them by what
/// `change` does to the identity. The second solve must run the case again and record a mesh and
/// a result under keys of their own, leaving the first run as it was.
fn a_changed_tool_gives_new_keys(tag: &str, change: impl FnOnce(&mut ToolIdentity)) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let current = tool_identity(Dialect::EsiV2512);
    let mut other = current.clone();
    change(&mut other);
    assert_ne!(other, current);

    passed(&solve_under(&path, &mut runner, &current).report);
    assert_eq!(runner.calls, 1);
    let first = open(&path).unwrap().document.artifacts.runs[0].clone();

    let done = solve_under(&path, &mut runner, &other);
    passed(&done.report);
    assert_eq!(runner.calls, 2);
    assert_eq!(data(&done.report).recorded, ["mesh", "result"]);
    assert!(data(&done.report).reused.is_empty());

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0], first);
    let second = &runs[1];
    assert_eq!(second.id, "run-2");
    assert_eq!(second.provenance.tool, other);
    let (old_mesh, new_mesh) = (first.mesh.as_ref().unwrap(), second.mesh.as_ref().unwrap());
    let (old_result, new_result) = (
        first.result.as_ref().unwrap(),
        second.result.as_ref().unwrap(),
    );
    assert_ne!(old_mesh.key, new_mesh.key);
    assert_ne!(old_result.key, new_result.key);

    // Each set of tools has its own mesh and result now, so neither solves again.
    let before = fs::read(&path).unwrap();
    for tool in [&other, &current] {
        let again = solve_under(&path, &mut runner, tool);
        passed(&again.report);
        assert_eq!(again.line, Some(Line::NothingToRun));
    }
    assert_eq!(runner.calls, 2);
    assert_eq!(fs::read(&path).unwrap(), before);
}

// ---------------------------------------------------------------------------------------------
// Claim 1: the same inputs are reused, a changed tool is not
// ---------------------------------------------------------------------------------------------

#[test]
fn sc1_the_same_study_twice_reuses_the_mesh_and_the_result() {
    let dir = scratch("sc1-twice");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));

    passed(&solve_document(&path, &mut runner).report);
    assert_eq!(runner.calls, 1);
    let before = fs::read(&path).unwrap();

    let again = solve_document(&path, &mut runner);
    passed(&again.report);
    assert_eq!(runner.calls, 1);
    assert_eq!(again.line, Some(Line::NothingToRun));
    assert_eq!(data(&again.report).reused, ["mesh", "result"]);
    assert!(data(&again.report).recorded.is_empty());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(open(&path).unwrap().document.artifacts.runs.len(), 1);
}

#[test]
fn sc1_a_changed_image_digest_gives_new_keys_and_a_new_run() {
    a_changed_tool_gives_new_keys("sc1-image", |tool| {
        tool.image_digest = "sha256:0000000000000000".to_owned();
    });
}

#[test]
fn sc1_a_changed_dialect_gives_new_keys_and_a_new_run() {
    a_changed_tool_gives_new_keys("sc1-dialect", |tool| {
        tool.dialect = "foundation-v14".to_owned();
    });
}

#[test]
fn sc1_a_changed_sidecar_lock_gives_new_keys_and_a_new_run() {
    a_changed_tool_gives_new_keys("sc1-sidecar", |tool| {
        tool.sidecar_lock = Some("lock-1".to_owned());
    });
}

#[test]
fn sc1_a_changed_writer_version_gives_new_keys_and_a_new_run() {
    a_changed_tool_gives_new_keys("sc1-writer", |tool| {
        tool.writer_version = "0.0.0-other".to_owned();
    });
}

// ---------------------------------------------------------------------------------------------
// Claim 2: a changed input marks the downstream work stale, with the reason
// ---------------------------------------------------------------------------------------------

/// A document made from the cavity and solved once.
fn solved(tag: &str) -> (PathBuf, FixtureRunner) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    (path, runner)
}

#[test]
fn sc2_the_lid_speed_stales_the_result_with_the_reason_and_not_the_mesh() {
    let (path, _) = solved("sc2-lid");
    passed(&save_document(&with_lid_speed(0.2), &path, true).report);

    let done = document_status(&path);
    passed(&done.report);
    assert_eq!(done.report.exit_class, 0);
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "fresh");
    assert_eq!(status.result.state, "stale");

    let stale = stale_warnings(&done.report);
    assert_eq!(stale.len(), 1, "{}", done.report.render_text());
    assert_eq!(
        stale[0].message,
        "result stale because lid velocity 0.1 → 0.2 m/s"
    );
}

#[test]
fn sc2_the_block_count_stales_both() {
    let (path, _) = solved("sc2-cells");
    passed(&save_document(&with_cells_x(256), &path, true).report);

    let done = document_status(&path);
    passed(&done.report);
    let messages: Vec<&str> = stale_warnings(&done.report)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "mesh stale because cells in x 128 → 256",
            "result stale because its mesh changed"
        ]
    );
    let status = status_of(&done.report);
    assert_eq!(status.mesh.state, "stale");
    assert_eq!(status.result.state, "stale");
}

// ---------------------------------------------------------------------------------------------
// Claim 3: runs are immutable and keep their provenance
// ---------------------------------------------------------------------------------------------

#[test]
fn sc3_editing_a_solved_study_creates_a_new_run_and_keeps_the_old_one() {
    let (path, mut runner) = solved("sc3-edit");
    let first = open(&path).unwrap().document.artifacts.runs[0].clone();
    let first_text = serde_json::to_string(&first).unwrap();

    passed(&save_document(&with_lid_speed(0.2), &path, true).report);
    passed(&solve_document(&path, &mut runner).report);
    assert_eq!(runner.calls, 2);

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(serde_json::to_string(&runs[0]).unwrap(), first_text);
    assert_eq!(runs[0], first);

    let kept = first.result.as_ref().unwrap();
    let store = Store::open(store_dir(&path)).unwrap();
    assert!(store.contains(&kept.blob));
    store.verify(&kept.blob).unwrap();

    let second = &runs[1];
    assert_eq!(second.id, "run-2");
    assert_ne!(second.id, first.id);
    let made = second.result.as_ref().expect("the new run has a result");
    assert_ne!(made.key, kept.key);
    assert_ne!(made.blob, kept.blob);
    store.verify(&made.blob).unwrap();
}

/// The speed of the lid as the settings of a run record it.
fn recorded_lid_speed(run: &cfdcraft_runs::RunRecord) -> Option<f64> {
    serde_json::to_value(&run.provenance.settings).unwrap()["boundary_conditions"]["lid"]["velocity"]
        ["value"]
        .as_f64()
}

#[test]
fn sc3_every_run_keeps_its_provenance() {
    let (path, mut runner) = solved("sc3-provenance");
    let study_one = open(&path).unwrap().document.study;
    passed(&save_document(&with_lid_speed(0.2), &path, true).report);
    let study_two = open(&path).unwrap().document.study;
    passed(&solve_document(&path, &mut runner).report);
    assert_ne!(study_one, study_two);

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 2);
    let first_mesh = runs[0].mesh.as_ref().unwrap();
    for (run, study, lid) in [(&runs[0], &study_one, 0.1), (&runs[1], &study_two, 0.2)] {
        let provenance = &run.provenance;
        let geometry = provenance.geometry_hash.to_string();
        assert_eq!(geometry.len(), 64, "{}", run.id);
        assert!(
            geometry.bytes().all(|b| b.is_ascii_hexdigit()),
            "{}",
            run.id
        );
        // Both runs stand on the one mesh the first run made.
        assert_eq!(provenance.mesh_key, first_mesh.key, "{}", run.id);
        assert_eq!(provenance.mesh_hash, first_mesh.blob, "{}", run.id);
        assert_eq!(
            provenance.result_key,
            Some(run.result.as_ref().unwrap().key),
            "{}",
            run.id
        );
        assert_eq!(&provenance.settings, study, "{}", run.id);
        assert_eq!(recorded_lid_speed(run), Some(lid), "{}", run.id);
        assert!(
            provenance.tool.image_digest.starts_with("sha256:"),
            "{}",
            run.id
        );
        assert_eq!(provenance.tool.dialect, "esi-v2512");
        assert_eq!(provenance.tool.writer_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(provenance.host.as_deref(), Some("fixture-host"));
    }
}

// ---------------------------------------------------------------------------------------------
// Claim 4: rerun from provenance, compare, and update in an order
// ---------------------------------------------------------------------------------------------

#[test]
fn sc4_a_rerun_reproduces_within_the_stated_tolerance() {
    // The case is run five times: the original, then four reruns of it with these shifts.
    let dir = scratch("sc4-tolerance");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = vec![0.0, 0.0, 1e-12, 0.1];
    passed(&solve_document(&path, &mut runner).report);
    let tight = Tolerance::same_image();

    let same = rerun(&path, "run-1", &mut runner, tight);
    passed(&same.report);
    assert_eq!(
        comparison_of(&same.report).comparison.verdict,
        Verdict::Reproduced
    );
    let report = json(&same.report);
    let tolerance = &report["repro"]["comparison"]["tolerance"];
    assert_eq!(tolerance["abs"].as_f64(), Some(1e-12));
    assert_eq!(tolerance["rel"].as_f64(), Some(1e-9));

    let near = rerun(&path, "run-1", &mut runner, tight);
    passed(&near.report);
    assert_eq!(
        comparison_of(&near.report).comparison.verdict,
        Verdict::WithinTolerance
    );

    let far = rerun(&path, "run-1", &mut runner, tight);
    assert_eq!(far.report.class(), ExitClass::GateFailed);
    assert_eq!(far.report.exit_class, 1);
    assert_eq!(
        comparison_of(&far.report).comparison.verdict,
        Verdict::Differs
    );

    let loose = Tolerance::new(0.0, 1.0).unwrap();
    let accepted = rerun(&path, "run-1", &mut runner, loose);
    passed(&accepted.report);
    let data = comparison_of(&accepted.report);
    assert_ne!(data.comparison.verdict, Verdict::Differs);
    assert_eq!(data.comparison.tolerance, loose);

    // Every rerun is a run of its own, whatever it found.
    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 5);
    assert!(
        runs[1..]
            .iter()
            .all(|r| r.rerun_of.as_deref() == Some("run-1"))
    );
}

#[test]
fn sc4_a_rerun_uses_only_the_provenance() {
    let (path, mut runner) = solved("sc4-provenance");
    let recorded = open(&path).unwrap().document.artifacts.runs[0]
        .provenance
        .settings
        .clone();
    passed(&save_document(&with_lid_speed(0.5), &path, true).report);
    let edited = open(&path).unwrap().document.study;
    assert_ne!(edited, recorded);

    passed(&rerun(&path, "run-1", &mut runner, Tolerance::same_image()).report);
    assert_eq!(runner.calls, 2);
    assert_eq!(
        runner.last_study.as_deref(),
        Some(serde_json::to_string_pretty(&recorded).unwrap().as_str())
    );
    assert_ne!(
        runner.last_study.as_deref(),
        Some(serde_json::to_string_pretty(&edited).unwrap().as_str())
    );
}

/// Documents `a`, `b` and `c`, each solved once and then given a finer mesh, so both stages of all
/// three are out of date.
fn stale_documents(tag: &str) -> (Vec<PathBuf>, PathBuf) {
    let dir = scratch(tag);
    let mut setup = FixtureRunner::new(&dir.join("setup-runs"));
    let mut paths = Vec::new();
    for name in ["a", "b", "c"] {
        let path = new_document(&dir, name, CAVITY);
        passed(&solve_document(&path, &mut setup).report);
        passed(&save_document(&with_cells_x(256), &path, true).report);
        paths.push(path);
    }
    (paths, dir)
}

fn texts(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// The items an update made, in order.
fn made(report: &Report) -> Vec<&str> {
    match &report.repro {
        Some(Repro::Update(data)) => data.updated.iter().map(|m| m.item.as_str()).collect(),
        other => panic!("expected the data of an update, found {other:?}"),
    }
}

fn snapshots(paths: &[PathBuf]) -> Vec<(Vec<u8>, Vec<String>)> {
    paths.iter().map(|path| snapshot(path)).collect()
}

#[test]
fn sc4_stale_items_are_updated_one_some_or_all_in_the_chosen_order() {
    let (paths, dir) = stale_documents("sc4-update");
    let untouched = [snapshot(&paths[0]), snapshot(&paths[2])];

    // One item.
    let mut runner = FixtureRunner::new(&dir.join("one"));
    let select = texts(&["b:mesh"]);
    let done = update_documents(&paths, Some(select.as_slice()), None, &mut runner);
    passed(&done.report);
    assert_eq!(made(&done.report), ["b:mesh"]);
    assert_eq!(runner.calls, 1);
    assert_eq!([snapshot(&paths[0]), snapshot(&paths[2])], untouched);

    // Some items, in the order chosen rather than the order of the files.
    let mut runner = FixtureRunner::new(&dir.join("some"));
    let select = texts(&["a:mesh", "c:mesh"]);
    let order = texts(&["c:mesh", "a:mesh"]);
    let done = update_documents(
        &paths,
        Some(select.as_slice()),
        Some(order.as_slice()),
        &mut runner,
    );
    passed(&done.report);
    assert_eq!(made(&done.report), ["c:mesh", "a:mesh"]);
    assert_eq!(runner.calls, 2);

    // All that is left, in dependency order, with the case run once for each document.
    let mut runner = FixtureRunner::new(&dir.join("all"));
    let done = update_documents(&paths, None, None, &mut runner);
    passed(&done.report);
    assert_eq!(made(&done.report), ["a:result", "b:result", "c:result"]);
    assert_eq!(runner.calls, 3);

    for path in &paths {
        let status = document_status(path);
        assert_eq!(status_of(&status.report).mesh.state, "fresh");
        assert_eq!(status_of(&status.report).result.state, "fresh");
    }
}

#[test]
fn sc4_an_order_that_breaks_a_dependency_is_refused_before_anything_runs() {
    let (paths, dir) = stale_documents("sc4-refused");
    let before = snapshots(&paths);
    let mut runner = FixtureRunner::new(&dir.join("update-runs"));

    // The result of a document before its mesh.
    let backwards = texts(&[
        "a:result", "a:mesh", "b:mesh", "b:result", "c:mesh", "c:result",
    ]);
    let done = update_documents(&paths, None, Some(backwards.as_slice()), &mut runner);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    assert_eq!(done.report.diagnostics.len(), 1);
    assert_eq!(done.report.diagnostics[0].code, "CFD-REPRO-008");
    assert_eq!(
        done.report.diagnostics[0].message,
        "a:result cannot come before a:mesh, which it is made from."
    );

    // A result alone while its mesh is still waiting.
    let select = texts(&["a:result"]);
    let done = update_documents(&paths, Some(select.as_slice()), None, &mut runner);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    assert_eq!(done.report.diagnostics.len(), 1);
    assert_eq!(done.report.diagnostics[0].code, "CFD-REPRO-008");
    assert_eq!(
        done.report.diagnostics[0].message,
        "a:result is made from a:mesh, which is also out of date; update a:mesh too, and first."
    );

    assert_eq!(runner.calls, 0);
    assert_eq!(snapshots(&paths), before);
}

// ---------------------------------------------------------------------------------------------
// Claim 5: JSON from the operations
// ---------------------------------------------------------------------------------------------

#[test]
fn sc5_results_checks_and_provenance_are_json() {
    let (path, _) = two_runs("sc5-json", 0.2, Vec::new());

    let runs = list_runs(&path);
    let result = run_result(&path, Some("run-1"));
    let provenance = run_provenance(&path, Some("run-1"));
    let comparison = compare_runs(&path, "run-1", "run-2", Tolerance::same_image());
    let status = document_status(&path);
    let all = [
        (&runs, "runs"),
        (&result, "result"),
        (&provenance, "provenance"),
        (&comparison, "comparison"),
        (&status, "status"),
    ];
    for (done, kind) in all {
        let report = json(&done.report);
        assert_eq!(report["repro"]["kind"], kind, "{kind}");
        assert_eq!(report["exit_class"], done.report.exit_class, "{kind}");
    }

    let report = json(&result.report);
    let checks = report["repro"]["checks"].as_array().unwrap();
    let evidence = checks
        .iter()
        .find(|c| c["name"] == "evidence")
        .expect("the evidence verdict is a check");
    assert_eq!(evidence["passed"], true);
    assert!(report["repro"]["key_numbers"]["solver.iterations"].is_number());
    assert!(report["repro"]["key_numbers"]["residual.p"].is_number());

    let report = json(&provenance.report);
    let recorded = &report["repro"]["provenance"];
    for field in ["geometry_hash", "mesh_key", "mesh_hash", "result_key"] {
        assert_eq!(recorded[field].as_str().map(str::len), Some(64), "{field}");
    }
    assert!(recorded["settings"].is_object());
    assert!(
        recorded["tool"]["image_digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(recorded["tool"]["dialect"], "esi-v2512");
    assert_eq!(recorded["host"], "fixture-host");

    // The two runs differ in the lid, so the comparison is a finding with data, not a failure to
    // answer.
    assert_eq!(comparison.report.class(), ExitClass::GateFailed);
    assert_eq!(
        comparison_of(&comparison.report).comparison.verdict,
        Verdict::Differs
    );
}

// ---------------------------------------------------------------------------------------------
// The pack carries the history
// ---------------------------------------------------------------------------------------------

#[test]
fn the_pack_carries_the_history_and_can_drop_the_host() {
    let (path, _) = two_runs("pack-history", 0.2, Vec::new());
    let dir = path.parent().unwrap().to_owned();
    let original = open(&path).unwrap().document;
    assert_eq!(original.artifacts.runs.len(), 2);

    for (redact, name) in [(false, "kept"), (true, "dropped")] {
        let pack = dir.join(format!("{name}.cfdpack"));
        passed(&pack_document_with(&path, &pack, redact).report);
        let unpacked = dir.join(format!("unpacked-{name}"));
        passed(&unpack_document(&pack, &unpacked).report);
        let target = unpacked.join(format!("{name}.cfdcraft"));

        let document = open(&target).unwrap().document;
        assert_eq!(document.artifacts.runs.len(), 2, "{name}");
        for (run, source) in document.artifacts.runs.iter().zip(&original.artifacts.runs) {
            let mut expected = source.clone();
            if redact {
                expected.provenance.host = None;
            }
            assert_eq!(run, &expected, "{name}");
        }

        let store = Store::open(store_dir(&target)).unwrap();
        for run in &document.artifacts.runs {
            for used in run.blob_uses() {
                store.verify(&used.hash).unwrap();
            }
        }
    }
}
