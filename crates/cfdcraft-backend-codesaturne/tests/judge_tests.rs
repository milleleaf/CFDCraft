//! The judge of a finished run, over a run held in memory. Every piece of evidence the judge reads
//! is built from the study, so a clean run is clean by the same facts the judge checks.

mod common;

use cfdcraft_backend::{JudgeMode, JudgeOutcome, RunFiles};
use cfdcraft_backend_codesaturne::{markers::*, models::run_of, probes::probe_set};
use cfdcraft_model::ResolvedStudy;
use cfdcraft_templates::{MeshPreset, TimeChoice};
use common::{backend, cavity};
use std::collections::{BTreeMap, BTreeSet};

/// The files of a run held in memory, by name. A size can be stated apart from the text, so a file
/// longer than the read limit needs no big text.
#[derive(Clone, Default)]
struct MapFiles {
    files: BTreeMap<String, String>,
    sizes: BTreeMap<String, u64>,
}

impl MapFiles {
    fn put(&mut self, name: &str, text: &str) {
        self.files.insert(name.to_owned(), text.to_owned());
    }

    fn remove(&mut self, name: &str) {
        self.files.remove(name);
    }

    fn state_size(&mut self, name: &str, size: u64) {
        self.sizes.insert(name.to_owned(), size);
    }
}

impl RunFiles for MapFiles {
    fn read_text(&self, rel: &str) -> Option<String> {
        self.files.get(rel).cloned()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        let prefix = if rel_dir.is_empty() {
            String::new()
        } else {
            format!("{rel_dir}/")
        };
        let names: BTreeSet<String> = self
            .files
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_owned)
            .collect();
        names.into_iter().collect()
    }

    fn size(&self, rel: &str) -> Option<u64> {
        self.sizes
            .get(rel)
            .copied()
            .or_else(|| self.files.get(rel).map(|t| t.len() as u64))
    }
}

/// A residual history of `rows` iterations that falls steadily.
fn residuals(rows: u64) -> String {
    let mut text = String::from("Iteration,Pressure,Velocity[X],Velocity[Y]\n");
    for k in 1..=rows {
        let k = k as f64;
        text.push_str(&format!("{k},{},{},{}\n", 1e-3 / k, 2e-3 / k, 3e-3 / k));
    }
    text
}

fn log_with_marker() -> String {
    format!("start of the run\n{COMPLETION_MARKER}\n")
}

/// A run that reached its end: a clean log, every iteration, and both monitoring histories.
fn clean_run(study: &ResolvedStudy) -> MapFiles {
    let iterations = run_of(study).unwrap().iterations;
    let mut files = MapFiles::default();
    files.put(SOLVER_LOG_FILE, &log_with_marker());
    files.put(RESIDUALS_FILE, &residuals(iterations));
    files.put(PROBES_VELOCITY_FILE, "Time,U1x,U1y,U1z\n1,0,0,0\n");
    files.put(PROBES_PRESSURE_FILE, "Time,p1\n1,0\n");
    files
}

fn judged(files: &MapFiles, study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
    backend().judge(files, study, mode)
}

fn run(files: &MapFiles, study: &ResolvedStudy) -> JudgeOutcome {
    judged(files, study, JudgeMode::Run)
}

fn failed_with(outcome: &JudgeOutcome, words: &str) {
    assert_eq!(outcome.verdict, "failed", "{outcome:?}");
    assert!(
        outcome.reasons.iter().any(|r| r.contains(words)),
        "no reason holds {words:?}: {outcome:?}"
    );
}

#[test]
fn a_clean_code_saturne_run_passes() {
    let study = cavity(8);
    let files = clean_run(&study);
    let found = run(&files, &study);
    assert_eq!(found.verdict, "passed", "{found:?}");
    assert!(found.reasons.is_empty(), "{found:?}");
    let smoke = judged(&files, &study, JudgeMode::Smoke);
    assert_eq!(smoke.verdict, "smoke_passed", "{smoke:?}");
    let finished = judged(&files, &study, JudgeMode::Finished);
    assert_eq!(finished.verdict, "passed", "{finished:?}");
}

