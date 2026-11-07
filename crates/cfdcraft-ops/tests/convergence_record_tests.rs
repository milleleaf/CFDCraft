//! The verdict a recorded result carries, and the functions that make it from the stored files.
//! A run that is not converged is recorded all the same; the word converged is never made from
//! residuals alone. No test needs a compute host or a container engine.

use std::fs;
use std::path::Path;

use cfdcraft_backend_esi::Dialect;
use cfdcraft_doc::{open, store_dir};
use cfdcraft_model::{SettleSettings, Study};
use cfdcraft_ops::convergence::{files_from_bundle, files_from_dir, settings_for, spec_for};
use cfdcraft_ops::run::judge_results;
use cfdcraft_ops::{ExitClass, Repro, RunMode, prepare, run_result, solve_document};
use cfdcraft_settle::{BalanceStatus, Settings, VerdictState};
use cfdcraft_store::Store;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const CHANNEL: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn resolved(study: &str) -> cfdcraft_model::ResolvedStudy {
    prepare(study, Dialect::EsiV2512)
        .expect("the study passes the gates")
        .resolved
}

#[test]
fn a_solved_cavity_with_no_monitor_files_records_not_converged_and_still_records() {
    let dir = scratch("conv-none");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let done = solve_document(&path, &mut runner);
    passed(&done.report);

    let runs = open(&path).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, "run-1");
    assert!(runs[0].result.is_some(), "the verdict does not block");
    let verdict = runs[0].verdict.as_ref().expect("the verdict is recorded");
    assert_eq!(verdict.state, VerdictState::NotConverged);
    for name in ["probe_1.Ux", "probe_1.Uy", "probe_5.Ux"] {
        let reason = format!("not_settled:{name}");
        assert!(verdict.reasons.contains(&reason), "{:?}", verdict.reasons);
    }
}

#[test]
fn a_solved_cavity_with_settled_probe_files_records_converged() {
    let dir = scratch("conv-settled");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.monitors = true;
    passed(&solve_document(&path, &mut runner).report);

    let runs = open(&path).unwrap().document.artifacts.runs;
    let verdict = runs[0].verdict.as_ref().expect("the verdict is recorded");
    assert_eq!(verdict.state, VerdictState::Converged, "{verdict:?}");
    assert!(verdict.reasons.is_empty());
    assert!(verdict.residuals_ok);
    assert_eq!(verdict.mass.status, BalanceStatus::NotApplicable);
    assert_eq!(verdict.quantities.len(), 10);
}

#[test]
fn residuals_alone_never_record_converged() {
    let dir = scratch("conv-residuals");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);

    let runs = open(&path).unwrap().document.artifacts.runs;
    let verdict = runs[0].verdict.as_ref().expect("the verdict is recorded");
    // The residuals of the fixture are under the limits, and that is not enough.
    assert!(verdict.residuals_ok);
    assert_ne!(verdict.state, VerdictState::Converged);
}

#[test]
fn the_spec_is_built_from_the_monitor_plan_and_the_patch_table() {
    let spec = spec_for(&resolved(CAVITY));
    let names: Vec<&str> = spec.probes.iter().map(|p| p.fo.as_str()).collect();
    assert_eq!(
        names,
        ["probe_1", "probe_2", "probe_3", "probe_4", "probe_5"]
    );
    // A default probe has no label of its own and is shown by its name.
    assert!(spec.probes.iter().all(|p| p.label == p.fo));
    assert!(spec.lines.is_empty());
    assert!(spec.surfaces.is_empty());
    let patches: Vec<(&str, bool)> = spec
        .patches
        .iter()
        .map(|p| (p.fo.as_str(), p.inlet))
        .collect();
    assert_eq!(
        patches,
        [("balance_movingWall", false), ("balance_fixedWalls", false)]
    );
    assert_eq!(spec.residual_limits, Some((1e-6, 1e-6)));
    assert!(!spec.transient);
    assert!(!spec.solves_energy);

    let channel = spec_for(&resolved(CHANNEL));
    let inlet: Vec<&str> = channel
        .patches
        .iter()
        .filter(|p| p.inlet)
        .map(|p| p.fo.as_str())
        .collect();
    assert_eq!(inlet, ["balance_inlet"]);
}

