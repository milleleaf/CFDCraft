//! The end of a job: download what the container left, judge it on evidence, record it in the
//! document, give the host back, and write `report.json`.
//!
//! A job is judged from its files, never from the exit status of the container. A run that
//! finished is recorded together with its convergence verdict, whatever that verdict says; a run
//! that did not finish records nothing. The report is written before the host is cleaned up, so a
//! step that fails after the document was changed is picked up again: from the report when it was
//! written, and otherwise from the run itself, which carries the name of the container it came
//! from, so the run is never recorded twice.

use std::fs;
use std::path::Path;

use cfdcraft_logscan::redact_sentence;
use cfdcraft_ops::jobs::{CaseRun, JobAction, JobOptions, plan_scenario_job_on, prepare_on};
use cfdcraft_ops::run::judge_finished_on;
use cfdcraft_ops::{Report, Repro};
use cfdcraft_queue::{Action, Job, JobKind, Outcome};
use cfdcraft_remote::{Host, is_owned};
use cfdcraft_runs::{Check, Outcome as RunOutcome};
use cfdcraft_text::Line;
use serde::{Deserialize, Serialize};

use crate::DrainError;
use crate::case::row_of;
use crate::explain::{self, Classified};
use crate::stop::{self, Ended};
use crate::tick::{self, Drain, STUDY, run_id_of};

/// The code of a run whose output was not recorded: the document changed while it ran, or a
/// check did not hold.
pub const NOT_RECORDED: &str = "CFD-REPRO-009";
/// The code of a finished run whose files do not show that it finished, when no more specific
/// reason is known.
pub const EVIDENCE_FAILED: &str = "CFD-SOLVER-099";
/// The name of the check that ties a recorded run to the container it came from.
pub const QUEUE_RUN_CHECK: &str = "queue_run";
/// The file in the live folder that holds the outcome of the job.
pub const REPORT: &str = "report.json";
/// The sentence in the report of a job whose run folder on the host could not be removed.
const NOT_REMOVED: &str = "the folder of the run on the compute host could not be removed";
/// The most rounds of fetching after the container ended; each round is one piece per file.
const MAX_PULLS: u32 = 70;

/// The options a queued job is planned with. A job is in the queue because someone wanted it, so
/// a result that was pruned is made again.
pub const PLAN_OPTIONS: JobOptions = JobOptions {
    remake_pruned: true,
};

/// What the job asks the planner for.
pub fn action_of(job: &Job) -> JobAction {
    match &job.kind {
        JobKind::Solve => JobAction::Solve,
        JobKind::Continue { run } => JobAction::Continue { run: run.clone() },
    }
}

/// How a job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportOutcome {
    Finished,
    Stopped,
    Failed,
}

/// What `report.json` holds. It names no host, key, folder on the host or container: the lines
/// are sentences for a person and pass through the log redaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobReport {
    pub job: String,
    pub outcome: ReportOutcome,
    /// The run recorded for a finished or stopped job.
    pub run: Option<String>,
    /// The stable code of a failed job.
    pub code: Option<String>,
    pub lines: Vec<String>,
}

/// Writes the report of a job into its live folder.
pub fn write_report(live: &Path, report: &JobReport) -> Result<(), DrainError> {
    let text = serde_json::to_string_pretty(report).map_err(|e| DrainError::Io {
        reason: e.to_string(),
    })?;
    // A job that failed before it ever started has no live folder yet.
    fs::create_dir_all(live).map_err(|e| DrainError::io(&e))?;
    craft_atomic::atomic_write(&live.join(REPORT), text.as_bytes()).map_err(|e| DrainError::io(&e))
}

