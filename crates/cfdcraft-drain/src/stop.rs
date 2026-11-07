//! Stopping a job on request and recording the run that was stopped.
//!
//! A stop is an edit of `stopAt` in the control file of the running case, made through the same
//! dictionary editor and safety scan as every other change to a case, and written back as one
//! replacement on the host. The solver reads it, writes its fields and ends. A job whose solver
//! does not follow is asked again and then stopped by force; that run is interrupted, not stopped,
//! and the lock is given back only once the container is confirmed gone.

use std::fs;
use std::path::PathBuf;

use cfdcraft_backend_esi::guard::scan_tree;
use cfdcraft_backend_esi::{CaseFile, CaseTree, Dialect};
use cfdcraft_dictedit::{get_entry, set_entry};
use cfdcraft_logscan::redact;
use cfdcraft_ops::jobs::{CaseRun, plan_scenario_job_with};
use cfdcraft_ops::run::judge_stopped_results;
use cfdcraft_ops::{Repro, prepare};
use cfdcraft_queue::{Action, Job, JobState, Outcome, StopCause, StopMode, StopRequest};
use cfdcraft_remote::{ContainerState, Host, HostError};
use cfdcraft_text::Line;

use crate::DrainError;
use crate::explain::{self, Classified};
use crate::finalize::{
    EVIDENCE_FAILED, JobReport, NOT_RECORDED, PLAN_OPTIONS, ReportOutcome, action_of, conclude,
    fail, first_error_code, run_checks, write_report,
};
use crate::tick::{Drain, STUDY, run_id_of};

/// The control file of a case, below the run folder.
const CONTROL: &str = "system/controlDict";
/// The solver log, in the mirror.
const SOLVER_LOG: &str = "log.simpleFoam";
/// The code of a run that could not be confirmed stopped.
const NOT_CONFIRMED: &str = "CFD-RUN-008";
/// The seconds a container is given to end when it is stopped by force.
const GRACE_SECONDS: u32 = 30;
/// How many times the stop is written again before the container is stopped by force.
const REWRITES: u32 = 2;
/// The value of `Scratch::rewrites` that says the run was stopped by force. The follow-up of a
/// tick that stopped a container ends the job in the finishing step, which asks this module how
/// the run ended.
const FORCED: u32 = u32::MAX;
/// Bytes in a gigabyte, counted in kilobytes.
const KB_PER_GB: u64 = 1024 * 1024;

/// How a job that ended ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The run went to its end.
    Normal,
    /// The run was stopped on request at `time`, the latest time it wrote.
    Stopped { time: String },
}

/// Carries out the stop a job was asked for: edits the control file of a running job, and keeps
/// track of a job that was already told to stop.
pub fn apply_requests<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<(), DrainError> {
    let Some(request) = job.stop else {
        return Ok(());
    };
    match job.state {
        JobState::Running => begin_stop(ctx, job, request),
        JobState::Stopping => keep_waiting(ctx, job, request),
        _ => Ok(()),
    }
}

/// Writes the stop into the case and moves the job to stopping. A control file that cannot be read,
/// or an edit the safety scan refuses, is not written: the run is stopped by force instead.
fn begin_stop<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    request: StopRequest,
) -> Result<(), DrainError> {
    let run_id = run_id_of(job)?.to_owned();
    let Some(text) = soft(ctx.host.read_text(&run_id, CONTROL))? else {
        return force(ctx, job);
    };
    let Some(edited) = edited(&text, request.mode) else {
        return force(ctx, job);
    };
    ctx.host.write_text(&run_id, CONTROL, &edited)?;
    ctx.persist(Action::StopApplied { id: job.id.clone() })?;
    ctx.scratch.stop_applied_tick = Some(ctx.scratch.tick_no);
    ctx.scratch.rewrites = 0;
    ctx.scratch.disk_stop = request.cause == StopCause::Disk;
    Ok(())
}

/// The control file with `stopAt` set for `mode`, when the edit holds and passes the safety scan.
fn edited(text: &str, mode: StopMode) -> Option<String> {
    let word = match mode {
        StopMode::WriteNow => "writeNow",
        StopMode::NoWrite => "noWriteNow",
    };
    let next = set_entry(text, "stopAt", word).ok()?;
    let tree = CaseTree {
        files: vec![CaseFile {
            rel_path: PathBuf::from(CONTROL),
            bytes: next.clone().into_bytes(),
        }],
    };
    scan_tree(&tree).is_empty().then_some(next)
}

/// A host call whose failure, other than the host not answering, is not worth stopping for.
fn soft<T>(result: Result<T, HostError>) -> Result<Option<T>, DrainError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(HostError::Unreachable) => Err(HostError::Unreachable.into()),
        Err(_) => Ok(None),
    }
}

