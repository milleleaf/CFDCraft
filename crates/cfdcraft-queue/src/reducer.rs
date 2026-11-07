//! The moves of a queue: a pure function from a queue and an action to a new queue.

use thiserror::Error;

use crate::check::{check_queue, id_number};
use crate::model::{Job, JobState, NewJob, Progress, Queue, StopCause, StopMode, StopRequest};

/// How many times a start may be retried before the caller must fail the job.
pub const MAX_ATTEMPTS: u32 = 2;

/// The first thing wrong with a queue or a move.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QueueError {
    /// The queue holds more jobs than the limit.
    #[error("the queue holds {count} jobs, but at most {max} are allowed")]
    TooMany { count: usize, max: usize },
    /// No job has this id.
    #[error("there is no job {job} in the queue")]
    Unknown { job: String },
    /// The job is not queued, or is not the next one in line.
    #[error("the job {job} is not queued, or is not the next one in line")]
    NotQueued { job: String },
    /// The job is in a state that does not allow the move.
    #[error("the job {job} is {state}, which does not allow the move {action}")]
    BadState {
        job: String,
        state: String,
        action: String,
    },
    /// A job is already active; the job named here was refused (or, in a file, is the second
    /// active job).
    #[error("the job {job} cannot be active while another job is active")]
    Active { job: String },
    /// A job id is not `q-` and digits.
    #[error("the job id \"{job}\" is not q- followed by 1 to 10 digits")]
    BadId { job: String },
    /// A scenario is not a plain name.
    #[error("the job {job} names a scenario that is not a plain name")]
    BadScenario { job: String },
    /// The queue breaks a rule, or the file cannot be read.
    #[error("the queue is damaged: {reason}")]
    Damaged { reason: String },
    /// The file was written by a newer version.
    #[error("the queue file has the version {found}, which is newer than this program reads")]
    NewerSchema { found: u32 },
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Ended and recorded as a run.
    Finished { run: String },
    /// Stopped on request, with a recorded checkpoint.
    Stopped { run: String },
    /// Ended with an error; the stable code says which.
    Failed { code: String },
    /// Cancelled.
    Cancelled,
    /// Lost; the stable code says why.
    Interrupted { code: String },
}

/// A move of the queue.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Adds jobs at the end.
    Enqueue { jobs: Vec<NewJob> },
    /// Moves a queued job to a position among the queued jobs (clamped to the ends).
    Reorder { id: String, to: usize },
    /// Cancels a queued job at once, or asks a running one for a graceful stop.
    Cancel { id: String },
    /// Asks a running job to stop.
    RequestStop {
        id: String,
        mode: StopMode,
        cause: StopCause,
    },
    /// Queued to starting, under the run id the container will carry.
    Begin { id: String, run_id: String },
    /// Starting to running.
    Started { id: String },
    /// Running to stopping: the stop was carried out.
    StopApplied { id: String },
    /// Stores a progress report on an active job.
    Progress { id: String, progress: Progress },
    /// Running or stopping to finishing: the container ended.
    ContainerEnded { id: String },
    /// The host cannot be reached.
    Unreachable { id: String },
    /// The host can be reached again.
    Reachable { id: String },
    /// Starting back to queued, after a start that failed.
    Retry { id: String },
    /// Starting back to queued because the compute host was busy: no try is used up, and a cancel
    /// asked for meanwhile ends the job.
    Requeue { id: String },
    /// Ends a job.
    Complete { id: String, outcome: Outcome },
    /// Drops every job that has ended.
    ClearFinished,
}

impl Action {
    /// The move as a snake_case word, as it appears in an error.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Enqueue { .. } => "enqueue",
            Self::Reorder { .. } => "reorder",
            Self::Cancel { .. } => "cancel",
            Self::RequestStop { .. } => "request_stop",
            Self::Begin { .. } => "begin",
            Self::Started { .. } => "started",
            Self::StopApplied { .. } => "stop_applied",
            Self::Progress { .. } => "progress",
            Self::ContainerEnded { .. } => "container_ended",
            Self::Unreachable { .. } => "unreachable",
            Self::Reachable { .. } => "reachable",
            Self::Retry { .. } => "retry",
            Self::Requeue { .. } => "requeue",
            Self::Complete { .. } => "complete",
            Self::ClearFinished => "clear_finished",
        }
    }
}

/// The job that is starting, running, stopping, finishing or unreachable, if there is one.
pub fn active(queue: &Queue) -> Option<&Job> {
    queue.jobs.iter().find(|j| j.state.is_active())
}