#[test]
fn each_missing_piece_fails_with_its_own_reason() {
    let study = cavity(8);
    let iterations = run_of(&study).unwrap().iterations;
    let mut cases: Vec<(&str, MapFiles, &str)> = Vec::new();
    let mut add = |name: &'static str, words: &'static str, edit: &dyn Fn(&mut MapFiles)| {
        let mut files = clean_run(&study);
        edit(&mut files);
        cases.push((name, files, words));
    };
    add("an error file", "error file", &|f: &mut MapFiles| {
        f.put(&format!("{RESULTS_DIR}/{ERROR_FILE_PREFIX}"), "boom");
    });
    add(
        "no completion marker",
        "reached its end",
        &|f: &mut MapFiles| {
            f.put(SOLVER_LOG_FILE, "start of the run\n");
        },
    );
    add(
        "no solver log",
        "solver log is missing",
        &|f: &mut MapFiles| {
            f.remove(SOLVER_LOG_FILE);
        },
    );
    add("a nan", "not a finite number", &|f: &mut MapFiles| {
        f.put(RESIDUALS_FILE, "Iteration,Pressure\n1,nan\n");
    });
    add(
        "an empty residual file",
        "residual history is empty",
        &|f: &mut MapFiles| {
            f.put(RESIDUALS_FILE, "");
        },
    );
    add(
        "no residual file",
        "residual history is missing",
        &|f: &mut MapFiles| {
            f.remove(RESIDUALS_FILE);
        },
    );
    add(
        "a short count",
        "stops at iteration",
        &|f: &mut MapFiles| {
            f.put(RESIDUALS_FILE, &residuals(iterations - 1));
        },
    );
    add(
        "no velocity history",
        "velocity monitoring",
        &|f: &mut MapFiles| {
            f.remove(PROBES_VELOCITY_FILE);
        },
    );
    add(
        "no pressure history",
        "pressure monitoring",
        &|f: &mut MapFiles| {
            f.remove(PROBES_PRESSURE_FILE);
        },
    );
    let mut first_reasons = BTreeSet::new();
    for (name, files, words) in &cases {
        let found = run(files, &study);
        failed_with(&found, words);
        // Nothing but the files was given to the judge, so the exit status could not take part.
        first_reasons.insert(found.reasons[0].clone());
        assert_eq!(found.reasons.len(), 1, "{name}: {found:?}");
    }
    assert_eq!(first_reasons.len(), cases.len(), "{first_reasons:?}");
    // An infinite value is refused with the reason of a nan.
    let mut files = clean_run(&study);
    files.put(RESIDUALS_FILE, "Iteration,Pressure\n1,inf\n");
    failed_with(&run(&files, &study), "not a finite number");
}

#[test]
fn a_stopped_run_needs_finite_residuals_and_no_error_file() {
    let study = cavity(8);
    let stopped = || JudgeMode::Stopped {
        time: "100".to_owned(),
    };
    let mut files = MapFiles::default();
    files.put(RESIDUALS_FILE, &residuals(100));
    let found = judged(&files, &study, stopped());
    assert_eq!(found.verdict, "passed", "{found:?}");
    assert!(found.reasons.is_empty(), "{found:?}");

    let mut with_error = files.clone();
    with_error.put(&format!("{RESULTS_DIR}/{ERROR_FILE_PREFIX}"), "boom");
    failed_with(&judged(&with_error, &study, stopped()), "error file");

    let mut with_nan = files.clone();
    with_nan.put(RESIDUALS_FILE, "Iteration,Pressure\n1,nan\n");
    failed_with(&judged(&with_nan, &study, stopped()), "not a finite number");

    let mut without = files.clone();
    without.remove(RESIDUALS_FILE);
    failed_with(&judged(&without, &study, stopped()), "residual history");
}

#[test]
fn a_file_past_the_read_cap_is_said_to_be_cut() {
    let study = cavity(8);
    let too_long = READ_CAP_BYTES + 1;
    for name in [RESIDUALS_FILE, PROBES_VELOCITY_FILE, SOLVER_LOG_FILE] {
        let mut files = clean_run(&study);
        files.state_size(name, too_long);
        let found = run(&files, &study);
        assert!(
            found.reasons.iter().any(|r| r.contains("read limit")),
            "{name}: {found:?}"
        );
        assert!(
            found
                .reasons
                .iter()
                .all(|r| !r.contains("RESU") && !r.contains(".csv") && !r.contains(".log")),
            "a reason holds a path: {found:?}"
        );
    }
    // The sentence is there whatever the verdict is: a cut history that was otherwise whole
    // passes, and a cut file beside a missing marker fails.
    let mut files = clean_run(&study);
    files.state_size(RESIDUALS_FILE, too_long);
    assert_eq!(run(&files, &study).verdict, "passed");
    files.put(SOLVER_LOG_FILE, "start of the run\n");
    let found = run(&files, &study);
    failed_with(&found, "reached its end");
    assert!(found.reasons.iter().any(|r| r.contains("read limit")));
}

/// A residual history of `rows` iterations with the pressure residual `p(k)` and the velocity
/// residual `u(k)` at iteration `k`.
fn history(rows: u64, p: impl Fn(f64) -> f64, u: impl Fn(f64) -> f64) -> String {
    let mut text = String::from("Iteration,Pressure,Velocity[X],Velocity[Y]\n");
    for k in 1..=rows {
        let k = k as f64;
        text.push_str(&format!("{k},{},{},{}\n", p(k), u(k), 0.5 * u(k)));
    }
    text
}