/// What a job that was told to stop does while the solver has not ended. The wait is counted in
/// ticks since the stop was written: the same text is written again a third and two thirds of the
/// way through, so the file counts as changed, and when the wait is over a container that still
/// runs is stopped by force. Nothing is changed before the wait has passed.
fn keep_waiting<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    request: StopRequest,
) -> Result<(), DrainError> {
    if forced(ctx) {
        return force(ctx, job);
    }
    let now = ctx.scratch.tick_no;
    // After a restart the wait starts again from the first tick that sees the job stopping.
    let applied = *ctx.scratch.stop_applied_tick.get_or_insert(now);
    let waited = now.saturating_sub(applied);
    let wait = u64::from(ctx.config.stop_wait_ticks);
    if waited >= wait {
        let run_id = run_id_of(job)?.to_owned();
        if ctx.host.inspect(&run_id)? == ContainerState::Running {
            return force(ctx, job);
        }
        return Ok(());
    }
    let due = (u64::from(ctx.scratch.rewrites) + 1) * (wait / 3).max(1);
    if ctx.scratch.rewrites < REWRITES && waited >= due && due < wait {
        ctx.scratch.rewrites += 1;
        let run_id = run_id_of(job)?.to_owned();
        if let Some(text) = soft(ctx.host.read_text(&run_id, CONTROL))?
            && let Some(again) = edited(&text, request.mode)
        {
            ctx.host.write_text(&run_id, CONTROL, &again)?;
        }
    }
    Ok(())
}

/// Stops the container of a run that cannot be stopped any other way. The tick goes on to see the
/// container end, and the job is then ended as interrupted when it is finished.
fn force<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<(), DrainError> {
    let run_id = run_id_of(job)?.to_owned();
    soft(ctx.host.stop_container(&run_id, GRACE_SECONDS))?;
    ctx.scratch.rewrites = FORCED;
    Ok(())
}

/// Whether the run of the job in hand was stopped by force.
fn forced<H: Host>(ctx: &Drain<H>) -> bool {
    ctx.scratch.rewrites == FORCED
}

/// Ends a job whose run could not be confirmed stopped: the container is removed, and the lock is
/// given back only when the container is confirmed gone. The job ends interrupted and nothing is
/// recorded.
fn interrupt<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<(), DrainError> {
    let run_id = run_id_of(job)?.to_owned();
    soft(ctx.host.remove_container(&run_id))?;
    let gone = matches!(ctx.host.inspect(&run_id)?, ContainerState::None);
    if gone {
        ctx.host.release(&run_id)?;
    }
    let reason = if gone {
        "the run could not be confirmed stopped, so nothing was recorded for it"
    } else {
        "the run could not be confirmed stopped, so nothing was recorded for it, and its \
         container may still be running, so the compute host stays reserved"
    };
    let sentence = ctx.text.render(&Line::JobInterrupted {
        job: job.id.clone(),
        reason: reason.to_owned(),
    });
    // The report has no interrupted outcome of its own: the queue holds that, and the code here.
    let report = JobReport {
        job: job.id.clone(),
        outcome: ReportOutcome::Failed,
        run: None,
        code: Some(NOT_CONFIRMED.to_owned()),
        lines: vec![redact(&sentence)],
    };
    // The queue is written first: a report left behind by a crash would be read at the next start
    // as a failure to clean up after, which gives the lock back whether or not the container is gone.
    ctx.persist(Action::Complete {
        id: job.id.clone(),
        outcome: Outcome::Interrupted {
            code: NOT_CONFIRMED.to_owned(),
        },
    })?;
    let live = ctx.live_dir(job)?;
    write_report(&live, &report)
}

/// Tells a run that went to its end from one that was stopped on request. A run is stopped when a
/// stop that writes was asked for, the log ends, and the newest time folder is the last logged
/// time and is short of the end time. A run that was stopped by force comes back as stopped at no
/// time, which [`finish_stopped`] ends as interrupted.
pub fn classify_end<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<Ended, DrainError> {
    // The record that follows says how many processes the run had.
    crate::case::restore_np(ctx, job);
    if forced(ctx) {
        return Ok(Ended::Stopped {
            time: String::new(),
        });
    }
    if !matches!(
        job.stop,
        Some(StopRequest {
            mode: StopMode::WriteNow,
            ..
        })
    ) {
        return Ok(Ended::Normal);
    }
    let log = ctx.mirror(job)?.read_to_string(SOLVER_LOG)?;
    let last = last_logged_time(&log).filter(|t| t.is_finite() && *t > 0.0);
    let Some(last) = last.filter(|_| ends_with_end(&log)) else {
        return Ok(Ended::Normal);
    };
    let run_id = run_id_of(job)?.to_owned();
    let Some(dirs) = soft(ctx.host.time_dirs(&run_id))? else {
        return Ok(Ended::Normal);
    };
    let newest = dirs
        .iter()
        .filter_map(|d| d.name.parse::<f64>().ok().map(|t| (t, &d.name)))
        .filter(|(t, _)| t.is_finite() && *t > 0.0)
        .max_by(|a, b| a.0.total_cmp(&b.0));
    let Some((newest, name)) = newest.filter(|(t, _)| close(*t, last)) else {
        return Ok(Ended::Normal);
    };
    let end = match soft(ctx.host.read_text(&run_id, CONTROL))? {
        Some(text) => get_entry(&text, "endTime")
            .ok()
            .flatten()
            .and_then(|v| v.trim().parse::<f64>().ok()),
        None => None,
    };
    let short_of_end = end.is_some_and(|end| newest < end && !close(newest, end));
    Ok(if short_of_end {
        Ended::Stopped { time: name.clone() }
    } else {
        Ended::Normal
    })
}

