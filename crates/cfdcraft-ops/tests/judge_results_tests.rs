//! How a results folder is judged for a run that reached its end without a residual limit and for
//! a run stopped on request, beside the attached rule, which keeps its residual limit. No test
//! needs a compute host or a container engine.

use std::fs;
use std::path::Path;

use cfdcraft_backend_esi::Dialect;
use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_model::ResolvedStudy;
use cfdcraft_ops::run::{judge_finished_results, judge_results, judge_stopped_results};
use cfdcraft_ops::{RunMode, prepare};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

fn cavity() -> ResolvedStudy {
    prepare(CAVITY, Dialect::EsiV2512)
        .expect("the cavity passes the gates")
        .resolved
}

/// A log in the shape the fixture uses, ending at `time`, with this last residual for each field.
fn log_with(time: u64, residual: &str, extra: &str) -> String {
    format!(
        "Time = {time}\n\
         smoothSolver: Solving for Ux, Initial residual = {residual}, Final residual = 1e-11, No Iterations 1\n\
         smoothSolver: Solving for Uy, Initial residual = {residual}, Final residual = 1e-11, No Iterations 1\n\
         PCG: Solving for p, Initial residual = {residual}, Final residual = 1e-11, No Iterations 1\n\
         ExecutionTime = 1 s ClockTime = 1 s\n\
         {extra}End\n"
    )
}

fn finished_folder(tag: &str, residual: &str) -> std::path::PathBuf {
    let dir = scratch(tag);
    let spec = FixtureSpec {
        end_time: 5000,
        cells: 16384,
        samples: 64,
        shift: 0.0,
    };
    write_results(&dir, &spec).unwrap();
    fs::write(dir.join("log.simpleFoam"), log_with(5000, residual, "")).unwrap();
    dir
}

#[test]
fn judge_finished_results_passes_a_finished_run_whose_residuals_are_above_the_limit() {
    let dir = finished_folder("judge-finished", "1e-03");
    let study = cavity();

    let finished = judge_finished_results(&dir, &study);
    assert_eq!(finished.verdict, "passed", "{:?}", finished.reasons);
    assert!(finished.reasons.is_empty());

    // The attached rule keeps its residual limit for the same folder.
    let attached = judge_results(&dir, &study, RunMode::Run);
    assert_eq!(attached.verdict, "failed");
    assert!(attached.reasons.iter().any(|r| r.contains("exceeds")));
}

#[test]
fn judge_finished_results_still_needs_the_evidence_that_the_run_finished() {
    let dir = finished_folder("judge-unfinished", "1e-03");
    // The run never reached its end time.
    fs::write(dir.join("log.simpleFoam"), log_with(3000, "1e-03", "")).unwrap();
    let judged = judge_finished_results(&dir, &cavity());
    assert_eq!(judged.verdict, "failed");
    assert!(judged.reasons.iter().any(|r| r.contains("final time")));
}

fn stopped_folder(tag: &str, time: u64) -> std::path::PathBuf {
    let dir = scratch(tag);
    write_stopped(&dir, time);
    dir
}

fn add_time_folder(dir: &Path, name: &str) {
    fs::create_dir_all(dir.join(name)).unwrap();
    fs::write(dir.join(name).join("U"), "field U\n").unwrap();
    fs::write(dir.join(name).join("p"), "field p\n").unwrap();
}

#[test]
fn judge_stopped_results_uses_the_partial_rules() {
    let study = cavity();
    let dir = stopped_folder("judge-stopped", 1200);
    let judged = judge_stopped_results(&dir, &study, "1200");
    assert_eq!(judged.verdict, "passed", "{:?}", judged.reasons);

    // A newer time folder than the one the log ended at.
    add_time_folder(&dir, "1300");
    let newer = judge_stopped_results(&dir, &study, "1200");
    assert_eq!(newer.verdict, "failed");
    assert!(
        newer.reasons.iter().any(|r| r.contains("newest time")),
        "{:?}",
        newer.reasons
    );

    // A floating-point trap in the log.
    let trapped = stopped_folder("judge-trapped", 1200);
    fs::write(
        trapped.join("log.simpleFoam"),
        log_with(1200, "1e-07", "Floating point exception (core dumped)\n"),
    )
    .unwrap();
    let judged = judge_stopped_results(&trapped, &study, "1200");
    assert_eq!(judged.verdict, "failed");
    assert!(judged.reasons.iter().any(|r| r.contains("fatal")));

    // A saved state with an empty field file.
    let empty = stopped_folder("judge-empty", 1200);
    fs::write(empty.join("1200/p"), "").unwrap();
    assert_eq!(
        judge_stopped_results(&empty, &study, "1200").verdict,
        "failed"
    );
}

#[test]
fn a_time_that_is_not_a_plain_number_is_never_used_as_a_path() {
    let study = cavity();
    let dir = stopped_folder("judge-path", 1200);
    let judged = judge_stopped_results(&dir, &study, "../1200");
    assert_eq!(judged.verdict, "failed");
}
