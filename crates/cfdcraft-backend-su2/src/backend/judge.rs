//! Judging a finished SU2 run from the files it left: the solver log, the history table and the
//! volume solution. The exit status of the solver takes no part in it, and a missing piece of
//! evidence is a failure with its own reason.
//!
//! Every file is read up to `READ_CAP_BYTES`. A longer file is read from its end (a table keeps
//! its header line) and the outcome says so.

use super::{
    native::{Table, cut_to_cap, within_cap},
    results::{NativeView, RunAsResults},
};
use crate::markers::{
    CONVERGED_MESSAGE, DIVERGED_MESSAGE, END_BANNER, HISTORY_FILE, HISTORY_RESIDUAL_P,
    HISTORY_RESIDUAL_U, HISTORY_RESIDUAL_V, READ_CAP_BYTES, SOLVER_LOG, VOLUME_FILE,
};
use cfdcraft_backend::{ConvergenceOutcome, JudgeMode, JudgeOutcome, RunFiles};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{ResolvedStudy, ResolvedTime};
use cfdcraft_settle::{PeriodicOptions, PeriodicStatus, Samples, assess_periodic};

/// The sentence added to an outcome when a file was longer than the read limit.
const CUT_SENTENCE: &str =
    "a result file was longer than the read limit and was read only up to it";

/// A file of the run cut to the read limit, or `None` when it is missing. `cut` is set when the
/// file was over the limit.
fn read_capped(files: &dyn RunFiles, rel: &str, table: bool, cut: &mut bool) -> Option<String> {
    let size = files.size(rel)?;
    let text = files.read_text(rel)?;
    if size > READ_CAP_BYTES || !within_cap(&text) {
        *cut = true;
        return Some(cut_to_cap(&text, table));
    }
    Some(text)
}

fn transient(study: &ResolvedStudy) -> bool {
    matches!(
        study.physics.as_ref().map(|p| &p.time),
        Some(ResolvedTime::Transient { .. })
    )
}

/// The number of history rows a finished run has: the iterations of a steady run, the time steps
/// of a transient one.
fn expected_rows(study: &ResolvedStudy) -> Option<u64> {
    let end = study.solver.end_time;
    let count = match study.physics.as_ref().map(|p| &p.time) {
        Some(ResolvedTime::Transient { delta_t }) => (end / delta_t).round(),
        _ => end.round(),
    };
    (count.is_finite() && (1.0..=1e12).contains(&count)).then_some(count as u64)
}

/// What is wrong with the log, if anything; returns whether it holds the convergence message.
fn check_log(log: Option<&str>, stopped: bool, reasons: &mut Vec<String>) -> bool {
    let Some(log) = log.filter(|l| !l.trim().is_empty()) else {
        reasons.push("the solver log is missing or empty".to_owned());
        return false;
    };
    if !stopped && !log.contains(END_BANNER) {
        reasons.push("the solver log has no end-of-run line".to_owned());
    }
    if log.contains(DIVERGED_MESSAGE) {
        reasons.push("the solver reports that the run diverged".to_owned());
    }
    let broken = log
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w.eq_ignore_ascii_case("nan") || w.eq_ignore_ascii_case("inf"));
    if broken {
        reasons.push("the solver log holds a value that is not finite".to_owned());
    }
    log.contains(CONVERGED_MESSAGE)
}

/// The history table, or the reason it cannot be used.
fn history_table(text: Option<&str>) -> Result<Table, &'static str> {
    let text = text.ok_or("the history file is missing")?;
    if text.trim().is_empty() {
        return Err("the history file is empty");
    }
    let table = Table::parse(text).map_err(|_| "the history file cannot be read")?;
    if table.rows().is_empty() {
        return Err("the history file holds no rows");
    }
    if !table.all_finite() {
        return Err("the history file holds a value that is not finite");
    }
    Ok(table)
}

/// The residual columns of a steady run with the limit each is held to, from the study.
fn residual_columns(study: &ResolvedStudy) -> [(&'static str, f64, &'static str); 3] {
    let c = &study.solver.convergence;
    [
        (HISTORY_RESIDUAL_P, c.p, "pressure"),
        (HISTORY_RESIDUAL_U, c.u, "x velocity"),
        (HISTORY_RESIDUAL_V, c.u, "y velocity"),
    ]
}

/// The last row of a steady run against the limits of the study. The residuals are base-10
/// logarithms.
fn check_residuals(table: &Table, study: &ResolvedStudy, reasons: &mut Vec<String>) {
    let Some(last) = table.rows().last() else {
        return;
    };
    for (name, limit, what) in residual_columns(study) {
        match table.column(name) {
            None => reasons.push(format!("the history has no {what} residual column")),
            Some(at) if last[at] > limit.log10() => reasons.push(format!(
                "the last {what} residual is above the limit of the study"
            )),
            Some(_) => {}
        }
    }
}

