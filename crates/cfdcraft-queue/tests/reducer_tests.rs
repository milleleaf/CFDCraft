use cfdcraft_queue::{
    Action, Job, JobKind, JobState, NewJob, Outcome, Progress, Queue, QueueError, StopCause,
    StopMode, StopRequest, apply, next_to_start,
};

fn solve() -> NewJob {
    NewJob {
        scenario: None,
        kind: JobKind::Solve,
        origin: None,
    }
}

/// A queue with `n` queued solves.
fn queued(n: usize) -> Queue {
    let jobs = (0..n).map(|_| solve()).collect();
    apply(&Queue::new(), Action::Enqueue { jobs }).unwrap()
}

/// Applies a move that must succeed, and checks the input was left alone.
fn go(queue: &Queue, action: Action) -> Queue {
    let before = queue.clone();
    let next = apply(queue, action).unwrap();
    assert_eq!(queue, &before);
    next
}

fn job<'a>(queue: &'a Queue, id: &str) -> &'a Job {
    queue.jobs.iter().find(|j| j.id == id).unwrap()
}

fn begin(id: &str) -> Action {
    Action::Begin {
        id: id.to_owned(),
        run_id: format!("cfdcraft-demo-{id}"),
    }
}

fn started(id: &str) -> Action {
    Action::Started { id: id.to_owned() }
}

fn ended(id: &str) -> Action {
    Action::ContainerEnded { id: id.to_owned() }
}

fn running(queue: &Queue, id: &str) -> Queue {
    let queue = go(queue, begin(id));
    go(&queue, started(id))
}

#[test]
fn a_job_runs_from_queued_to_finished() {
    let queue = queued(1);
    assert_eq!(job(&queue, "q-001").state, JobState::Queued);

    let queue = go(&queue, begin("q-001"));
    assert_eq!(job(&queue, "q-001").state, JobState::Starting);
    assert_eq!(
        job(&queue, "q-001").run_id.as_deref(),
        Some("cfdcraft-demo-q-001")
    );

    let queue = go(&queue, started("q-001"));
    assert_eq!(job(&queue, "q-001").state, JobState::Running);

    let queue = go(&queue, ended("q-001"));
    assert_eq!(job(&queue, "q-001").state, JobState::Finishing);

    let queue = go(
        &queue,
        Action::Complete {
            id: "q-001".to_owned(),
            outcome: Outcome::Finished {
                run: "run-1".to_owned(),
            },
        },
    );
    let done = job(&queue, "q-001");
    assert_eq!(done.state, JobState::Finished);
    assert_eq!(done.run.as_deref(), Some("run-1"));
    assert_eq!(done.run_id.as_deref(), Some("cfdcraft-demo-q-001"));
}

#[test]
fn ids_count_up_and_are_never_reused() {
    let queue = queued(3);
    let ids: Vec<&str> = queue.jobs.iter().map(|j| j.id.as_str()).collect();
    assert_eq!(ids, ["q-001", "q-002", "q-003"]);

    let mut queue = queue;
    for id in ["q-001", "q-002", "q-003"] {
        queue = go(&queue, Action::Cancel { id: id.to_owned() });
    }
    let queue = go(&queue, Action::ClearFinished);
    assert!(queue.jobs.is_empty());

    let queue = go(
        &queue,
        Action::Enqueue {
            jobs: vec![solve()],
        },
    );
    assert_eq!(queue.jobs.len(), 1);
    assert_eq!(queue.jobs[0].id, "q-004");
}

