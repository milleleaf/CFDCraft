//! The judge and the convergence verdict of the SU2 backend, on synthetic native output held in
//! memory. The exit status of the solver never takes part: only the evidence in the files does.

mod common;

use cfdcraft_backend::{Backend, JudgeMode, JudgeOutcome};
use cfdcraft_backend_su2::markers::READ_CAP_BYTES;
use cfdcraft_model::ResolvedStudy;
use common::{
    HISTORY, LOG, Mem, RESIDUALS, VOLUME, clean_steady, coarse, good_log, history_text, log_limits,
    rows_needed, steady_history,
};

/// The backend behind its trait, so the trait is in scope for every call.
fn row() -> &'static dyn Backend {
    common::backend()
}

fn judge(files: &Mem, study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
    row().judge(files, study, mode)
}

fn cavity() -> ResolvedStudy {
    coarse("lid-cavity")
}

fn words(outcome: &JudgeOutcome) -> String {
    outcome.reasons.join(" | ")
}

#[test]
fn a_clean_su2_run_passes() {
    let study = cavity();
    let files = clean_steady(&study);
    let run = judge(&files, &study, JudgeMode::Run);
    assert_eq!(
        (run.verdict.as_str(), run.reasons.clone()),
        ("passed", vec![])
    );
    let smoke = judge(&files, &study, JudgeMode::Smoke);
    assert_eq!(smoke.verdict, "smoke_passed", "{}", words(&smoke));
    let finished = judge(&files, &study, JudgeMode::Finished);
    assert_eq!(finished.verdict, "passed", "{}", words(&finished));

    // A steady run that stopped early on its residual limit says so in its log.
    let short = Mem::default()
        .with(
            LOG,
            format!("{}All convergence criteria satisfied.\n", good_log()),
        )
        .with(
            HISTORY,
            history_text(
                &["Inner_Iter", RESIDUALS[0], RESIDUALS[1], RESIDUALS[2]],
                12,
                |i| {
                    let (p, u) = log_limits(&study);
                    vec![i as f64, p - 1.0, u - 1.0, u - 1.0]
                },
            ),
        )
        .with(VOLUME, "\"x\",\"y\"\n0,0\n");
    let early = judge(&short, &study, JudgeMode::Run);
    assert_eq!(early.verdict, "passed", "{}", words(&early));
}

