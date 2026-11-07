//! A queued job runs on the backend it names, and a job that names one that does not exist fails
//! with that finding's code and nothing is recorded.

use cfdcraft_drain::TickOutcome;
use cfdcraft_queue::{Action, JobKind, JobState, NewJob};

mod support;
use support::*;

/// Puts one job for the study in the queue, naming `backend`.
fn enqueue_on(w: &World, backend: &str) -> String {
    w.file()
        .update(|_| {
            Ok(vec![Action::Enqueue {
                jobs: vec![NewJob {
                    scenario: None,
                    kind: JobKind::Solve,
                    origin: None,
                    backend: Some(backend.to_owned()),
                }],
            }])
        })
        .unwrap();
    w.queue().jobs.last().unwrap().id.clone()
}

#[test]
fn a_job_naming_a_backend_that_is_gone_fails_with_its_code() {
    let w = world("backend-gone");
    let id = enqueue_on(&w, "gone-1");
    let mut drain = w.drain();

    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some("CFD-BACKEND-001"));
    assert!(w.runs().is_empty(), "nothing is recorded");
}

#[test]
fn a_job_on_a_named_backend_runs_and_is_recorded_with_it() {
    let w = world("backend-named");
    let id = enqueue_on(&w, "esi-v2606");
    let mut drain = w.drain();

    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    assert_eq!(w.state(&id), JobState::Running);
    let run_id = w.run_id(&id);
    w.play(&run_id);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);

    let job = w.job(&id);
    assert_eq!(job.state, JobState::Finished, "{job:?}");
    assert_eq!(job.backend.as_deref(), Some("esi-v2606"));
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].provenance.tool.dialect, "esi-v2606");
}