/// The first queued job, but only while no job is active: one job at a time.
pub fn next_to_start(queue: &Queue) -> Option<&Job> {
    if active(queue).is_some() {
        return None;
    }
    queue.jobs.iter().find(|j| j.state == JobState::Queued)
}

/// Applies one action and returns the new queue; the input is never changed. The result passes
/// [`check_queue`] or the move is an error.
pub fn apply(queue: &Queue, action: Action) -> Result<Queue, QueueError> {
    let mut next = queue.clone();
    let name = action.name();
    match action {
        Action::Enqueue { jobs } => enqueue(&mut next, jobs)?,
        Action::Reorder { id, to } => reorder(&mut next, &id, to)?,
        Action::Cancel { id } => cancel(&mut next, &id)?,
        Action::RequestStop { id, mode, cause } => {
            let job = find(&mut next, &id)?;
            if !matches!(job.state, JobState::Running | JobState::Unreachable) {
                return Err(bad(job, name));
            }
            set_stop(job, mode, cause);
        }
        Action::Begin { id, run_id } => begin(&mut next, &id, run_id)?,
        Action::Started { id } => {
            step(
                &mut next,
                &id,
                name,
                &[JobState::Starting],
                JobState::Running,
            )?;
        }
        Action::StopApplied { id } => {
            let job = find(&mut next, &id)?;
            if job.state != JobState::Running || job.stop.is_none() {
                return Err(bad(job, name));
            }
            job.state = JobState::Stopping;
        }
        Action::Progress { id, progress } => {
            let job = find(&mut next, &id)?;
            if !job.state.is_active() {
                return Err(bad(job, name));
            }
            job.progress = Some(progress);
        }
        Action::ContainerEnded { id } => {
            let from = [JobState::Running, JobState::Stopping];
            step(&mut next, &id, name, &from, JobState::Finishing)?;
        }
        Action::Unreachable { id } => {
            let from = [JobState::Running, JobState::Stopping];
            step(&mut next, &id, name, &from, JobState::Unreachable)?;
        }
        Action::Reachable { id } => {
            step(
                &mut next,
                &id,
                name,
                &[JobState::Unreachable],
                JobState::Running,
            )?;
        }
        Action::Retry { id } => {
            let job = find(&mut next, &id)?;
            if job.state != JobState::Starting || job.attempts >= MAX_ATTEMPTS {
                return Err(bad(job, name));
            }
            job.state = JobState::Queued;
            job.attempts += 1;
            job.run_id = None;
        }
        Action::Requeue { id } => {
            let job = find(&mut next, &id)?;
            if job.state != JobState::Starting {
                return Err(bad(job, name));
            }
            job.run_id = None;
            job.state = if job.stop.take().is_some() {
                JobState::Cancelled
            } else {
                JobState::Queued
            };
        }
        Action::Complete { id, outcome } => complete(&mut next, &id, outcome)?,
        Action::ClearFinished => next.jobs.retain(|j| !j.state.is_terminal()),
    }
    check_queue(&next)?;
    Ok(next)
}

fn find<'a>(queue: &'a mut Queue, id: &str) -> Result<&'a mut Job, QueueError> {
    queue
        .jobs
        .iter_mut()
        .find(|j| j.id == id)
        .ok_or_else(|| QueueError::Unknown {
            job: id.chars().take(40).collect(),
        })
}

fn bad(job: &Job, action: &str) -> QueueError {
    QueueError::BadState {
        job: job.id.clone(),
        state: job.state.as_str().to_owned(),
        action: action.to_owned(),
    }
}

/// Moves a job from one of the `from` states to `to`.
fn step<'a>(
    queue: &'a mut Queue,
    id: &str,
    name: &str,
    from: &[JobState],
    to: JobState,
) -> Result<&'a mut Job, QueueError> {
    let job = find(queue, id)?;
    if !from.contains(&job.state) {
        return Err(bad(job, name));
    }
    job.state = to;
    Ok(job)
}

/// Records a stop request. The first request stays, except that a stop without writing replaces a
/// stop that would write: fields that cannot be trusted must not be written.
fn set_stop(job: &mut Job, mode: StopMode, cause: StopCause) {
    let replace = match &job.stop {
        None => true,
        Some(old) => old.mode == StopMode::WriteNow && mode == StopMode::NoWrite,
    };
    if replace {
        job.stop = Some(StopRequest { mode, cause });
    }
}

