//! The OpenCFD judge behind the backend trait, over a run held in memory: every mode, the reasons
//! it gives, and the stopped-time rule. The files are built from the study and its run plan, so a
//! clean run is clean by the same facts the judge reads.

mod plan_studies;

use cfdcraft_backend::{Backend, JudgeMode, JudgeOutcome, RunFiles};
use cfdcraft_backend_esi::{Dialect, adapter::EsiBackend};
use cfdcraft_model::ResolvedStudy;
use plan_studies::{cavity, heated};
use std::collections::BTreeMap;

/// The files of a run held in memory, by name.
#[derive(Clone)]
struct MapFiles(BTreeMap<String, String>);

impl MapFiles {
    fn put(&mut self, name: &str, text: &str) {
        self.0.insert(name.to_owned(), text.to_owned());
    }
}

impl RunFiles for MapFiles {
    fn read_text(&self, rel: &str) -> Option<String> {
        self.0.get(rel).cloned()
    }
    fn list(&self, rel_dir: &str) -> Vec<String> {
        let prefix = if rel_dir.is_empty() {
            String::new()
        } else {
            format!("{rel_dir}/")
        };
        let mut names: Vec<String> = self
            .0
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        names
    }
    fn size(&self, rel: &str) -> Option<u64> {
        self.0.get(rel).map(|t| t.len() as u64)
    }
}

fn backend() -> EsiBackend {
    EsiBackend::new(Dialect::EsiV2512)
}

const FIELD: &str = "internalField uniform 0;\n";

/// A run that reached `time`: clean logs, a mesh that matches the study, residuals far below any
/// limit, and a folder of the time holding every final field.
fn run_files(study: &ResolvedStudy, time: &str) -> MapFiles {
    let plan = backend().run_plan(study, 1);
    let mut files = MapFiles(BTreeMap::new());
    files.put("log.blockMesh", "Creating block mesh\nEnd\n");
    if plan.potential {
        files.put(&plan.judged_logs[2], "End\n");
    }
    files.put(
        "log.checkMesh",
        &format!(
            "cells: {}\nTotal volume = {}\nMesh OK.\nEnd\n",
            study.cell_count().unwrap(),
            study.meshed_volume()
        ),
    );
    files.put(
        &plan.solver_log,
        &format!(
            "Time = {time}\n\
             smoothSolver: Solving for Ux, Initial residual = 1e-12, Final residual = 1e-13, No Iterations 1\n\
             smoothSolver: Solving for Uy, Initial residual = 1e-12, Final residual = 1e-13, No Iterations 1\n\
             PCG: Solving for p, Initial residual = 1e-12, Final residual = 1e-13, No Iterations 1\n\
             End\n"
        ),
    );
    for field in &plan.final_fields {
        files.put(&format!("{time}/{field}"), FIELD);
    }
    files
}

fn study() -> ResolvedStudy {
    cavity("{}", "5000", "500", 0)
}

/// The run of the study that ended at its end time.
fn clean(study: &ResolvedStudy) -> MapFiles {
    run_files(study, &format!("{}", study.solver.end_time))
}

fn judge(files: &MapFiles, study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
    backend().judge(files, study, mode)
}

#[test]
fn a_clean_run_passes_in_run_mode() {
    let study = study();
    let outcome = judge(&clean(&study), &study, JudgeMode::Run);
    assert_eq!(outcome.verdict, "passed", "{:?}", outcome.reasons);
    assert!(outcome.reasons.is_empty());
}

#[test]
fn a_missing_end_marker_fails_naming_the_solver() {
    let study = study();
    let plan = backend().run_plan(&study, 1);
    let mut files = clean(&study);
    let log = files.read_text(&plan.solver_log).unwrap();
    files.put(&plan.solver_log, log.strip_suffix("End\n").unwrap());
    let outcome = judge(&files, &study, JudgeMode::Run);
    assert_eq!(outcome.verdict, "failed");
    assert!(
        outcome
            .reasons
            .iter()
            .any(|r| r.contains(&plan.solver) && r.contains("missing terminal End marker")),
        "{:?}",
        outcome.reasons
    );
}

