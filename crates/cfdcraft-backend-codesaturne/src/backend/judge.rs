//! Judging a finished code_saturne run from the files it left. The exit status of the solver takes
//! no part in it: a run passes only on evidence, and any piece of evidence that is missing fails
//! it.

use super::results::{ResultView, RunSide, Table, TableError, parse_table};
use crate::{
    markers::{
        COMPLETION_MARKER, ERROR_FILE_PREFIX, PROBES_PRESSURE_FILE, PROBES_VELOCITY_FILE,
        READ_CAP_BYTES, RESIDUAL_PRESSURE_WORD, RESIDUAL_VELOCITY_WORD, RESIDUALS_FILE,
        RESULTS_DIR, SOLVER_LOG_FILE,
    },
    models::run_of,
};
use cfdcraft_backend::{ConvergenceOutcome, JudgeMode, JudgeOutcome, RunFiles};
use cfdcraft_model::{MonitorField, ResolvedStudy, ResolvedTime};
use cfdcraft_settle::{PeriodicOptions, PeriodicStatus, Samples, assess_periodic};

/// The sentence a report carries when a file was longer than the read limit. It names no path.
const CUT_SENTENCE: &str =
    "a file of the run was longer than the read limit, so it was read only up to the limit";

/// A file read up to the limit, and whether it was longer.
struct Capped {
    text: String,
    cut: bool,
}

/// The last [`READ_CAP_BYTES`] of a text, starting at a whole line so no half row is read.
fn tail(text: String) -> String {
    let cap = READ_CAP_BYTES as usize;
    if text.len() <= cap {
        return text;
    }
    let mut start = text.len() - cap;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let kept = &text[start..];
    kept.split_once('\n')
        .map_or(kept, |(_, rest)| rest)
        .to_owned()
}

/// A file as text, cut to its end at the limit.
fn read_capped(files: &dyn RunFiles, rel: &str) -> Option<Capped> {
    let text = files.read_text(rel)?;
    let long = files.size(rel).is_some_and(|n| n > READ_CAP_BYTES);
    let cut = long || text.len() as u64 > READ_CAP_BYTES;
    Some(Capped {
        text: tail(text),
        cut,
    })
}

/// The reasons a run fails, and whether any file was cut at the limit.
#[derive(Default)]
struct Findings {
    failures: Vec<String>,
    cut: bool,
}

impl Findings {
    fn fail(&mut self, reason: &str) {
        self.failures.push(reason.to_owned());
    }

    /// Whether nothing failed, and the reasons: the failures, then the sentence about the limit
    /// when a file was cut.
    fn finish(self) -> (bool, Vec<String>) {
        let clean = self.failures.is_empty();
        let mut reasons = self.failures;
        if self.cut {
            reasons.push(CUT_SENTENCE.to_owned());
        }
        (clean, reasons)
    }
}

/// An `error` file left by the solver in the results folder or in the case folder.
fn has_error_file(files: &dyn RunFiles) -> bool {
    [RESULTS_DIR, ""].iter().any(|dir| {
        files
            .list(dir)
            .iter()
            .any(|name| name.starts_with(ERROR_FILE_PREFIX))
    })
}

fn check_log(files: &dyn RunFiles, found: &mut Findings) {
    let Some(log) = read_capped(files, SOLVER_LOG_FILE) else {
        found.fail("the solver log is missing");
        return;
    };
    found.cut |= log.cut;
    if !log.text.contains(COMPLETION_MARKER) {
        found.fail("the solver log does not show that the run reached its end");
    }
}

/// The residual history as a table of finite numbers, and whether it was cut at the limit. A
/// history that is missing, empty or not finite is a failure and gives no table.
fn read_residuals(files: &dyn RunFiles, found: &mut Findings) -> Option<(Table, bool)> {
    let Some(read) = read_capped(files, RESIDUALS_FILE) else {
        found.fail("the residual history is missing");
        return None;
    };
    found.cut |= read.cut;
    match parse_table(&read.text) {
        Ok(table) => Some((table, read.cut)),
        Err(TableError::Empty) => {
            found.fail("the residual history is empty");
            None
        }
        Err(TableError::NotFinite) => {
            found.fail("the residual history holds a value that is not a finite number");
            None
        }
    }
}

/// The residual history must hold finite values, and, for a run that reached its end, as many
/// iterations as the study asks for. A history cut at the limit has lost its first rows, so its
/// last iteration number stands for the count.
fn check_residuals(files: &dyn RunFiles, study: &ResolvedStudy, ended: bool, found: &mut Findings) {
    let Some((table, cut)) = read_residuals(files, found) else {
        return;
    };
    if !ended {
        return;
    }
    let Ok(run) = run_of(study) else {
        found.fail("the run length of the study is not a whole number of iterations");
        return;
    };
    let done = if cut {
        table.rows.last().and_then(|r| r.first()).copied()
    } else {
        Some(table.rows.len() as f64)
    }
    .unwrap_or(0.0);
    if done < run.iterations as f64 {
        found.fail(&format!(
            "the residual history stops at iteration {} of the {} the study asks for",
            done as u64, run.iterations
        ));
    }
}

fn check_monitoring(files: &dyn RunFiles, found: &mut Findings) {
    for (file, what) in [
        (PROBES_VELOCITY_FILE, "velocity"),
        (PROBES_PRESSURE_FILE, "pressure"),
    ] {
        match files.size(file) {
            None | Some(0) => found.fail(&format!(
                "the {what} monitoring history is missing or empty"
            )),
            Some(n) => found.cut |= n > READ_CAP_BYTES,
        }
    }
}

