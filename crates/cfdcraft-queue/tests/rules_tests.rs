use cfdcraft_queue::{
    Action, Job, JobKind, JobState, MAX_JOBS, NewJob, Outcome, Progress, Queue, QueueError,
    StopCause, StopMode, StopRequest, apply, check_queue,
};

fn solve() -> NewJob {
    NewJob {
        scenario: None,
        kind: JobKind::Solve,
        origin: None,
    }
}

fn queued(n: usize) -> Queue {
    let jobs = (0..n).map(|_| solve()).collect();
    apply(&Queue::new(), Action::Enqueue { jobs }).unwrap()
}

fn go(queue: &Queue, action: Action) -> Queue {
    let before = queue.clone();
    let next = apply(queue, action).unwrap();
    assert_eq!(queue, &before);
    next
}

fn job<'a>(queue: &'a Queue, id: &str) -> &'a Job {
    queue.jobs.iter().find(|j| j.id == id).unwrap()
}

fn order(queue: &Queue) -> Vec<&str> {
    queue.jobs.iter().map(|j| j.id.as_str()).collect()
}

fn id(text: &str) -> String {
    text.to_owned()
}

fn begin(text: &str) -> Action {
    Action::Begin {
        id: id(text),
        run_id: format!("cfdcraft-demo-{text}"),
    }
}

fn running(queue: &Queue, text: &str) -> Queue {
    let queue = go(queue, begin(text));
    go(&queue, Action::Started { id: id(text) })
}

fn stop_request(text: &str, mode: StopMode, cause: StopCause) -> Action {
    Action::RequestStop {
        id: id(text),
        mode,
        cause,
    }
}

fn complete(text: &str, outcome: Outcome) -> Action {
    Action::Complete {
        id: id(text),
        outcome,
    }
}

fn bad_state(text: &str, state: &str, action: &str) -> QueueError {
    QueueError::BadState {
        job: id(text),
        state: id(state),
        action: id(action),
    }
}

#[test]
fn reorder_moves_a_queued_job_among_queued_jobs() {
    let queue = queued(3);
    let moved = go(
        &queue,
        Action::Reorder {
            id: id("q-003"),
            to: 0,
        },
    );
    assert_eq!(order(&moved), ["q-003", "q-001", "q-002"]);

    let clamped = go(
        &queue,
        Action::Reorder {
            id: id("q-001"),
            to: 99,
        },
    );
    assert_eq!(order(&clamped), ["q-002", "q-003", "q-001"]);

    // A running job keeps its place; the queued jobs reorder around it.
    let busy = running(&queue, "q-001");
    let moved = go(
        &busy,
        Action::Reorder {
            id: id("q-003"),
            to: 0,
        },
    );
    assert_eq!(order(&moved), ["q-001", "q-003", "q-002"]);
}