fn enqueue(queue: &mut Queue, jobs: Vec<NewJob>) -> Result<(), QueueError> {
    // Never lower than a number already in use, even when the file's counter was edited down.
    let used = queue
        .jobs
        .iter()
        .filter_map(|j| id_number(&j.id))
        .max()
        .unwrap_or(0);
    let mut number = queue.next_number.max(used.saturating_add(1)).max(1);
    for new in jobs {
        let id = format!("q-{number:03}");
        number = number.checked_add(1).ok_or_else(|| QueueError::Damaged {
            reason: "the job numbers are used up".to_owned(),
        })?;
        queue.jobs.push(Job {
            id,
            scenario: new.scenario,
            kind: new.kind,
            origin: new.origin,
            backend: new.backend,
            state: JobState::Queued,
            attempts: 0,
            run_id: None,
            run: None,
            code: None,
            stop: None,
            progress: None,
        });
    }
    queue.next_number = number;
    Ok(())
}

fn reorder(queue: &mut Queue, id: &str, to: usize) -> Result<(), QueueError> {
    let Some(job) = queue.jobs.iter().find(|j| j.id == id) else {
        return Err(QueueError::Unknown {
            job: id.chars().take(40).collect(),
        });
    };
    if job.state != JobState::Queued {
        return Err(QueueError::NotQueued {
            job: job.id.clone(),
        });
    }
    // The queued jobs keep the places they hold among the other jobs; only their order changes.
    let slots: Vec<usize> = queue
        .jobs
        .iter()
        .enumerate()
        .filter(|(_, j)| j.state == JobState::Queued)
        .map(|(i, _)| i)
        .collect();
    let mut queued: Vec<Job> = slots.iter().map(|&i| queue.jobs[i].clone()).collect();
    let Some(from) = queued.iter().position(|j| j.id == id) else {
        return Err(QueueError::NotQueued { job: id.to_owned() });
    };
    let moved = queued.remove(from);
    queued.insert(to.min(queued.len()), moved);
    for (&slot, job) in slots.iter().zip(queued) {
        queue.jobs[slot] = job;
    }
    Ok(())
}

fn cancel(queue: &mut Queue, id: &str) -> Result<(), QueueError> {
    let job = find(queue, id)?;
    match job.state {
        JobState::Queued => job.state = JobState::Cancelled,
        JobState::Starting | JobState::Running | JobState::Stopping | JobState::Unreachable => {
            set_stop(job, StopMode::WriteNow, StopCause::Cancel);
        }
        _ => return Err(bad(job, "cancel")),
    }
    Ok(())
}

fn begin(queue: &mut Queue, id: &str, run_id: String) -> Result<(), QueueError> {
    let Some(job) = queue.jobs.iter().find(|j| j.id == id) else {
        return Err(QueueError::Unknown {
            job: id.chars().take(40).collect(),
        });
    };
    if job.state != JobState::Queued {
        return Err(bad(job, "begin"));
    }
    if active(queue).is_some() {
        return Err(QueueError::Active {
            job: job.id.clone(),
        });
    }
    if next_to_start(queue).map(|j| j.id.as_str()) != Some(id) {
        return Err(QueueError::NotQueued {
            job: job.id.clone(),
        });
    }
    let job = find(queue, id)?;
    job.state = JobState::Starting;
    job.run_id = Some(run_id);
    Ok(())
}

fn complete(queue: &mut Queue, id: &str, outcome: Outcome) -> Result<(), QueueError> {
    let job = find(queue, id)?;
    let allowed = match &outcome {
        Outcome::Finished { .. } | Outcome::Stopped { .. } => job.state == JobState::Finishing,
        Outcome::Failed { .. } | Outcome::Cancelled => !job.state.is_terminal(),
        Outcome::Interrupted { .. } => job.state.is_active(),
    };
    if !allowed {
        return Err(bad(job, "complete"));
    }
    match outcome {
        Outcome::Finished { run } => {
            job.state = JobState::Finished;
            job.run = Some(run);
        }
        Outcome::Stopped { run } => {
            job.state = JobState::Stopped;
            job.run = Some(run);
        }
        Outcome::Failed { code } => {
            job.state = JobState::Failed;
            job.code = Some(code);
        }
        Outcome::Cancelled => job.state = JobState::Cancelled,
        Outcome::Interrupted { code } => {
            job.state = JobState::Interrupted;
            job.code = Some(code);
        }
    }
    Ok(())
}