/// Judge a finished run. `Run` holds a steady run's last residuals to the limits of the study;
/// `Smoke` and `Finished` hold no limit (`Smoke` says `smoke_passed`, `Finished` says `passed`);
/// `Stopped` needs a finite history and a clean log, and no end line.
pub fn judge(files: &dyn RunFiles, study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
    let mut cut = false;
    let log = read_capped(files, SOLVER_LOG, false, &mut cut);
    let history = read_capped(files, HISTORY_FILE, true, &mut cut);
    let volume = files.size(VOLUME_FILE);
    cut |= volume.is_some_and(|n| n > READ_CAP_BYTES);
    let stopped = matches!(mode, JudgeMode::Stopped { .. });
    let mut reasons = Vec::new();
    let converged = check_log(log.as_deref(), stopped, &mut reasons);
    match history_table(history.as_deref()) {
        Err(reason) => reasons.push(reason.to_owned()),
        Ok(table) if !stopped => {
            let steady = !transient(study);
            if let Some(expected) = expected_rows(study) {
                let rows = table.rows().len() as u64;
                if rows < expected && !(steady && converged) {
                    reasons.push(format!(
                        "the history stops after {rows} of {expected} iterations"
                    ));
                }
            }
            if steady && mode == JudgeMode::Run {
                check_residuals(&table, study, &mut reasons);
            }
        }
        Ok(_) => {}
    }
    if !stopped && volume.is_none_or(|n| n == 0) {
        reasons.push("the volume solution file is missing or empty".to_owned());
    }
    let verdict = match (reasons.is_empty(), &mode) {
        (false, _) => "failed",
        (true, JudgeMode::Smoke) => "smoke_passed",
        (true, _) => "passed",
    };
    if cut {
        reasons.push(CUT_SENTENCE.to_owned());
    }
    JudgeOutcome {
        verdict: verdict.to_owned(),
        reasons,
    }
}

fn not_converged(reason: impl Into<String>) -> ConvergenceOutcome {
    ConvergenceOutcome {
        converged: false,
        reasons: vec![reason.into()],
    }
}

/// Whether the run converged. A steady run has when its last residuals are within the limits of
/// the study and the residuals of the last tenth of the iterations did not rise (the last value is
/// not above the first of that stretch). A transient run has when the periodic estimator calls
/// its monitored lift series settled.
pub fn convergence(files: &dyn RunFiles, study: &ResolvedStudy) -> ConvergenceOutcome {
    let mut outcome = if transient(study) {
        transient_convergence(files, study)
    } else {
        steady_convergence(files, study)
    };
    if files.size(HISTORY_FILE).is_some_and(|n| n > READ_CAP_BYTES) {
        outcome.reasons.push(CUT_SENTENCE.to_owned());
    }
    outcome
}

fn steady_convergence(files: &dyn RunFiles, study: &ResolvedStudy) -> ConvergenceOutcome {
    let mut cut = false;
    let text = read_capped(files, HISTORY_FILE, true, &mut cut);
    let table = match history_table(text.as_deref()) {
        Ok(table) => table,
        Err(reason) => return not_converged(reason),
    };
    let rows = table.rows();
    let window = rows.len().min((rows.len() / 10).max(2));
    let (start, last) = (&rows[rows.len() - window], &rows[rows.len() - 1]);
    let mut reasons = Vec::new();
    for (name, limit, what) in residual_columns(study) {
        let Some(at) = table.column(name) else {
            reasons.push(format!("the history has no {what} residual series"));
            continue;
        };
        if last[at] > limit.log10() {
            reasons.push(format!(
                "the last {what} residual is above the limit of the study"
            ));
        }
        if last[at] > start[at] {
            reasons.push(format!(
                "the {what} residual rose over the last tenth of the iterations"
            ));
        }
    }
    ConvergenceOutcome {
        converged: reasons.is_empty(),
        reasons,
    }
}

/// The time and lift columns of a coefficient file.
fn lift_series(text: &str) -> Option<(Vec<f64>, Vec<f64>)> {
    let mut times = Vec::new();
    let mut lift = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let mut words = line.split_whitespace().map(str::parse::<f64>);
        let (t, _, cl) = (
            words.next()?.ok()?,
            words.next()?.ok()?,
            words.next()?.ok()?,
        );
        times.push(t);
        lift.push(cl);
    }
    Some((times, lift))
}

/// The settled test of a shedding run. It reads the lift of the study's first force monitor
/// through the result view, the same text the benchmark comparers read, so the history columns
/// are read in one place only. `assess_periodic` is the estimator the benchmark comparers use.
fn transient_convergence(files: &dyn RunFiles, study: &ResolvedStudy) -> ConvergenceOutcome {
    if files.size(HISTORY_FILE).is_none() {
        return not_converged("the history file is missing");
    }
    let plan = study.monitor_plan();
    let Some(force) = plan.forces.first() else {
        return not_converged("the study monitors no force, so there is no series to judge");
    };
    let results = RunAsResults(files);
    let view = NativeView::over(&results, study);
    let path = format!("postProcessing/{}/0/coefficient.dat", force.fo);
    let Some(text) = view.read(&path) else {
        return not_converged("the history has no drag and lift series for the monitored force");
    };
    let Some((times, lift)) = lift_series(&text) else {
        return not_converged("the lift series cannot be read");
    };
    let series = Samples {
        name: "lift",
        times: &times,
        values: &lift,
    };
    let state = assess_periodic(&series, &PeriodicOptions::default());
    match state.status {
        PeriodicStatus::Settled => ConvergenceOutcome {
            converged: true,
            reasons: Vec::new(),
        },
        PeriodicStatus::Drifting => {
            not_converged("the amplitude or the cycle length of the lift is still changing")
        }
        PeriodicStatus::TooEarly => not_converged("the lift has fewer whole cycles than needed"),
        PeriodicStatus::TooFewSamples => not_converged("the lift series has too few samples"),
        PeriodicStatus::Error => not_converged("the lift series is not a usable time series"),
    }
}
