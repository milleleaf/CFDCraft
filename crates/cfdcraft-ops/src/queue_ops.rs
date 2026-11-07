//! The job queue of a document: add to it, list it, move a waiting job, cancel a job and ask a
//! running job to stop. Every function takes the path of the document and returns a `DocOutcome`;
//! the command line and the tool server are thin adapters over them, and the background runner
//! advances what they write.
//!
//! Adding plans each job with the same planner a solve uses, so a request the planner refuses or
//! finds already up to date never enters the queue. A sweep becomes one job per point, in point
//! order. A job asked for twice while the first is still waiting or running is added once. All
//! the jobs of one request go in together or not at all.
//!
//! Finding codes of this family: 001 a request is refused, because of its shape, the pack it names
//! or the limit of jobs (class 2); 002 no such job (class 2); 003 the job is in a state that does
//! not allow the action (class 2); 004 the queue file is damaged, newer than this build or cannot
//! be used (class 4); 005 nothing to add, because everything asked for is up to date or already
//! queued (a reading, class 0). A variant that is not listed is `CFD-SCN-001` and a sweep that is
//! not stored `CFD-SWP-004`. A report names the document and jobs by id, never the store folder.

use std::path::Path;

use cfdcraft_doc::{Opened, open_stamped};
use cfdcraft_model::Severity;
use cfdcraft_queue::{
    Action, Job, JobKind, JobState, NewJob, Origin, Queue, QueueError, StopCause, StopMode, active,
};
use cfdcraft_scenarios::BASE;
use cfdcraft_text::Line;

use crate::documents::{
    DocOutcome, finding, is_pack_name, migration_with, problem, refused, shown, study_hash,
};
use crate::jobs::{JobAction, JobOptions, open_refusal, plan_scenario_job_with};
use crate::queue_file::{QueueFile, QueueFileError};
use crate::report::{ExitClass, Report};
use crate::repro::{JobView, QueueData, Repro};

/// A queue request is refused.
pub const QUEUE_REFUSED: &str = "CFD-QUEUE-001";
/// No job has the id.
pub const QUEUE_UNKNOWN_JOB: &str = "CFD-QUEUE-002";
/// The job is in a state that does not allow the action.
pub const QUEUE_NOT_ALLOWED: &str = "CFD-QUEUE-003";
/// The queue file is damaged, newer than this build or cannot be used.
pub const QUEUE_DAMAGED: &str = "CFD-QUEUE-004";
/// Nothing to add: everything asked for is up to date or already queued.
pub const QUEUE_NOTHING: &str = "CFD-QUEUE-005";
/// A run cannot be continued: it did not stop on request, its saved state is gone, or its tools
/// are not those of this build.
pub const CONTINUE_REFUSED: &str = "CFD-CONT-001";

const ADD: &str = "queue-add";
const LIST: &str = "queue-list";
const REORDER: &str = "queue-reorder";
const CANCEL: &str = "queue-cancel";
const STOP: &str = "queue-stop";

/// What to put in the queue. With no field set it is the study itself; `scenario` names one
/// variant, `sweep` names every point of a stored sweep, and `continue_run` names a stopped run to
/// continue. The three are not combined.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddRequest {
    /// The variant to solve; `base` and none mean the study itself.
    pub scenario: Option<String>,
    /// The stored sweep whose points are all solved, in point order.
    pub sweep: Option<String>,
    /// Make a result again that was pruned on purpose.
    pub remake_pruned: bool,
    /// The stopped run whose saved state is continued.
    pub continue_run: Option<String>,
}

type Reply = Result<DocOutcome, Box<DocOutcome>>;