/// The report of a job, when one was written and reads as one.
pub fn read_report(live: &Path) -> Option<JobReport> {
    let text = fs::read_to_string(live.join(REPORT)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The report of a job whose run is already in the document, when there is one. It is how a
/// finish that was cut short after the run was recorded is picked up without recording it again.
fn recorded_report<H: Host>(ctx: &Drain<H>, job: &Job) -> Option<JobReport> {
    let run_id = job.run_id.as_deref()?;
    let runs = cfdcraft_doc::open(&ctx.config.doc)
        .ok()?
        .document
        .artifacts
        .runs;
    let found = runs.iter().find(|r| {
        r.checks
            .iter()
            .any(|c| c.name == QUEUE_RUN_CHECK && c.detail == run_id)
    })?;
    let (outcome, line) = match &found.outcome {
        RunOutcome::Stopped { at_time } => (
            ReportOutcome::Stopped,
            Line::JobStopped {
                job: job.id.clone(),
                run: found.id.clone(),
                time: at_time.clone(),
            },
        ),
        RunOutcome::Complete => (
            ReportOutcome::Finished,
            Line::JobFinished {
                job: job.id.clone(),
                run: found.id.clone(),
            },
        ),
    };
    Some(JobReport {
        job: job.id.clone(),
        outcome,
        run: Some(found.id.clone()),
        code: None,
        lines: vec![ctx.text.render(&line)],
    })
}

/// The checks every recorded run of a job carries: the evidence, and the container it came from.
pub(crate) fn run_checks(job: &Job, verdict: String) -> Vec<Check> {
    vec![
        Check {
            name: "evidence".to_owned(),
            passed: true,
            detail: verdict,
        },
        Check {
            name: QUEUE_RUN_CHECK.to_owned(),
            passed: true,
            detail: job.run_id.clone().unwrap_or_default(),
        },
    ]
}

/// The code of the first error a report holds, or of its first finding.
pub(crate) fn first_error_code(report: &Report) -> Option<String> {
    report
        .diagnostics
        .iter()
        .find(|d| format!("{:?}", d.severity) == "Error")
        .or_else(|| report.diagnostics.first())
        .map(|d| d.code.clone())
}

/// The messages of the findings of a report.
fn messages(report: &Report) -> Vec<String> {
    report
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

/// Ends a job as failed with `code`: the report is written, then, when the host was involved
/// (`cleanup`), the lock is given back if it is ours and the container is removed, and last the
/// move is written to the queue. The run folder on the host stays: the failure is not tied to
/// what the logs show.
pub fn fail<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    code: &str,
    lines: &[String],
    cleanup: bool,
) -> Result<(), DrainError> {
    fail_with(ctx, job, code, lines, cleanup, false)
}

/// Ends a job on a refusal that came up before or after the host was used (a backend that does
/// not exist, an image that is not pinned): the job fails with the refusal's code and sentence.
/// Anything else is handed back as the error it is.
pub fn fail_refused<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    error: DrainError,
    cleanup: bool,
) -> Result<(), DrainError> {
    match error {
        DrainError::Refused { code, line } => {
            let sentence = ctx.text.render(&line);
            fail(ctx, job, code, &[sentence], cleanup)
        }
        other => Err(other),
    }
}

/// Like [`fail`] for a failure the mirrored logs explain: the run folder on the host is removed
/// as well, when the drain was set up to do that.
pub fn fail_explained<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    code: &str,
    lines: &[String],
) -> Result<(), DrainError> {
    fail_with(ctx, job, code, lines, true, true)
}

fn fail_with<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    code: &str,
    lines: &[String],
    cleanup: bool,
    drop_folder: bool,
) -> Result<(), DrainError> {
    let live = ctx.live_dir(job)?;
    let report = JobReport {
        job: job.id.clone(),
        outcome: ReportOutcome::Failed,
        run: None,
        code: Some(code.to_owned()),
        lines: lines
            .iter()
            .map(String::as_str)
            .map(redact_sentence)
            .collect(),
    };
    write_report(&live, &report)?;
    conclude_with(ctx, job, &report, cleanup, drop_folder)
}

/// Gives the host back and writes the end of the job to the queue, as the report says. The run
/// folder on the host is removed last, and only for a run that was recorded.
pub fn conclude<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    report: &JobReport,
    cleanup: bool,
) -> Result<(), DrainError> {
    let recorded = report.outcome != ReportOutcome::Failed;
    conclude_with(ctx, job, report, cleanup, recorded)
}

fn conclude_with<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    report: &JobReport,
    cleanup: bool,
    drop_folder: bool,
) -> Result<(), DrainError> {
    let mut report = report.clone();
    if cleanup {
        let run_id = run_id_of(job)?.to_owned();
        ctx.host.release(&run_id)?;
        ctx.host.remove_container(&run_id)?;
        // The results are recorded and the report is on disk: only now may the folder go. A
        // failure to remove it is a note in the report, never a failed job, and the same call
        // can be made again.
        if drop_folder
            && ctx.config.clean_run_dirs
            && is_owned(&run_id)
            && ctx.host.remove_run_dir(&run_id).is_err()
            && !report.lines.iter().any(|l| l == NOT_REMOVED)
        {
            report.lines.push(NOT_REMOVED.to_owned());
            write_report(&ctx.live_dir(job)?, &report)?;
        }
    }
    let outcome = match (report.outcome, &report.run) {
        (ReportOutcome::Finished, Some(run)) => Outcome::Finished { run: run.clone() },
        (ReportOutcome::Stopped, Some(run)) => Outcome::Stopped { run: run.clone() },
        _ => Outcome::Failed {
            code: report
                .code
                .clone()
                .unwrap_or_else(|| "CFD-INFRA-005".to_owned()),
        },
    };
    ctx.persist(Action::Complete {
        id: job.id.clone(),
        outcome,
    })?;
    Ok(())
}