#[test]
fn each_missing_piece_fails_with_its_own_reason() {
    let study = cavity();
    let clean = clean_steady(&study);
    let nan_history = {
        let mut text = steady_history(&study, 1.0);
        text.push_str("1,nan,-9,-9\n");
        text
    };
    let header_only = format!("\"Inner_Iter\",\"{}\"\n", RESIDUALS[0]);
    let cases: Vec<(&str, Mem, &str)> = vec![
        (
            "no end marker",
            clean.clone().with(LOG, "Iteration 1\n"),
            "no end-of-run line",
        ),
        (
            "nan in the log",
            clean
                .clone()
                .with(LOG, format!("residual -nan\n{}", good_log())),
            "solver log holds a value that is not finite",
        ),
        (
            "inf in the log",
            clean.clone().with(LOG, format!("flux inf\n{}", good_log())),
            "solver log holds a value that is not finite",
        ),
        (
            "empty history",
            clean.clone().with(HISTORY, ""),
            "history file is empty",
        ),
        (
            "history without rows",
            clean.clone().with(HISTORY, header_only),
            "history file holds no rows",
        ),
        (
            "nan in the history",
            clean.clone().with(HISTORY, nan_history),
            "history file holds a value that is not finite",
        ),
        (
            "history stopped short",
            clean.clone().with(
                HISTORY,
                history_text(&["Inner_Iter", RESIDUALS[0]], 3, |i| vec![i as f64, -9.0]),
            ),
            "the history stops after 3 of",
        ),
        (
            "no volume file",
            clean.clone().without(VOLUME),
            "volume solution file is missing",
        ),
        (
            "no history file",
            clean.clone().without(HISTORY),
            "history file is missing",
        ),
        (
            "no log",
            clean.clone().without(LOG),
            "solver log is missing",
        ),
    ];
    let mut problems = Vec::new();
    let mut seen = Vec::new();
    for (what, files, expected) in &cases {
        let out = judge(files, &study, JudgeMode::Run);
        if out.verdict != "failed" || !words(&out).contains(expected) {
            problems.push(format!("{what}: {} / {}", out.verdict, words(&out)));
        }
        seen.push((what, words(&out)));
    }
    // Exit status zero and none of the evidence is still a failure.
    let nothing = judge(&Mem::default(), &study, JudgeMode::Run);
    if nothing.verdict != "failed" || nothing.reasons.len() < 3 {
        problems.push(format!(
            "empty folder: {} / {}",
            nothing.verdict,
            words(&nothing)
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    // Two different gaps do not give the same words.
    let distinct: std::collections::BTreeSet<&String> = seen.iter().map(|(_, w)| w).collect();
    assert!(
        distinct.len() >= 8,
        "only {} distinct reasons",
        distinct.len()
    );
}

#[test]
fn the_steady_residual_limits_apply_in_run_mode() {
    let study = cavity();
    let (p, u) = log_limits(&study);
    let rows = rows_needed(&study);
    // The pressure residual ends above its limit, the velocity residuals end below theirs.
    let above = history_text(
        &["Inner_Iter", RESIDUALS[0], RESIDUALS[1], RESIDUALS[2]],
        rows,
        |i| vec![i as f64, p + 1.0, u - 1.0, u - 1.0],
    );
    let files = clean_steady(&study).with(HISTORY, above);
    let run = judge(&files, &study, JudgeMode::Run);
    assert_eq!(run.verdict, "failed");
    assert!(
        words(&run).contains("pressure residual is above the limit"),
        "{}",
        words(&run)
    );
    assert_eq!(
        judge(&files, &study, JudgeMode::Smoke).verdict,
        "smoke_passed"
    );
    assert_eq!(judge(&files, &study, JudgeMode::Finished).verdict, "passed");

    // The velocity limit holds both components.
    let slow = history_text(
        &["Inner_Iter", RESIDUALS[0], RESIDUALS[1], RESIDUALS[2]],
        rows,
        |i| vec![i as f64, p - 1.0, u - 1.0, u + 1.0],
    );
    let files = clean_steady(&study).with(HISTORY, slow);
    let run = judge(&files, &study, JudgeMode::Run);
    assert!(
        words(&run).contains("y velocity residual is above"),
        "{}",
        words(&run)
    );

    // A residual column the history lacks cannot pass either.
    let lacking = history_text(&["Inner_Iter", RESIDUALS[0]], rows, |i| {
        vec![i as f64, p - 1.0]
    });
    let files = clean_steady(&study).with(HISTORY, lacking);
    let run = judge(&files, &study, JudgeMode::Run);
    assert!(
        words(&run).contains("no x velocity residual column"),
        "{}",
        words(&run)
    );
}

#[test]
fn a_stopped_run_needs_a_finite_history() {
    let study = cavity();
    let stopped = JudgeMode::Stopped {
        time: "40".to_owned(),
    };
    let files = Mem::default().with(LOG, "Iteration 1\nIteration 2\n").with(
        HISTORY,
        history_text(&["Inner_Iter", RESIDUALS[0]], 40, |i| vec![i as f64, -3.0]),
    );
    let out = judge(&files, &study, stopped.clone());
    assert_eq!((out.verdict.as_str(), out.reasons), ("passed", vec![]));

    let bad = files.clone().with(
        HISTORY,
        history_text(&["Inner_Iter", RESIDUALS[0]], 3, |i| {
            vec![i as f64, if i == 2 { f64::INFINITY } else { -3.0 }]
        }),
    );
    let out = judge(&bad, &study, stopped.clone());
    assert_eq!(out.verdict, "failed");
    assert!(words(&out).contains("history file holds a value that is not finite"));

    let diverged = files.clone().with(LOG, "SU2 has diverged\n");
    assert_eq!(judge(&diverged, &study, stopped).verdict, "failed");
}

#[test]
fn a_file_past_the_read_cap_is_said_to_be_cut() {
    let study = cavity();
    let over = READ_CAP_BYTES + 1;
    for name in [HISTORY, VOLUME, LOG] {
        let files = clean_steady(&study).sized(name, over);
        for mode in [JudgeMode::Run, JudgeMode::Smoke, JudgeMode::Finished] {
            let out = judge(&files, &study, mode);
            let cut: Vec<&String> = out
                .reasons
                .iter()
                .filter(|r| r.contains("read only up to"))
                .collect();
            assert_eq!(cut.len(), 1, "{name}: {}", words(&out));
            // The sentence names no file and no folder.
            assert!(!cut[0].contains('/') && !cut[0].contains(".csv") && !cut[0].contains("log"));
        }
    }
    // A run that failed for another reason still carries the sentence.
    let files = clean_steady(&study).without(LOG).sized(HISTORY, over);
    let out = judge(&files, &study, JudgeMode::Run);
    assert_eq!(out.verdict, "failed");
    assert!(words(&out).contains("read only up to"));
    // A file under the cap carries none.
    let out = judge(&clean_steady(&study), &study, JudgeMode::Run);
    assert!(!words(&out).contains("read only up to"));
}

// Convergence.

fn convergence(files: &Mem, study: &ResolvedStudy) -> (bool, String) {
    let out = row()
        .convergence(files, study)
        .expect("SU2 judges convergence itself");
    (out.converged, out.reasons.join(" | "))
}

#[test]
fn a_steady_run_within_the_limits_is_converged() {
    let study = cavity();
    let (converged, reasons) = convergence(&clean_steady(&study), &study);
    assert!(converged, "{reasons}");
    assert_eq!(reasons, "");
}

#[test]
fn a_steady_run_with_a_rising_tail_is_not_converged() {
    let study = cavity();
    let (p, u) = log_limits(&study);
    let rows = rows_needed(&study);
    let columns = ["Inner_Iter", RESIDUALS[0], RESIDUALS[1], RESIDUALS[2]];
    // Under every limit at the end, but the last tenth climbs.
    let tenth = rows / 10;
    let rising = history_text(&columns, rows, |i| {
        let climb = if i + tenth > rows {
            (i + tenth - rows) as f64 * 1e-3
        } else {
            0.0
        };
        vec![i as f64, p - 2.0 + climb, u - 2.0 + climb, u - 2.0 + climb]
    });
    let (converged, reasons) = convergence(&clean_steady(&study).with(HISTORY, rising), &study);
    assert!(!converged);
    assert!(reasons.contains("rose over the last tenth"), "{reasons}");
    assert!(!reasons.contains("above the limit"), "{reasons}");

    // A flat run whose last row is above a limit.
    let high = history_text(&columns, rows, |i| {
        vec![i as f64, p + 1.0, u - 1.0, u - 1.0]
    });
    let (converged, reasons) = convergence(&clean_steady(&study).with(HISTORY, high), &study);
    assert!(!converged);
    assert!(
        reasons.contains("pressure residual is above the limit"),
        "{reasons}"
    );
    assert!(!reasons.contains("rose"), "{reasons}");
}

/// A cylinder history of `seconds` seconds at a step of a millisecond, with a lift that sheds at 3 Hz and
/// grows by `growth` per second; the columns are the ones the view presents.
fn shedding(seconds: f64, growth: f64) -> String {
    let dt = 0.001;
    let rows = (seconds / dt) as usize;
    history_text(&["Cur_Time", "CD", "CL", "probe_1", "probe_2"], rows, |i| {
        let t = i as f64 * dt;
        let cl = (1.0 + growth * t) * (2.0 * std::f64::consts::PI * 3.0 * t).sin();
        vec![t, 3.2, cl, 3.1, 0.7]
    })
}

#[test]
fn a_settled_transient_run_is_converged() {
    let study = coarse("cylinder");
    let files = Mem::default().with(HISTORY, shedding(8.0, 0.0));
    let (converged, reasons) = convergence(&files, &study);
    assert!(converged, "{reasons}");

    let growing = Mem::default().with(HISTORY, shedding(8.0, 0.5));
    let (converged, reasons) = convergence(&growing, &study);
    assert!(!converged);
    assert!(reasons.contains("still changing"), "{reasons}");

    let short = Mem::default().with(HISTORY, shedding(1.0, 0.0));
    let (converged, reasons) = convergence(&short, &study);
    assert!(!converged);
    assert!(reasons.contains("fewer whole cycles"), "{reasons}");
}

#[test]
fn a_missing_series_is_not_converged_with_its_reason() {
    let steady = cavity();
    let (converged, reasons) = convergence(&Mem::default(), &steady);
    assert!(!converged);
    assert!(reasons.contains("history file is missing"), "{reasons}");

    let lacking = Mem::default().with(
        HISTORY,
        history_text(&["Inner_Iter"], 5, |i| vec![i as f64]),
    );
    let (converged, reasons) = convergence(&lacking, &steady);
    assert!(!converged);
    assert!(reasons.contains("no pressure residual series"), "{reasons}");

    let cylinder = coarse("cylinder");
    let (converged, reasons) = convergence(&Mem::default(), &cylinder);
    assert!(!converged);
    assert!(reasons.contains("history file is missing"), "{reasons}");

    let no_force = Mem::default().with(
        HISTORY,
        history_text(&["Cur_Time", "probe_1"], 20, |i| {
            vec![i as f64 * 0.001, 1.0]
        }),
    );
    let (converged, reasons) = convergence(&no_force, &cylinder);
    assert!(!converged);
    assert!(reasons.contains("no drag and lift series"), "{reasons}");

    // A study with no force monitor has no series to judge.
    let mut bare = coarse("cylinder");
    bare.monitors.forces.clear();
    let files = Mem::default().with(HISTORY, shedding(2.0, 0.0));
    let (converged, reasons) = convergence(&files, &bare);
    assert!(!converged);
    assert!(reasons.contains("monitors no force"), "{reasons}");
}