/// Whether two solver times are the same number.
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// Whether the last line of a log with text in it is `End`, not counting the one line a parallel
/// run adds after it.
fn ends_with_end(log: &str) -> bool {
    let mut lines = log.lines().rev().map(str::trim).filter(|l| !l.is_empty());
    let mut last = lines.next();
    if last == Some("Finalising parallel run") {
        last = lines.next();
    }
    last == Some("End")
}

/// The last `Time = ` the log shows.
fn last_logged_time(log: &str) -> Option<f64> {
    log.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix("Time = ")
            .and_then(|t| t.trim().parse::<f64>().ok())
    })
}

/// Records a run that was stopped on request at `time`: the results are downloaded and judged by
/// the rules for a cut-short run, the run is recorded with its checkpoint, and the job ends
/// stopped. A run whose log shows a failure, or whose files do not show a clean stop, fails the
/// job and records nothing.
pub fn finish_stopped<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    time: &str,
) -> Result<(), DrainError> {
    if forced(ctx) {
        return interrupt(ctx, job);
    }
    if let Classified::Failed { code, line } = explain::classify(ctx, job)? {
        return explain::finish_failed(ctx, job, &code, line);
    }
    let live = ctx.live_dir(job)?;
    let run_id = run_id_of(job)?.to_owned();
    let results = live.join("results");
    match fs::remove_dir_all(&results) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(DrainError::io(&e)),
    }
    ctx.host.download(&run_id, &results)?;

    let study = fs::read_to_string(live.join(STUDY)).map_err(|e| DrainError::io(&e))?;
    let Ok(prepared) = prepare(&study, Dialect::EsiV2512) else {
        let lines = ["the study of the job could not be prepared again".to_owned()];
        return fail(ctx, job, NOT_RECORDED, &lines, true);
    };
    let evidence = judge_stopped_results(&results, &prepared.resolved, time);
    if evidence.verdict == "failed" {
        return fail(ctx, job, EVIDENCE_FAILED, &evidence.reasons, true);
    }

    // Everything that can fail on the host is read before the run is recorded: nothing fallible
    // sits between the record and the report.
    let disk_free = if job.stop.is_some_and(|s| s.cause == StopCause::Disk) {
        Some(ctx.host.free_kb()? / KB_PER_GB)
    } else {
        None
    };
    let changed = "the document changed while the case ran, so the run was not recorded".to_owned();
    let planned = match plan_scenario_job_with(
        &ctx.config.doc,
        job.scenario.as_deref(),
        action_of(job),
        &PLAN_OPTIONS,
    ) {
        Ok(planned) => planned,
        Err(outcome) => {
            let code = first_error_code(&outcome.report).unwrap_or_else(|| NOT_RECORDED.to_owned());
            return fail(ctx, job, &code, &[changed], true);
        }
    };
    if planned.study_json() != study {
        return fail(ctx, job, NOT_RECORDED, &[changed], true);
    }
    let run = CaseRun {
        results_dir: results,
        checks: run_checks(job, evidence.verdict),
        host: Some(ctx.config.profile_name.clone()),
        np: Some(ctx.scratch.np.unwrap_or(1)),
        stopped_at: Some(time.to_owned()),
    };
    let outcome = planned.finish(run);
    let recorded = match &outcome.report.repro {
        Some(Repro::Run(data)) => data.run.clone(),
        _ => None,
    };
    let Some(recorded) = recorded else {
        let code = first_error_code(&outcome.report).unwrap_or_else(|| NOT_RECORDED.to_owned());
        let lines: Vec<String> = outcome
            .report
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        return fail(ctx, job, &code, &lines, true);
    };

    let mut sentences = vec![Line::JobStopped {
        job: job.id.clone(),
        run: recorded.clone(),
        time: time.to_owned(),
    }];
    if let Some(free) = disk_free {
        sentences.push(Line::DiskStopped {
            free: free.to_string(),
            floor: ctx.config.min_free_gb.to_string(),
        });
    } else {
        sentences.push(Line::StoppedRunNote {
            run: recorded.clone(),
            time: time.to_owned(),
        });
    }
    sentences.extend(crate::case::lines_for(ctx, job));
    let report = JobReport {
        job: job.id.clone(),
        outcome: ReportOutcome::Stopped,
        run: Some(recorded),
        code: None,
        lines: sentences.iter().map(|l| ctx.text.render(l)).collect(),
    };
    write_report(&live, &report)?;
    conclude(ctx, job, &report, true)
}