/// An id given by a person, shortened and with control characters replaced, for a message.
fn clean(id: &str) -> String {
    id.chars()
        .take(40)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn refuse(operation: &str, reason: &str) -> Box<DocOutcome> {
    let line = Line::QueueRefused {
        reason: reason.to_owned(),
    };
    Box::new(refused(operation, problem((QUEUE_REFUSED, line))))
}

/// Open the document at `path`. A pack is refused: its queue would have no store to live in.
fn load(operation: &str, path: &Path) -> Result<Opened, Box<DocOutcome>> {
    let packed = "it is a pack; unpack it and queue jobs for the unpacked document";
    if is_pack_name(path) {
        return Err(refuse(operation, packed));
    }
    let (opened, _) =
        open_stamped(path).map_err(|e| Box::new(open_refusal(operation, &e, path)))?;
    if opened.packed_entries > 0 {
        return Err(refuse(operation, packed));
    }
    Ok(opened)
}

fn job_view(job: &Job) -> JobView {
    JobView {
        id: job.id.clone(),
        scenario: job.scenario.clone(),
        kind: match &job.kind {
            JobKind::Solve => "solve",
            JobKind::Continue { .. } => "continue",
        }
        .to_owned(),
        state: job.state.as_str().to_owned(),
        origin: job
            .origin
            .as_ref()
            .map(|origin| format!("{} {}", origin.sweep, origin.index)),
        run: job.run.clone(),
        code: job.code.clone(),
        progress_line: job.progress.as_ref().and_then(|p| p.summary.clone()),
    }
}

fn queue_data(file: &QueueFile, queue: &Queue) -> QueueData {
    QueueData {
        jobs: queue.jobs.iter().map(job_view).collect(),
        active: active(queue).map(|job| job.id.clone()),
        drain: file.drain_running(),
    }
}

fn waiting(queue: &Queue) -> usize {
    queue
        .jobs
        .iter()
        .filter(|job| job.state == JobState::Queued)
        .count()
}

/// The report that carries the queue as it is after the operation.
fn queue_report(
    operation: &str,
    path: &Path,
    opened: &Opened,
    file: &QueueFile,
    queue: &Queue,
) -> Report {
    Report::from_diagnostics(operation, Vec::new(), study_hash(&opened.document.study))
        .with_migration(migration_with(&opened.notes, false))
        .with_artifact("document", shown(path))
        .with_repro(Repro::Queue(Box::new(queue_data(file, queue))))
}

/// The refusal for a queue move or file that failed. `action` is the word that ends the sentence
/// "so it cannot be ..." when a job is in the wrong state.
fn failure(operation: &str, error: QueueFileError, action: &str) -> Box<DocOutcome> {
    let not_allowed = |job: String, state: String| {
        let line = Line::QueueNotAllowed {
            job,
            state,
            action: action.to_owned(),
        };
        (QUEUE_NOT_ALLOWED, line)
    };
    let damaged = |reason: String| (QUEUE_DAMAGED, Line::QueueDamaged { reason });
    let found = match error {
        QueueFileError::Queue { source } => match source {
            QueueError::Unknown { job } => (QUEUE_UNKNOWN_JOB, Line::QueueUnknownJob { job }),
            QueueError::BadState { job, state, .. } => not_allowed(job, state),
            QueueError::NotQueued { job } => not_allowed(job, "not waiting".to_owned()),
            QueueError::Damaged { reason } => damaged(reason),
            newer @ QueueError::NewerSchema { .. } => damaged(newer.to_string()),
            other => {
                let line = Line::QueueRefused {
                    reason: other.to_string(),
                };
                (QUEUE_REFUSED, line)
            }
        },
        QueueFileError::Busy => damaged(QueueFileError::Busy.to_string()),
        QueueFileError::Io { reason } => damaged(reason),
    };
    let infrastructure = found.0 == QUEUE_DAMAGED;
    let mut outcome = refused(operation, problem(found));
    if infrastructure {
        outcome.report.set_class(ExitClass::Infrastructure);
    }
    Box::new(outcome)
}

/// One thing a request would solve.
struct Candidate {
    scenario: Option<String>,
    origin: Option<Origin>,
}

/// What `request` asks for, in the order the jobs should start.
fn candidates(opened: &Opened, request: &AddRequest) -> Result<Vec<Candidate>, Box<DocOutcome>> {
    match (&request.scenario, &request.sweep) {
        (Some(_), Some(_)) => Err(refuse(
            ADD,
            "ask for a variant or for a sweep, not for both at once",
        )),
        (None, None) => Ok(vec![Candidate {
            scenario: None,
            origin: None,
        }]),
        (Some(scenario), None) => Ok(vec![Candidate {
            scenario: (scenario != BASE).then(|| scenario.clone()),
            origin: None,
        }]),
        (None, Some(id)) => {
            let artifacts = &opened.document.artifacts;
            if !artifacts.sweeps.iter().any(|sweep| sweep.id == *id) {
                let sweep = clean(id);
                let line = Line::SweepUnknown { sweep };
                return Err(Box::new(refused(ADD, problem(("CFD-SWP-004", line)))));
            }
            let mut points: Vec<_> = artifacts
                .scenarios
                .iter()
                .filter_map(|scenario| scenario.origin.as_ref().map(|o| (scenario, o)))
                .filter(|(_, origin)| origin.sweep == *id)
                .collect();
            points.sort_by_key(|(_, origin)| origin.index);
            Ok(points
                .into_iter()
                .map(|(scenario, origin)| Candidate {
                    scenario: Some(scenario.id.clone()),
                    origin: Some(Origin {
                        sweep: origin.sweep.clone(),
                        index: origin.index,
                    }),
                })
                .collect())
        }
    }
}

/// Put jobs in the queue of the document at `path`.
///
/// Each candidate is planned the way a solve plans it. One that is up to date is left out and
/// counted; one the planner refuses ends the whole request with the planner's own report; the
/// rest become jobs, unless an equal job is already waiting or running. All of them are added in
/// one change, so a request over the limit of jobs adds none. When nothing is added the answer is
/// the reading `CFD-QUEUE-005` and the queue is not touched.
pub fn queue_add(path: &Path, request: &AddRequest) -> DocOutcome {
    add_inner(path, request).unwrap_or_else(|refusal| *refusal)
}

fn add_inner(path: &Path, request: &AddRequest) -> Reply {
    let opened = load(ADD, path)?;
    if let Some(run) = &request.continue_run {
        return add_continue(path, &opened, request, run);
    }
    let candidates = candidates(&opened, request)?;
    let options = JobOptions {
        remake_pruned: request.remake_pruned,
    };
    let mut wanted = Vec::new();
    for candidate in candidates {
        let scenario = candidate.scenario.as_deref();
        match plan_scenario_job_with(path, scenario, JobAction::Solve, &options) {
            // The planned job is dropped: the runner plans again when the job starts.
            Ok(_) => wanted.push(NewJob {
                scenario: candidate.scenario,
                kind: JobKind::Solve,
                origin: candidate.origin,
            }),
            Err(outcome) if matches!(outcome.line, Some(Line::NothingToRun)) => {}
            Err(outcome) => return Err(outcome),
        }
    }

    enqueue(path, &opened, wanted)
}

/// Queue the continuation of the stopped run `run`. The planner decides whether it can be
/// continued; what it refuses is the answer, unchanged.
fn add_continue(path: &Path, opened: &Opened, request: &AddRequest, run: &str) -> Reply {
    if request.scenario.is_some() || request.sweep.is_some() {
        return Err(refuse(
            ADD,
            "ask to continue a run on its own, not together with a variant or a sweep",
        ));
    }
    let options = JobOptions::default();
    let action = JobAction::Continue {
        run: run.to_owned(),
    };
    let planned = plan_scenario_job_with(path, None, action, &options)?;
    let wanted = vec![NewJob {
        scenario: planned.scenario().map(str::to_owned),
        kind: JobKind::Continue {
            run: run.to_owned(),
        },
        origin: None,
    }];
    enqueue(path, opened, wanted)
}

/// Add `wanted` to the queue in one change, leaving out a job equal to one that is still waiting
/// or running, and answer with the queue as it is afterwards.
fn enqueue(path: &Path, opened: &Opened, wanted: Vec<NewJob>) -> Reply {
    let file = QueueFile::new(path);
    let mut added = 0;
    let queue = file
        .update(|queue| {
            let open_for = |new: &NewJob| {
                queue.jobs.iter().any(|job| {
                    !job.state.is_terminal() && job.kind == new.kind && job.scenario == new.scenario
                })
            };
            let jobs: Vec<NewJob> = wanted.into_iter().filter(|new| !open_for(new)).collect();
            added = jobs.len();
            Ok(if jobs.is_empty() {
                Vec::new()
            } else {
                vec![Action::Enqueue { jobs }]
            })
        })
        .map_err(|e| failure(ADD, e, "added"))?;

    let mut report = queue_report(ADD, path, opened, &file, &queue);
    let line = if added == 0 {
        let note = finding(
            Severity::Warning,
            "queue",
            (QUEUE_NOTHING, Line::QueueNothingToAdd),
        );
        report.diagnostics.push(note);
        Line::QueueNothingToAdd
    } else {
        Line::QueueAdded {
            jobs: added.to_string(),
            queued: waiting(&queue).to_string(),
        }
    };
    Ok(DocOutcome {
        report,
        line: Some(line),
    })
}

/// The queue of the document at `path`, in the order its jobs start. A document that never had a
/// queue answers with an empty one, and nothing is written.
pub fn queue_list(path: &Path) -> DocOutcome {
    list_inner(path).unwrap_or_else(|refusal| *refusal)
}

fn list_inner(path: &Path) -> Reply {
    let opened = load(LIST, path)?;
    let file = QueueFile::new(path);
    let queue = file.read().map_err(|e| failure(LIST, e, "listed"))?;
    Ok(DocOutcome {
        report: queue_report(LIST, path, &opened, &file, &queue),
        line: None,
    })
}

/// The job `id` of `queue`, or the error that says there is none.
fn find<'a>(queue: &'a Queue, id: &str) -> Result<&'a Job, QueueError> {
    queue
        .jobs
        .iter()
        .find(|job| job.id == id)
        .ok_or_else(|| QueueError::Unknown { job: clean(id) })
}