#[test]
fn a_job_that_is_not_queued_cannot_be_reordered() {
    let busy = running(&queued(2), "q-001");
    let err = apply(
        &busy,
        Action::Reorder {
            id: id("q-001"),
            to: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err, QueueError::NotQueued { job: id("q-001") });

    let cancelled = go(&busy, Action::Cancel { id: id("q-002") });
    let err = apply(
        &cancelled,
        Action::Reorder {
            id: id("q-002"),
            to: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err, QueueError::NotQueued { job: id("q-002") });

    let err = apply(
        &busy,
        Action::Reorder {
            id: id("q-009"),
            to: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err, QueueError::Unknown { job: id("q-009") });
}

#[test]
fn cancelling_a_queued_job_ends_it() {
    let queue = go(&queued(2), Action::Cancel { id: id("q-002") });
    let cancelled = job(&queue, "q-002");
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert!(cancelled.stop.is_none());
    assert_eq!(job(&queue, "q-001").state, JobState::Queued);

    // An ended job cannot be cancelled again.
    let err = apply(&queue, Action::Cancel { id: id("q-002") }).unwrap_err();
    assert_eq!(err, bad_state("q-002", "cancelled", "cancel"));
}

#[test]
fn cancelling_a_running_job_asks_for_a_write_now_stop() {
    let queue = running(&queued(1), "q-001");
    let asked = go(&queue, Action::Cancel { id: id("q-001") });
    let held = job(&asked, "q-001");
    assert_eq!(held.state, JobState::Running);
    assert_eq!(
        held.stop,
        Some(StopRequest {
            mode: StopMode::WriteNow,
            cause: StopCause::Cancel
        })
    );

    // Asking again changes nothing and is not an error.
    let again = go(&asked, Action::Cancel { id: id("q-001") });
    assert_eq!(again, asked);

    // The job ends stopped, with a recorded run, after the stop was carried out ...
    let stopping = go(&again, Action::StopApplied { id: id("q-001") });
    let finishing = go(&stopping, Action::ContainerEnded { id: id("q-001") });
    let stopped = go(
        &finishing,
        complete("q-001", Outcome::Stopped { run: id("run-2") }),
    );
    assert_eq!(job(&stopped, "q-001").state, JobState::Stopped);
    assert_eq!(job(&stopped, "q-001").run.as_deref(), Some("run-2"));

    // ... or cancelled, when nothing was recorded.
    let cancelled = go(&finishing, complete("q-001", Outcome::Cancelled));
    assert_eq!(job(&cancelled, "q-001").state, JobState::Cancelled);
}

#[test]
fn a_user_stop_needs_a_running_job() {
    let queue = queued(2);
    let err = apply(
        &queue,
        stop_request("q-001", StopMode::WriteNow, StopCause::User),
    )
    .unwrap_err();
    assert_eq!(err, bad_state("q-001", "queued", "request_stop"));

    let busy = running(&queue, "q-001");
    let asked = go(
        &busy,
        stop_request("q-001", StopMode::WriteNow, StopCause::User),
    );
    assert_eq!(
        job(&asked, "q-001").stop,
        Some(StopRequest {
            mode: StopMode::WriteNow,
            cause: StopCause::User
        })
    );

    // A divergence stop does not write, and replaces a stop that would have.
    let diverged = go(
        &asked,
        stop_request("q-001", StopMode::NoWrite, StopCause::Diverged),
    );
    assert_eq!(
        job(&diverged, "q-001").stop,
        Some(StopRequest {
            mode: StopMode::NoWrite,
            cause: StopCause::Diverged
        })
    );

    // A later request to write does not undo it.
    let kept = go(&diverged, Action::Cancel { id: id("q-001") });
    assert_eq!(job(&kept, "q-001").stop, job(&diverged, "q-001").stop);

    // A queued job still cannot be stopped.
    let err = apply(
        &busy,
        stop_request("q-002", StopMode::NoWrite, StopCause::Disk),
    )
    .unwrap_err();
    assert_eq!(err, bad_state("q-002", "queued", "request_stop"));
}

#[test]
fn a_stop_applied_moves_the_job_to_stopping() {
    let busy = running(&queued(1), "q-001");

    // Nothing was asked for yet.
    let err = apply(&busy, Action::StopApplied { id: id("q-001") }).unwrap_err();
    assert_eq!(err, bad_state("q-001", "running", "stop_applied"));

    let asked = go(
        &busy,
        stop_request("q-001", StopMode::WriteNow, StopCause::User),
    );
    let stopping = go(&asked, Action::StopApplied { id: id("q-001") });
    let held = job(&stopping, "q-001");
    assert_eq!(held.state, JobState::Stopping);
    assert!(held.stop.is_some());

    // A stopping job ends through the container.
    let finishing = go(&stopping, Action::ContainerEnded { id: id("q-001") });
    assert_eq!(job(&finishing, "q-001").state, JobState::Finishing);
}

#[test]
fn retry_goes_back_to_queued_twice_then_fails() {
    let queue = go(&queued(1), begin("q-001"));
    let first = go(&queue, Action::Retry { id: id("q-001") });
    let held = job(&first, "q-001");
    assert_eq!((held.state, held.attempts), (JobState::Queued, 1));
    assert!(held.run_id.is_none());

    let queue = go(&first, begin("q-001"));
    let second = go(&queue, Action::Retry { id: id("q-001") });
    assert_eq!(job(&second, "q-001").attempts, 2);

    let queue = go(&second, begin("q-001"));
    let err = apply(&queue, Action::Retry { id: id("q-001") }).unwrap_err();
    assert_eq!(err, bad_state("q-001", "starting", "retry"));

    let failed = go(
        &queue,
        complete(
            "q-001",
            Outcome::Failed {
                code: id("CFD-INFRA-003"),
            },
        ),
    );
    let held = job(&failed, "q-001");
    assert_eq!(held.state, JobState::Failed);
    assert_eq!(held.code.as_deref(), Some("CFD-INFRA-003"));

    // Only a starting job can be retried.
    let err = apply(
        &running(&queued(1), "q-001"),
        Action::Retry { id: id("q-001") },
    )
    .unwrap_err();
    assert_eq!(err, bad_state("q-001", "running", "retry"));
}

#[test]
fn requeue_uses_no_try_and_a_cancel_asked_for_meanwhile_ends_the_job() {
    let starting = go(&queued(1), begin("q-001"));
    let back = go(&starting, Action::Requeue { id: id("q-001") });
    let held = job(&back, "q-001");
    assert_eq!((held.state, held.attempts), (JobState::Queued, 0));
    assert!(held.run_id.is_none());

    let asked = go(&starting, Action::Cancel { id: id("q-001") });
    assert!(job(&asked, "q-001").stop.is_some());
    let ended = go(&asked, Action::Requeue { id: id("q-001") });
    let held = job(&ended, "q-001");
    assert_eq!(held.state, JobState::Cancelled);
    assert!(held.stop.is_none() && held.run_id.is_none());

    let err = apply(&queued(1), Action::Requeue { id: id("q-001") }).unwrap_err();
    assert_eq!(err, bad_state("q-001", "queued", "requeue"));
}

#[test]
fn unreachable_and_back() {
    let busy = running(&queued(1), "q-001");
    let asked = go(
        &busy,
        stop_request("q-001", StopMode::WriteNow, StopCause::User),
    );
    let lost = go(&asked, Action::Unreachable { id: id("q-001") });
    assert_eq!(job(&lost, "q-001").state, JobState::Unreachable);
    assert_eq!(job(&lost, "q-001").stop, job(&asked, "q-001").stop);

    let back = go(&lost, Action::Reachable { id: id("q-001") });
    assert_eq!(job(&back, "q-001").state, JobState::Running);
    assert_eq!(job(&back, "q-001").stop, job(&asked, "q-001").stop);

    // A job that is not unreachable is already reachable.
    let err = apply(&back, Action::Reachable { id: id("q-001") }).unwrap_err();
    assert_eq!(err, bad_state("q-001", "running", "reachable"));

    // A lost job can be given up on.
    let given_up = go(
        &lost,
        complete(
            "q-001",
            Outcome::Interrupted {
                code: id("CFD-RUN-010"),
            },
        ),
    );
    assert_eq!(job(&given_up, "q-001").state, JobState::Interrupted);
}

#[test]
fn clearing_drops_terminal_jobs_only() {
    let queue = queued(4);
    let queue = go(&queue, Action::Cancel { id: id("q-004") });
    let queue = running(&queue, "q-001");
    let cleared = go(&queue, Action::ClearFinished);
    assert_eq!(order(&cleared), ["q-001", "q-002", "q-003"]);
    assert_eq!(cleared.next_number, 5);
}

#[test]
fn moves_from_the_wrong_state_are_refused() {
    let queue = queued(1);
    for (action, name) in [
        (Action::Started { id: id("q-001") }, "started"),
        (
            Action::ContainerEnded { id: id("q-001") },
            "container_ended",
        ),
        (Action::Unreachable { id: id("q-001") }, "unreachable"),
        (Action::Retry { id: id("q-001") }, "retry"),
        (Action::Requeue { id: id("q-001") }, "requeue"),
        (
            complete("q-001", Outcome::Finished { run: id("run-1") }),
            "complete",
        ),
        (
            complete(
                "q-001",
                Outcome::Interrupted {
                    code: id("CFD-RUN-010"),
                },
            ),
            "complete",
        ),
    ] {
        assert_eq!(
            apply(&queue, action).unwrap_err(),
            bad_state("q-001", "queued", name)
        );
    }

    // Begin only takes a queued job.
    let busy = running(&queue, "q-001");
    assert_eq!(
        apply(&busy, begin("q-001")).unwrap_err(),
        bad_state("q-001", "running", "begin")
    );
    assert_eq!(
        apply(&busy, Action::Started { id: id("q-009") }).unwrap_err(),
        QueueError::Unknown { job: id("q-009") }
    );

    // A job that is not first in line cannot jump the queue.
    let two = queued(2);
    assert_eq!(
        apply(&two, begin("q-002")).unwrap_err(),
        QueueError::NotQueued { job: id("q-002") }
    );

    // A run id that is not 1 to 128 letters, digits or hyphens is refused.
    let long = "x".repeat(129);
    for bad in ["", "has space", "a/b", long.as_str()] {
        let err = apply(
            &two,
            Action::Begin {
                id: id("q-001"),
                run_id: bad.to_owned(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, QueueError::Damaged { .. }), "{bad}");
    }

    // Too many jobs at once.
    let err = apply(
        &Queue::new(),
        Action::Enqueue {
            jobs: (0..=MAX_JOBS).map(|_| solve()).collect(),
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        QueueError::TooMany {
            count: MAX_JOBS + 1,
            max: MAX_JOBS
        }
    );
}

#[test]
fn an_edited_down_counter_never_repeats_an_id() {
    let mut queue = queued(2);
    queue.next_number = 1;
    let queue = go(
        &queue,
        Action::Enqueue {
            jobs: vec![solve()],
        },
    );
    assert_eq!(order(&queue), ["q-001", "q-002", "q-003"]);
    assert_eq!(queue.next_number, 4);
}

fn raw(text: &str, state: JobState) -> Job {
    Job {
        id: id(text),
        scenario: None,
        kind: JobKind::Solve,
        origin: None,
        state,
        attempts: 0,
        run_id: None,
        run: None,
        code: None,
        stop: None,
        progress: None,
    }
}

fn active_raw(text: &str) -> Job {
    Job {
        run_id: Some(id("cfdcraft-demo-1")),
        ..raw(text, JobState::Running)
    }
}

fn file(jobs: Vec<Job>) -> Queue {
    Queue {
        schema_version: 1,
        next_number: 100,
        jobs,
    }
}

#[test]
fn check_queue_refuses_a_hostile_file() {
    // Duplicate ids.
    let err = check_queue(&file(vec![
        raw("q-001", JobState::Queued),
        raw("q-001", JobState::Queued),
    ]))
    .unwrap_err();
    assert!(matches!(err, QueueError::Damaged { .. }));

    // Two active jobs.
    let err = check_queue(&file(vec![active_raw("q-001"), active_raw("q-002")])).unwrap_err();
    assert_eq!(err, QueueError::Active { job: id("q-002") });

    // 1001 jobs.
    let many: Vec<Job> = (1..=MAX_JOBS + 1)
        .map(|n| raw(&format!("q-{n:03}"), JobState::Queued))
        .collect();
    let err = check_queue(&file(many)).unwrap_err();
    assert_eq!(
        err,
        QueueError::TooMany {
            count: MAX_JOBS + 1,
            max: MAX_JOBS
        }
    );

    // An id of another shape.
    for bad in ["Q 1", "q-", "q-x", "job-1", "q-12345678901", "q-1; rm"] {
        let err = check_queue(&file(vec![raw(bad, JobState::Queued)])).unwrap_err();
        assert_eq!(err, QueueError::BadId { job: id(bad) });
    }

    // A scenario that is not a plain name.
    let job = Job {
        scenario: Some(id("Bad Name")),
        ..raw("q-001", JobState::Queued)
    };
    assert_eq!(
        check_queue(&file(vec![job])).unwrap_err(),
        QueueError::BadScenario { job: id("q-001") }
    );

    // A running job with no run id, a finished one with no run, a queued one with a run id.
    let no_run_id = raw("q-001", JobState::Running);
    let no_run = raw("q-001", JobState::Finished);
    let no_code = raw("q-001", JobState::Failed);
    let held = Job {
        run_id: Some(id("cfdcraft-demo-1")),
        ..raw("q-001", JobState::Queued)
    };
    for job in [no_run_id, no_run, no_code, held] {
        let err = check_queue(&file(vec![job])).unwrap_err();
        assert!(matches!(err, QueueError::Damaged { .. }), "{err}");
    }

    // Members that would reach a command line or a screen.
    let weird = [
        Job {
            run_id: Some(id("x; rm -rf /")),
            ..raw("q-001", JobState::Running)
        },
        Job {
            run: Some(id("Run 1")),
            ..raw("q-001", JobState::Finished)
        },
        Job {
            code: Some(id("bad code")),
            ..raw("q-001", JobState::Failed)
        },
        Job {
            kind: JobKind::Continue { run: id("../x") },
            ..raw("q-001", JobState::Queued)
        },
        Job {
            progress: Some(Progress {
                summary: Some(id("two\nlines")),
                ..Progress::default()
            }),
            ..active_raw("q-001")
        },
        Job {
            progress: Some(Progress {
                time: Some(f64::NAN),
                ..Progress::default()
            }),
            ..active_raw("q-001")
        },
    ];
    for job in weird {
        let err = check_queue(&file(vec![job])).unwrap_err();
        assert!(matches!(err, QueueError::Damaged { .. }), "{err}");
    }

    // A sound file passes.
    let sound = file(vec![
        Job {
            run: Some(id("run-1")),
            ..raw("q-001", JobState::Finished)
        },
        active_raw("q-002"),
        raw("q-003", JobState::Queued),
    ]);
    assert!(check_queue(&sound).is_ok());
}