#[test]
fn settings_for_reads_the_study_tolerances() {
    let (mut study, _) = Study::from_json_with_note(CAVITY).unwrap();
    assert_eq!(settings_for(&study), Settings::default());

    study.monitors.settle = Some(SettleSettings {
        spread_rel: Some(0.02),
        ..SettleSettings::default()
    });
    let expected = Settings {
        spread_rel: 0.02,
        ..Settings::default()
    };
    assert_eq!(settings_for(&study), expected);

    // A setting outside its range is not used.
    study.monitors.settle = Some(SettleSettings {
        spread_rel: Some(5.0),
        ..SettleSettings::default()
    });
    assert_eq!(settings_for(&study), Settings::default());
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn files_from_dir_reads_the_log_and_the_function_object_folders_in_time_order() {
    let dir = scratch("conv-dir");
    let spec = spec_for(&resolved(CAVITY));
    write(&dir, "log.simpleFoam", "Time = 1\nEnd\n");
    write(&dir, "postProcessing/probe_1/1200/U", "late");
    write(&dir, "postProcessing/probe_1/0/U", "first");
    write(&dir, "postProcessing/probe_1/400/U", "middle");
    // A file nobody asked for, and a function object the spec does not name.
    write(&dir, "postProcessing/probe_1/0/other", "ignored");
    write(&dir, "postProcessing/probe_9/0/U", "unknown");
    // Over the size cap: left out.
    let big = vec![b'x'; 8 * 1024 * 1024 + 1];
    fs::create_dir_all(dir.join("postProcessing/probe_2/0")).unwrap();
    fs::write(dir.join("postProcessing/probe_2/0/U"), big).unwrap();

    let files = files_from_dir(&dir, &spec);
    assert_eq!(files.log, "Time = 1\nEnd\n");
    let order: Vec<(&str, &str)> = files.monitor_files["probe_1"]
        .iter()
        .map(|(folder, text)| (folder.as_str(), text.as_str()))
        .collect();
    assert_eq!(order, [("0", "first"), ("400", "middle"), ("1200", "late")]);
    assert!(!files.monitor_files.contains_key("probe_2"));
    assert!(!files.monitor_files.contains_key("probe_9"));
}

#[test]
fn files_from_dir_keeps_the_newest_pieces_when_there_are_too_many() {
    let dir = scratch("conv-newest");
    let spec = spec_for(&resolved(CAVITY));
    for folder in 1..=205 {
        write(
            &dir,
            &format!("postProcessing/probe_1/{folder}/U"),
            &format!("t{folder}"),
        );
    }
    let files = files_from_dir(&dir, &spec);
    let pieces = &files.monitor_files["probe_1"];
    assert_eq!(pieces.len(), 200);
    assert_eq!(pieces[0], ("6".to_owned(), "t6".to_owned()));
    assert_eq!(pieces[199], ("205".to_owned(), "t205".to_owned()));
}

#[cfg(unix)]
#[test]
fn files_from_dir_does_not_follow_a_link() {
    let dir = scratch("conv-link");
    let spec = spec_for(&resolved(CAVITY));
    write(&dir, "elsewhere/U", "secret");
    fs::create_dir_all(dir.join("postProcessing/probe_1/0")).unwrap();
    std::os::unix::fs::symlink(
        dir.join("elsewhere/U"),
        dir.join("postProcessing/probe_1/0/U"),
    )
    .unwrap();
    // A whole function-object folder that is a link is not entered either.
    std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("postProcessing/probe_2")).unwrap();
    write(&dir, "elsewhere/0/U", "also secret");

    let files = files_from_dir(&dir, &spec);
    assert!(files.monitor_files.is_empty(), "{:?}", files.monitor_files);
}

#[test]
fn files_from_bundle_reads_the_same_files_from_a_stored_result() {
    let dir = scratch("conv-bundle");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.monitors = true;
    passed(&solve_document(&path, &mut runner).report);

    let spec = spec_for(&resolved(CAVITY));
    let from_folder = files_from_dir(&dir.join("runs/call-1"), &spec);
    assert!(!from_folder.monitor_files.is_empty());

    let runs = open(&path).unwrap().document.artifacts.runs;
    let blob = runs[0].result.as_ref().unwrap().blob;
    let bytes = Store::open(store_dir(&path)).unwrap().read(blob).unwrap();
    assert_eq!(files_from_bundle(&bytes, &spec), from_folder);
}

#[test]
fn run_result_shows_the_recorded_verdict() {
    let dir = scratch("conv-result");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.monitors = true;
    passed(&solve_document(&path, &mut runner).report);

    let shown = run_result(&path, Some("run-1"));
    assert_eq!(shown.report.class(), ExitClass::Passed);
    let Some(Repro::RunResult(data)) = &shown.report.repro else {
        panic!(
            "expected the result of a run, found {:?}",
            shown.report.repro
        );
    };
    let recorded = open(&path).unwrap().document.artifacts.runs[0]
        .verdict
        .clone();
    assert!(recorded.is_some());
    assert_eq!(data.convergence, recorded);
}

#[test]
fn the_ghia_gate_still_needs_its_residuals() {
    let dir = scratch("conv-ghia");
    let results = dir.join("results");
    cfdcraft_capture::fixture::write_results(
        &results,
        &cfdcraft_capture::fixture::FixtureSpec {
            end_time: 5000,
            cells: 16384,
            samples: 64,
            shift: 0.0,
        },
    )
    .unwrap();
    let study = resolved(CAVITY);
    assert_eq!(
        judge_results(&results, &study, RunMode::Validate).verdict,
        "passed"
    );

    // The same run with residuals over the limits is not accepted by the attached rule.
    let log = fs::read_to_string(results.join("log.simpleFoam")).unwrap();
    fs::write(
        results.join("log.simpleFoam"),
        log.replace("Initial residual = 2e-07", "Initial residual = 2e-03"),
    )
    .unwrap();
    let judged = judge_results(&results, &study, RunMode::Validate);
    assert_eq!(judged.verdict, "failed");
    assert!(
        judged.reasons.iter().any(|r| r.contains("exceeds")),
        "{:?}",
        judged.reasons
    );
}