fn converged_for(files: &MapFiles, study: &ResolvedStudy) -> cfdcraft_backend::ConvergenceOutcome {
    backend()
        .convergence(files, study)
        .expect("this backend gives its own verdict")
}

fn steady_files(study: &ResolvedStudy, text: &str) -> MapFiles {
    let mut files = clean_run(study);
    files.put(RESIDUALS_FILE, text);
    files
}

#[test]
fn a_steady_run_within_the_limits_is_converged() {
    let study = cavity(8);
    let files = clean_run(&study);
    let found = converged_for(&files, &study);
    assert!(found.converged, "{found:?}");
    assert!(found.reasons.is_empty(), "{found:?}");
}

#[test]
fn a_steady_run_with_a_rising_tail_is_not_converged() {
    let study = cavity(8);
    let rows = run_of(&study).unwrap().iterations;
    // The last row is above the pressure limit and the tail is flat: only the limit is named.
    let above = steady_files(&study, &history(rows, |_| 1e-5, |_| 1e-8));
    let found = converged_for(&above, &study);
    assert!(!found.converged, "{found:?}");
    assert!(
        found
            .reasons
            .iter()
            .any(|r| r.contains("pressure") && r.contains("above the limit")),
        "{found:?}"
    );
    assert!(
        !found.reasons.iter().any(|r| r.contains("rose")),
        "{found:?}"
    );

    // Every row is within the limits but the velocity residual climbs: only the rise is named.
    let climbing = steady_files(
        &study,
        &history(rows, |_| 1e-9, |k| 1e-8 * (1.0 + k / 1000.0)),
    );
    let found = converged_for(&climbing, &study);
    assert!(!found.converged, "{found:?}");
    assert!(
        found
            .reasons
            .iter()
            .any(|r| r.contains("velocity") && r.contains("rose")),
        "{found:?}"
    );
    assert!(
        !found.reasons.iter().any(|r| r.contains("above the limit")),
        "{found:?}"
    );
}

/// A lid cavity in time, watched at the five default points.
fn transient_cavity() -> ResolvedStudy {
    let mut options = common::options(MeshPreset::Coarse);
    options.time = Some(TimeChoice::Transient);
    let mut study = common::resolved("lid-cavity", &options);
    study.monitors.probes.clear();
    study.monitors.lines.clear();
    study.monitors.surfaces.clear();
    study.monitors.forces.clear();
    study
}

/// The velocity history of every point of the case: a transverse velocity that oscillates with a
/// cycle of 0.2 s, sampled 20 times a cycle, with the amplitude `1 + growth t`.
fn oscillating_velocity(study: &ResolvedStudy, growth: f64) -> String {
    let points = probe_set(study).unwrap().points.len();
    let mut text = String::from("Time,velocity columns\n");
    for k in 1..=400 {
        let t = 0.01 * f64::from(k);
        let uy = (1.0 + growth * t) * (std::f64::consts::TAU * (f64::from(k) + 0.5) / 20.0).sin();
        text.push_str(&format!("{t}"));
        for _ in 0..points {
            text.push_str(&format!(",0,{uy},0"));
        }
        text.push('\n');
    }
    text
}

#[test]
fn a_settled_transient_run_is_converged() {
    let study = transient_cavity();
    let mut files = clean_run(&study);
    files.put(PROBES_VELOCITY_FILE, &oscillating_velocity(&study, 0.0));
    let found = converged_for(&files, &study);
    assert!(found.converged, "{found:?}");

    // The amplitude still grows from cycle to cycle: the estimator does not call it settled.
    files.put(PROBES_VELOCITY_FILE, &oscillating_velocity(&study, 0.1));
    let found = converged_for(&files, &study);
    assert!(!found.converged, "{found:?}");
    assert!(
        found.reasons.iter().any(|r| r.contains("changing")),
        "{found:?}"
    );
}

#[test]
fn a_missing_series_is_not_converged_with_its_reason() {
    let study = cavity(8);
    let mut files = clean_run(&study);
    files.remove(RESIDUALS_FILE);
    let found = converged_for(&files, &study);
    assert!(!found.converged, "{found:?}");
    assert!(
        found
            .reasons
            .iter()
            .any(|r| r.contains("residual history is missing")),
        "{found:?}"
    );

    let study = transient_cavity();
    let mut files = clean_run(&study);
    files.remove(PROBES_VELOCITY_FILE);
    let found = converged_for(&files, &study);
    assert!(!found.converged, "{found:?}");
    assert!(
        found
            .reasons
            .iter()
            .any(|r| r.contains("monitored series is missing")),
        "{found:?}"
    );
}