/// Judge a finished run. A run that reached its end needs a solver log with its completion marker,
/// a finite residual history with every iteration, and both monitoring histories; a run stopped on
/// request needs only a finite residual history. A solver `error` file fails every run.
pub(super) fn judge(files: &dyn RunFiles, study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
    let ended = !matches!(mode, JudgeMode::Stopped { .. });
    let mut found = Findings::default();
    if has_error_file(files) {
        found.fail("the solver left an error file");
    }
    if ended {
        check_log(files, &mut found);
    }
    check_residuals(files, study, ended, &mut found);
    if ended {
        check_monitoring(files, &mut found);
    }
    let (clean, reasons) = found.finish();
    let verdict = match (clean, &mode) {
        (false, _) => "failed",
        (true, JudgeMode::Smoke) => "smoke_passed",
        (true, _) => "passed",
    };
    JudgeOutcome {
        verdict: verdict.to_owned(),
        reasons,
    }
}

/// The columns of a residual history whose name holds `word`, without regard to case.
fn columns_with(header: &[String], word: &str) -> Vec<usize> {
    header
        .iter()
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains(word))
        .map(|(at, _)| at)
        .collect()
}

/// The largest residual of a group of columns in one row.
fn level(row: &[f64], columns: &[usize]) -> f64 {
    columns
        .iter()
        .filter_map(|at| row.get(*at))
        .copied()
        .fold(0.0, f64::max)
}

/// Whether the residual of the last tenth of the iterations (at least two) rises: its second half
/// has a higher mean than its first. The caller holds at least two rows.
fn rising(rows: &[Vec<f64>], columns: &[usize]) -> bool {
    let tenth = (rows.len() / 10).max(2).min(rows.len());
    let (early, late) = rows[rows.len() - tenth..].split_at(tenth / 2);
    let mean = |part: &[Vec<f64>]| {
        part.iter().map(|row| level(row, columns)).sum::<f64>() / part.len() as f64
    };
    // A flat tail of equal values must not read as a rise through rounding of the two means.
    mean(late) > mean(early) * (1.0 + 1e-9)
}

/// A steady run has converged when the last residuals of the pressure and of the velocity are
/// within the limits of the study and neither rose over the last tenth of the iterations.
fn steady(files: &dyn RunFiles, study: &ResolvedStudy) -> Findings {
    let mut found = Findings::default();
    let Some((table, _)) = read_residuals(files, &mut found) else {
        return found;
    };
    let pressure = columns_with(&table.header, RESIDUAL_PRESSURE_WORD);
    let velocity = columns_with(&table.header, RESIDUAL_VELOCITY_WORD);
    if pressure.is_empty() {
        found.fail("the residual history has no pressure column");
    }
    if velocity.is_empty() {
        found.fail("the residual history has no velocity column");
    }
    if table.rows.len() < 2 {
        found.fail("the residual history holds too few iterations to judge");
    }
    let Some(last) = table.rows.last() else {
        return found;
    };
    if !found.failures.is_empty() {
        return found;
    }
    let limits = &study.solver.convergence;
    for (columns, limit, what) in [
        (&pressure, limits.p, "pressure"),
        (&velocity, limits.u, "velocity"),
    ] {
        if level(last, columns) > limit {
            found.fail(&format!(
                "the last {what} residual is above the limit of the study"
            ));
        }
        if rising(&table.rows, columns) {
            found.fail(&format!(
                "the {what} residual rose over the last tenth of the iterations"
            ));
        }
    }
    found
}

/// A run in time has converged when its first monitored point, read through the result view the
/// comparers use, settled into a periodic series. The view, not the raw file, feeds the estimator
/// so the series is the one a comparer would read and the file is read in one place only. The
/// series is the pressure of a pressure point and the transverse velocity of a velocity point.
fn transient(files: &dyn RunFiles, study: &ResolvedStudy) -> Findings {
    let mut found = Findings::default();
    let Some(probe) = study.monitor_plan().probes.into_iter().next() else {
        found.fail("the study monitors no point, so there is no series to judge");
        return found;
    };
    let file = match probe.field {
        MonitorField::Velocity => PROBES_VELOCITY_FILE,
        MonitorField::Pressure => PROBES_PRESSURE_FILE,
    };
    found.cut |= files.size(file).is_some_and(|n| n > READ_CAP_BYTES);
    let view = ResultView::new(Box::new(RunSide(files)), study);
    let Some((times, values)) = view.scalar_series(&probe.fo, probe.field) else {
        found.fail("the monitored series is missing or cannot be read");
        return found;
    };
    let series = Samples {
        name: &probe.fo,
        times: &times,
        values: &values,
    };
    // `assess_periodic` counts whole cycles and holds their amplitude and length steady.
    match assess_periodic(&series, &PeriodicOptions::default()).status {
        PeriodicStatus::Settled => {}
        PeriodicStatus::Drifting => {
            found.fail("the monitored series is still changing from cycle to cycle");
        }
        PeriodicStatus::TooEarly => {
            found.fail("the monitored series has fewer whole cycles than needed");
        }
        PeriodicStatus::TooFewSamples => {
            found.fail("the monitored series has too few samples to show a cycle");
        }
        PeriodicStatus::Error => {
            found.fail("the monitored series is not a usable series of values in time");
        }
    }
    found
}

/// Whether a run converged, from the evidence in its files.
pub(super) fn convergence(files: &dyn RunFiles, study: &ResolvedStudy) -> ConvergenceOutcome {
    let in_time = study
        .physics
        .as_ref()
        .is_some_and(|p| matches!(p.time, ResolvedTime::Transient { .. }));
    let found = if in_time {
        transient(files, study)
    } else {
        steady(files, study)
    };
    let (converged, reasons) = found.finish();
    ConvergenceOutcome { converged, reasons }
}
