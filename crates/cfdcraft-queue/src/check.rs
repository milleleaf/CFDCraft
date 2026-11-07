//! The rules a queue must keep, whoever wrote it.

use std::collections::HashSet;

use crate::model::{Job, JobKind, JobState, Queue};
use crate::reducer::QueueError;

/// The most jobs one queue may hold.
pub const MAX_JOBS: usize = 1000;
/// The most characters in a run id.
pub const MAX_RUN_ID: usize = 128;
/// The most characters in a stable code.
pub const MAX_CODE: usize = 64;
const MAX_SUMMARY: usize = 200;

/// The number in a job id (`q-` and 1 to 10 digits), or none when the id has another shape.
pub(crate) fn id_number(id: &str) -> Option<u32> {
    let digits = id.strip_prefix("q-")?;
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// A plain name: 1 to 40 lower-case letters, digits or hyphens, the first not a hyphen. It is the
/// rule that scenario, sweep and run ids follow elsewhere.
fn is_plain_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=40).contains(&bytes.len())
        && bytes
            .iter()
            .enumerate()
            .all(|(i, &c)| c.is_ascii_lowercase() || c.is_ascii_digit() || (i > 0 && c == b'-'))
}

fn is_run_id(text: &str) -> bool {
    (1..=MAX_RUN_ID).contains(&text.len())
        && text.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

fn is_code(text: &str) -> bool {
    (1..=MAX_CODE).contains(&text.len())
        && text
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn damaged(job: &Job, what: &str) -> QueueError {
    QueueError::Damaged {
        reason: format!("the job {} {what}", job.id),
    }
}

/// Checks every rule of a queue and returns the first one broken:
///
/// - at most [`MAX_JOBS`] jobs;
/// - job ids are `q-` and digits, and unique;
/// - a scenario, a sweep and a continued run, when present, are plain names;
/// - at most one job is active;
/// - a starting, running, stopping, finishing or unreachable job has a run id, and a queued job
///   has none; a run id is 1 to [`MAX_RUN_ID`] letters, digits or hyphens;
/// - a finished or stopped job has a run, and a failed or interrupted job has a code.
pub fn check_queue(queue: &Queue) -> Result<(), QueueError> {
    if queue.jobs.len() > MAX_JOBS {
        return Err(QueueError::TooMany {
            count: queue.jobs.len(),
            max: MAX_JOBS,
        });
    }
    let mut seen: HashSet<&str> = HashSet::with_capacity(queue.jobs.len());
    let mut active = false;
    for job in &queue.jobs {
        if id_number(&job.id).is_none() {
            return Err(QueueError::BadId {
                job: job.id.chars().take(40).collect(),
            });
        }
        if !seen.insert(job.id.as_str()) {
            return Err(damaged(job, "appears more than once"));
        }
        if let Some(scenario) = &job.scenario
            && !is_plain_name(scenario)
        {
            return Err(QueueError::BadScenario {
                job: job.id.clone(),
            });
        }
        if let JobKind::Continue { run } = &job.kind
            && !is_plain_name(run)
        {
            return Err(damaged(job, "continues a run that is not a plain name"));
        }
        if let Some(origin) = &job.origin
            && !is_plain_name(&origin.sweep)
        {
            return Err(damaged(job, "comes from a sweep that is not a plain name"));
        }
        if job.state.is_active() {
            if active {
                return Err(QueueError::Active {
                    job: job.id.clone(),
                });
            }
            active = true;
        }
        check_state_members(job)?;
        check_members(job)?;
    }
    Ok(())
}

/// Which of the run id, run and code a job must have, by its state.
fn check_state_members(job: &Job) -> Result<(), QueueError> {
    match job.state {
        JobState::Queued => {
            if job.run_id.is_some() {
                return Err(damaged(job, "is queued but holds a run id"));
            }
        }
        JobState::Starting
        | JobState::Running
        | JobState::Stopping
        | JobState::Finishing
        | JobState::Unreachable => {
            if job.run_id.is_none() {
                return Err(damaged(job, "is active but has no run id"));
            }
        }
        JobState::Finished | JobState::Stopped => {
            if job.run.is_none() {
                return Err(damaged(job, "has ended but has no recorded run"));
            }
        }
        JobState::Failed | JobState::Interrupted => {
            if job.code.is_none() {
                return Err(damaged(job, "has failed but has no code"));
            }
        }
        JobState::Cancelled => {}
    }
    Ok(())
}

/// The shape of the members that hold text or numbers.
fn check_members(job: &Job) -> Result<(), QueueError> {
    if let Some(run_id) = &job.run_id
        && !is_run_id(run_id)
    {
        return Err(damaged(
            job,
            "has a run id that is not 1 to 128 letters, digits or hyphens",
        ));
    }
    if let Some(run) = &job.run
        && !is_plain_name(run)
    {
        return Err(damaged(job, "has a recorded run that is not a plain name"));
    }
    if let Some(code) = &job.code
        && !is_code(code)
    {
        return Err(damaged(job, "has a code that is not a stable code"));
    }
    if let Some(progress) = &job.progress {
        if progress.time.is_some_and(|t| !t.is_finite()) {
            return Err(damaged(job, "has a progress time that is not a number"));
        }
        if let Some(summary) = &progress.summary
            && (summary.chars().count() > MAX_SUMMARY || summary.chars().any(char::is_control))
        {
            return Err(damaged(job, "has a progress summary that cannot be shown"));
        }
    }
    Ok(())
}