fn not_in_state(job: &Job, action: &str) -> QueueError {
    QueueError::BadState {
        job: job.id.clone(),
        state: job.state.as_str().to_owned(),
        action: action.to_owned(),
    }
}

/// Move the waiting job `job` to place `to` among the waiting jobs, counted from 0 and clamped to
/// the ends. Only a waiting job can be moved.
pub fn queue_reorder(path: &Path, job: &str, to: usize) -> DocOutcome {
    reorder_inner(path, job, to).unwrap_or_else(|refusal| *refusal)
}

fn reorder_inner(path: &Path, job: &str, to: usize) -> Reply {
    let opened = load(REORDER, path)?;
    let file = QueueFile::new(path);
    let queue = file
        .update(|queue| {
            let found = find(queue, job)?;
            if found.state != JobState::Queued {
                return Err(not_in_state(found, "moved"));
            }
            Ok(vec![Action::Reorder {
                id: found.id.clone(),
                to,
            }])
        })
        .map_err(|e| failure(REORDER, e, "moved"))?;
    let place = queue
        .jobs
        .iter()
        .filter(|other| other.state == JobState::Queued)
        .position(|other| other.id == job)
        .map_or(0, |index| index + 1);
    let line = Line::QueueReordered {
        job: job.to_owned(),
        position: place.to_string(),
    };
    Ok(DocOutcome {
        report: queue_report(REORDER, path, &opened, &file, &queue),
        line: Some(line),
    })
}