#[test]
fn a_missing_log_or_folder_fails_in_run_mode() {
    let study = study();
    let empty = MapFiles(BTreeMap::new());
    let outcome = judge(&empty, &study, JudgeMode::Run);
    assert_eq!(outcome.verdict, "failed");
    assert!(
        outcome
            .reasons
            .iter()
            .any(|r| r.contains("final-time U and p field files are missing or empty")),
        "{:?}",
        outcome.reasons
    );
}

#[test]
fn smoke_mode_says_smoke_passed_and_finished_mode_says_passed() {
    let study = study();
    let plan = backend().run_plan(&study, 1);
    let good = clean(&study);
    assert_eq!(
        judge(&good, &study, JudgeMode::Smoke).verdict,
        "smoke_passed"
    );
    assert_eq!(judge(&good, &study, JudgeMode::Finished).verdict, "passed");

    // Residuals above the limit fail a run, and neither of the other two modes holds a limit.
    let mut loose = good.clone();
    let log = loose.read_text(&plan.solver_log).unwrap();
    loose.put(&plan.solver_log, &log.replace("1e-12", "1e-02"));
    let run = judge(&loose, &study, JudgeMode::Run);
    assert_eq!(run.verdict, "failed");
    assert!(run.reasons.iter().any(|r| r.contains("exceeds")));
    assert_eq!(
        judge(&loose, &study, JudgeMode::Smoke).verdict,
        "smoke_passed"
    );
    assert_eq!(judge(&loose, &study, JudgeMode::Finished).verdict, "passed");

    // A run that never ended fails in every mode.
    let mut broken = good;
    let log = broken.read_text(&plan.solver_log).unwrap();
    broken.put(&plan.solver_log, log.strip_suffix("End\n").unwrap());
    for mode in [JudgeMode::Run, JudgeMode::Smoke, JudgeMode::Finished] {
        let outcome = judge(&broken, &study, mode.clone());
        assert_eq!(outcome.verdict, "failed", "{mode:?}");
        assert!(!outcome.reasons.is_empty(), "{mode:?}");
    }
}

fn stopped(time: &str) -> JudgeMode {
    JudgeMode::Stopped {
        time: time.to_owned(),
    }
}

#[test]
fn a_stopped_run_needs_u_and_pressure_in_the_stopped_time() {
    let study = study();
    let good = run_files(&study, "30");
    let outcome = judge(&good, &study, stopped("30"));
    assert_eq!(outcome.verdict, "passed", "{:?}", outcome.reasons);
    assert!(outcome.reasons.is_empty());

    // An empty pressure field fails, and so does a missing velocity.
    let mut empty_p = good.clone();
    empty_p.put("30/p", "");
    let outcome = judge(&empty_p, &study, stopped("30"));
    assert_eq!(outcome.verdict, "failed");
    assert!(
        outcome
            .reasons
            .iter()
            .any(|r| r.contains("U and p field files of the stopped time 30")),
        "{:?}",
        outcome.reasons
    );
    let mut no_u = good.clone();
    no_u.0.remove("30/U");
    assert_eq!(judge(&no_u, &study, stopped("30")).verdict, "failed");

    // The time text becomes part of a path, so only a plain decimal number is used.
    let mut escaping = good.clone();
    escaping.put("../1/U", FIELD);
    escaping.put("../1/p", FIELD);
    for text in ["../1", "", ".", "3 0", "30/.."] {
        let outcome = judge(&escaping, &study, stopped(text));
        assert_eq!(outcome.verdict, "failed", "{text:?}");
        assert!(
            outcome.reasons.iter().any(|r| r.contains("stopped time")),
            "{text:?}: {:?}",
            outcome.reasons
        );
    }
}

#[test]
fn a_stopped_buoyant_run_needs_the_reduced_pressure() {
    let study = heated("", "1000", "100");
    let plan = backend().run_plan(&study, 1);
    assert_eq!(plan.pressure_field, "p_rgh");
    let good = run_files(&study, "30");
    assert_eq!(judge(&good, &study, stopped("30")).verdict, "passed");
    let mut without = good;
    without.put("30/p_rgh", "");
    let outcome = judge(&without, &study, stopped("30"));
    assert_eq!(outcome.verdict, "failed");
    assert!(
        outcome
            .reasons
            .iter()
            .any(|r| r.contains("U and p_rgh field files")),
        "{:?}",
        outcome.reasons
    );
}