#[test]
fn only_one_job_is_active() {
    let queue = queued(2);
    assert_eq!(next_to_start(&queue).map(|j| j.id.as_str()), Some("q-001"));

    let queue = go(&queue, begin("q-001"));
    assert!(next_to_start(&queue).is_none());
    assert_eq!(
        apply(&queue, begin("q-002")).unwrap_err(),
        QueueError::Active {
            job: "q-002".to_owned()
        }
    );

    let queue = go(&queue, started("q-001"));
    assert!(next_to_start(&queue).is_none());

    let stopping = go(
        &queue,
        Action::RequestStop {
            id: "q-001".to_owned(),
            mode: StopMode::WriteNow,
            cause: StopCause::User,
        },
    );
    let stopping = go(
        &stopping,
        Action::StopApplied {
            id: "q-001".to_owned(),
        },
    );
    assert_eq!(job(&stopping, "q-001").state, JobState::Stopping);
    assert!(next_to_start(&stopping).is_none());

    let unreachable = go(
        &queue,
        Action::Unreachable {
            id: "q-001".to_owned(),
        },
    );
    assert_eq!(job(&unreachable, "q-001").state, JobState::Unreachable);
    assert!(next_to_start(&unreachable).is_none());

    let queue = go(&queue, ended("q-001"));
    assert_eq!(job(&queue, "q-001").state, JobState::Finishing);
    assert!(next_to_start(&queue).is_none());

    let queue = go(
        &queue,
        Action::Complete {
            id: "q-001".to_owned(),
            outcome: Outcome::Finished {
                run: "run-1".to_owned(),
            },
        },
    );
    assert_eq!(next_to_start(&queue).map(|j| j.id.as_str()), Some("q-002"));
}

#[test]
fn a_continue_job_carries_its_run() {
    let kind = JobKind::Continue {
        run: "run-3".to_owned(),
    };
    let queue = go(
        &Queue::new(),
        Action::Enqueue {
            jobs: vec![NewJob {
                scenario: Some("base".to_owned()),
                kind: kind.clone(),
                origin: None,
            }],
        },
    );
    assert_eq!(job(&queue, "q-001").kind, kind);
    assert_eq!(job(&queue, "q-001").scenario.as_deref(), Some("base"));
}

#[test]
fn progress_is_kept_for_an_active_job_only() {
    let report = Progress {
        time: Some(0.5),
        iterations: 40,
        settled: 1,
        monitors: 3,
        ..Progress::default()
    };
    let progress = |id: &str| Action::Progress {
        id: id.to_owned(),
        progress: report.clone(),
    };

    let queue = queued(2);
    assert_eq!(
        apply(&queue, progress("q-001")).unwrap_err(),
        QueueError::BadState {
            job: "q-001".to_owned(),
            state: "queued".to_owned(),
            action: "progress".to_owned(),
        }
    );

    let queue = running(&queue, "q-001");
    let queue = go(&queue, progress("q-001"));
    assert_eq!(job(&queue, "q-001").progress.as_ref(), Some(&report));

    let queue = go(&queue, ended("q-001"));
    let queue = go(
        &queue,
        Action::Complete {
            id: "q-001".to_owned(),
            outcome: Outcome::Finished {
                run: "run-1".to_owned(),
            },
        },
    );
    assert!(matches!(
        apply(&queue, progress("q-001")).unwrap_err(),
        QueueError::BadState { .. }
    ));
}

#[test]
fn apply_never_changes_its_input() {
    let queue = running(&queued(2), "q-001");
    let before = queue.clone();

    let ok = apply(
        &queue,
        Action::Enqueue {
            jobs: vec![solve()],
        },
    )
    .unwrap();
    assert_eq!(queue, before);
    assert_eq!(ok.jobs.len(), 3);

    let err = apply(&queue, begin("q-002")).unwrap_err();
    assert!(matches!(err, QueueError::Active { .. }));
    assert_eq!(queue, before);

    let stop = StopRequest {
        mode: StopMode::WriteNow,
        cause: StopCause::User,
    };
    let stopped = apply(
        &queue,
        Action::RequestStop {
            id: "q-001".to_owned(),
            mode: stop.mode,
            cause: stop.cause,
        },
    )
    .unwrap();
    assert_eq!(queue, before);
    assert_eq!(job(&stopped, "q-001").stop, Some(stop));
}