/// Finishes a job whose container has ended.
pub fn finish<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<(), DrainError> {
    let live = ctx.live_dir(job)?;
    // The document may already be changed and the report written: only the clean-up is left.
    if let Some(done) = read_report(&live) {
        return conclude(ctx, job, &done, true);
    }
    // The run may already be in the document although the report was never written.
    if let Some(done) = recorded_report(ctx, job) {
        write_report(&live, &done)?;
        return conclude(ctx, job, &done, true);
    }
    // Whatever the container wrote before it ended is in the mirror before anything reads it.
    for _ in 0..MAX_PULLS {
        if !tick::pull(ctx, job)? {
            break;
        }
    }
    match stop::classify_end(ctx, job)? {
        Ended::Stopped { time } => return stop::finish_stopped(ctx, job, &time),
        Ended::Normal => {}
    }
    if let Classified::Failed { code, line } = explain::classify(ctx, job)? {
        return explain::finish_failed(ctx, job, &code, line);
    }
    record(ctx, job, &live)
}

/// Downloads the results, judges them and records the run.
fn record<H: Host>(ctx: &mut Drain<H>, job: &Job, live: &Path) -> Result<(), DrainError> {
    let run_id = run_id_of(job)?.to_owned();
    let results = live.join("results");
    match fs::remove_dir_all(&results) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(DrainError::io(&e)),
    }
    ctx.host.download(&run_id, &results)?;

    // The case was made from the study as it was when the job started.
    let study = fs::read_to_string(live.join(STUDY)).map_err(|e| DrainError::io(&e))?;
    let row = match row_of(job) {
        Ok(row) => row,
        Err(error) => return fail_refused(ctx, job, error, true),
    };
    let Ok(prepared) = prepare_on(&study, &row) else {
        let lines = ["the study of the job could not be prepared again".to_owned()];
        return fail(ctx, job, NOT_RECORDED, &lines, true);
    };
    let evidence = judge_finished_on(&row, &results, &prepared.resolved);
    if evidence.verdict == "failed" {
        return fail(ctx, job, EVIDENCE_FAILED, &evidence.reasons, true);
    }

    let planned = match plan_scenario_job_on(
        &ctx.config.doc,
        job.scenario.as_deref(),
        action_of(job),
        &PLAN_OPTIONS,
        job.backend.as_deref(),
    ) {
        Ok(planned) => planned,
        Err(outcome) => {
            let code = first_error_code(&outcome.report).unwrap_or_else(|| NOT_RECORDED.to_owned());
            let lines = [changed()];
            return fail(ctx, job, &code, &lines, true);
        }
    };
    // A run is recorded only for the study it was made from.
    if planned.study_json() != study {
        return fail(ctx, job, NOT_RECORDED, &[changed()], true);
    }
    let run = CaseRun {
        results_dir: results,
        checks: run_checks(job, evidence.verdict),
        host: Some(ctx.config.profile_name.clone()),
        np: Some(ctx.scratch.np.unwrap_or(1)),
        stopped_at: None,
    };
    let outcome = planned.finish(run);
    let recorded = match &outcome.report.repro {
        Some(Repro::Run(data)) => data.run.clone(),
        _ => None,
    };
    let Some(recorded) = recorded else {
        let code = first_error_code(&outcome.report).unwrap_or_else(|| NOT_RECORDED.to_owned());
        return fail(ctx, job, &code, &messages(&outcome.report), true);
    };
    let sentence = ctx.text.render(&Line::JobFinished {
        job: job.id.clone(),
        run: recorded.clone(),
    });
    let mut lines = vec![sentence];
    lines.extend(
        crate::case::lines_for(ctx, job)
            .iter()
            .map(|l| ctx.text.render(l)),
    );
    let report = JobReport {
        job: job.id.clone(),
        outcome: ReportOutcome::Finished,
        run: Some(recorded),
        code: None,
        lines,
    };
    write_report(live, &report)?;
    conclude(ctx, job, &report, true)
}

fn changed() -> String {
    "the document changed while the case ran, so the run was not recorded".to_owned()
}