/// Cancel the job `job`. A waiting job ends at once. A job that is starting or running is asked to
/// write its latest results and stop, and ends as stopped when it has. A job that has ended
/// cannot be cancelled.
pub fn queue_cancel(path: &Path, job: &str) -> DocOutcome {
    cancel_inner(path, job).unwrap_or_else(|refusal| *refusal)
}

fn cancel_inner(path: &Path, job: &str) -> Reply {
    let opened = load(CANCEL, path)?;
    let file = QueueFile::new(path);
    let mut waiting_job = false;
    let queue = file
        .update(|queue| {
            let found = find(queue, job)?;
            match found.state {
                JobState::Queued => waiting_job = true,
                JobState::Starting
                | JobState::Running
                | JobState::Stopping
                | JobState::Unreachable => {}
                _ => return Err(not_in_state(found, "cancelled")),
            }
            Ok(vec![Action::Cancel {
                id: found.id.clone(),
            }])
        })
        .map_err(|e| failure(CANCEL, e, "cancelled"))?;
    let job = job.to_owned();
    let line = if waiting_job {
        Line::QueueCancelled { job }
    } else {
        Line::QueueStopRequested { job }
    };
    Ok(DocOutcome {
        report: queue_report(CANCEL, path, &opened, &file, &queue),
        line: Some(line),
    })
}

/// Ask the running job `job` to write its latest results and stop. Only a running job can be
/// asked; asking again while the stop is on its way is accepted and changes nothing.
pub fn queue_stop(path: &Path, job: &str) -> DocOutcome {
    stop_inner(path, job).unwrap_or_else(|refusal| *refusal)
}

fn stop_inner(path: &Path, job: &str) -> Reply {
    let opened = load(STOP, path)?;
    let file = QueueFile::new(path);
    let queue = file
        .update(|queue| {
            let found = find(queue, job)?;
            match found.state {
                JobState::Running | JobState::Unreachable => Ok(vec![Action::RequestStop {
                    id: found.id.clone(),
                    mode: StopMode::WriteNow,
                    cause: StopCause::User,
                }]),
                JobState::Stopping => Ok(Vec::new()),
                _ => Err(not_in_state(found, "stopped")),
            }
        })
        .map_err(|e| failure(STOP, e, "stopped"))?;
    let line = Line::QueueStopRequested {
        job: job.to_owned(),
    };
    Ok(DocOutcome {
        report: queue_report(STOP, path, &opened, &file, &queue),
        line: Some(line),
    })
}
