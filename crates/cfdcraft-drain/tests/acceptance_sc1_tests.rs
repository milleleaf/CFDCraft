//! A sweep becomes a queue, jobs are reordered and cancelled, the program can be closed while a
//! job runs, and the document holds each finished run once. The queue is advanced by the real
//! drain over the in-memory host, from a saved document, through the real operations.

use std::fs;

use cfdcraft_drain::TickOutcome;
use cfdcraft_drain::reattach::on_startup;
use cfdcraft_ops::queue_ops::{queue_cancel, queue_reorder};
use cfdcraft_queue::JobState;
use cfdcraft_remote::ContainerState;
use cfdcraft_text::{Line, Text};

mod support;
use support::*;

#[path = "support/acceptance.rs"]
mod acceptance;
use acceptance::*;

/// A sweep of three lid speeds, none of them the speed of the study itself.
const SPEEDS: [f64; 3] = [0.2, 0.3, 0.4];

#[test]
fn a_sweep_appears_as_a_queue_and_runs_to_the_end() {
    let w = sweep_world("sc1-sweep", &SPEEDS);
    let ids = queue_sweep(&w);
    assert_eq!(ids, ["q-001", "q-002", "q-003"]);

    // The jobs are the points of the sweep, in point order.
    let queue = w.queue();
    let scenarios: Vec<Option<&str>> = queue.jobs.iter().map(|j| j.scenario.as_deref()).collect();
    assert_eq!(
        scenarios,
        [Some("lids-001"), Some("lids-002"), Some("lids-003")]
    );
    let points: Vec<u32> = queue
        .jobs
        .iter()
        .map(|j| {
            let origin = j.origin.as_ref().expect("a point keeps its origin");
            assert_eq!(origin.sweep, SWEEP);
            origin.index
        })
        .collect();
    assert_eq!(points, [0, 1, 2]);
    assert_eq!(states(&w), ["queued", "queued", "queued"]);

    // They run one at a time, and each ends finished.
    let mut drain = w.drain();
    for id in &ids {
        assert_eq!(finish_job(&w, &mut drain, id), JobState::Finished, "{id}");
    }
    assert_eq!(states(&w), ["finished", "finished", "finished"]);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Idle);

    // The document holds each run once, tagged with its point.
    let runs = w.runs();
    assert_eq!(runs.len(), 3);
    for (n, run) in runs.iter().enumerate() {
        let point = format!("lids-{:03}", n + 1);
        assert_eq!(run.id, format!("run-{}", n + 1));
        assert_eq!(run.scenario.as_deref(), Some(point.as_str()));
        assert!(run.result.is_some(), "{} holds a result", run.id);
        assert_eq!(w.job(&ids[n]).run.as_deref(), Some(run.id.as_str()));
    }
    for _ in 0..3 {
        drain.tick().unwrap();
    }
    assert_eq!(w.runs().len(), 3, "later ticks record nothing more");
}

#[test]
fn reorder_and_cancel_change_what_runs() {
    let w = sweep_world("sc1-reorder", &SPEEDS);
    let ids = queue_sweep(&w);

    // Point 3 goes first and point 2 is cancelled before it starts.
    passed(&queue_reorder(&w.doc, &ids[2], 0).report);
    passed(&queue_cancel(&w.doc, &ids[1]).report);
    assert_eq!(w.state(&ids[1]), JobState::Cancelled);
    let first_waiting = w
        .queue()
        .jobs
        .into_iter()
        .find(|job| job.state == JobState::Queued)
        .expect("a job is waiting");
    assert_eq!(first_waiting.id, ids[2]);

    let mut drain = w.drain();
    assert_eq!(finish_job(&w, &mut drain, &ids[2]), JobState::Finished);
    assert_eq!(finish_job(&w, &mut drain, &ids[0]), JobState::Finished);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Idle);

    let runs = w.runs();
    let recorded: Vec<Option<&str>> = runs.iter().map(|run| run.scenario.as_deref()).collect();
    assert_eq!(recorded, [Some("lids-003"), Some("lids-001")]);

    // The cancelled point never reached the host.
    let cancelled = w.job(&ids[1]);
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert!(cancelled.run_id.is_none());
    assert!(cancelled.run.is_none());
    assert_eq!(start_calls(&w).len(), 2, "{:?}", w.shared.calls());
}

#[test]
fn closing_the_program_leaves_the_job_running_and_reopening_reattaches() {
    let w = sweep_world("sc1-reattach", &SPEEDS);
    let ids = queue_sweep(&w);
    let id = ids[0].clone();
    let mut drain = w.drain();
    let run_id = start_job(&w, &mut drain, &id);
    w.shared.grow_log(&run_id, 1, 30);
    drain.tick().unwrap();
    let before = mirror_len(&w, &id, "log.simpleFoam");
    assert!(before > 0, "the log so far is mirrored");

    // The program is closed. The solver goes on on the host, and keeps the lock.
    drop(drain);
    assert_eq!(w.shared.container(&run_id), ContainerState::Running);
    assert_eq!(w.shared.holder().expect("the lock is held").run_id, run_id);
    w.shared.grow_log(&run_id, 31, 60);

    // The program is opened again: the job is picked up, not started again.
    let seen = w.shared.calls().len();
    let mut drain = w.drain();
    on_startup(&mut drain).unwrap();
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Running);
    let summary = job.progress.and_then(|p| p.summary).unwrap_or_default();
    assert_eq!(
        summary,
        Text::english().render(&Line::JobReattached { job: id.clone() })
    );
    drain.tick().unwrap();
    let after = calls_since(&w, seen);
    assert!(
        after.iter().all(|call| !call.starts_with("start ")),
        "nothing was started again: {after:?}"
    );
    assert_eq!(first_tail_offset(&after, &run_id, "log.simpleFoam"), before);
    assert!(mirror_len(&w, &id, "log.simpleFoam") > before);

    // The job finishes and is recorded once.
    assert_eq!(finish_job(&w, &mut drain, &id), JobState::Finished);
    assert_eq!(w.runs().len(), 1);
    assert_eq!(w.job(&id).run.as_deref(), Some("run-1"));
    assert_eq!(
        fs::read(w.live(&id).join("log.simpleFoam")).ok(),
        w.shared.file(&run_id, "log.simpleFoam"),
        "the mirror equals the log on the host, byte for byte"
    );
    assert!(w.shared.holder().is_none(), "the lock was given back");
}

#[test]
fn the_queue_listing_shows_queued_running_and_finished_together() {
    let w = sweep_world("sc1-listing", &SPEEDS);
    let ids = queue_sweep(&w);
    let mut drain = w.drain();
    assert_eq!(finish_job(&w, &mut drain, &ids[0]), JobState::Finished);
    let run_id = start_job(&w, &mut drain, &ids[1]);
    w.shared.grow_log(&run_id, 1, 300);
    w.shared.grow_probes(&run_id, 1, 300);
    drain.tick().unwrap();

    let shown = listed(&w);
    let words: Vec<&str> = shown.jobs.iter().map(|job| job.state.as_str()).collect();
    assert_eq!(words, ["finished", "running", "queued"]);
    assert_eq!(shown.active.as_deref(), Some(ids[1].as_str()));
    assert_eq!(shown.jobs[0].run.as_deref(), Some("run-1"));
    let line = shown.jobs[1]
        .progress_line
        .as_deref()
        .expect("a running job shows a progress line");
    assert!(line.starts_with("settled "), "{line}");
    assert!(shown.jobs[2].progress_line.is_none());
    assert!(!shown.drain, "no runner is advancing this queue");
}

#[test]
fn a_cancel_of_a_running_job_ends_it_stopped_and_starts_the_next() {
    let w = sweep_world("sc1-cancel-running", &SPEEDS);
    let ids = queue_sweep(&w);
    let mut drain = w.drain();
    let run_id = start_job(&w, &mut drain, &ids[0]);
    w.shared.inner.borrow_mut().solver_obeys_stop(true);
    put_partial(&w, &run_id, 1200, 16384);
    passed(&queue_cancel(&w.doc, &ids[0]).report);

    assert_eq!(settle(&w, &mut drain, &ids[0], 40), JobState::Stopped);
    let runs = w.runs();
    assert_eq!(runs.len(), 1, "the stopped run is recorded");
    assert!(runs[0].result.is_none(), "a stopped run holds no result");
    assert_eq!(
        runs[0].checkpoint.as_ref().map(|c| c.time.as_str()),
        Some("1200")
    );
    assert_eq!(w.job(&ids[0]).run.as_deref(), Some("run-1"));
    assert!(w.shared.holder().is_none(), "the host was given back");

    // The next waiting job starts.
    start_job(&w, &mut drain, &ids[1]);
    assert_eq!(w.state(&ids[1]), JobState::Running);
    assert_eq!(w.state(&ids[2]), JobState::Queued);
}

#[test]
fn nothing_runs_in_parallel_on_the_host() {
    let w = sweep_world("sc1-serial", &SPEEDS);
    let ids = queue_sweep(&w);
    let mut drain = w.drain();
    for id in &ids {
        assert_eq!(finish_job(&w, &mut drain, id), JobState::Finished, "{id}");
    }

    // Walk the host's own record: a start never comes while an earlier job's lock is held.
    let mut held: Option<String> = None;
    let mut started = 0;
    for call in w.shared.calls() {
        if let Some(run_id) = call.strip_prefix("start ") {
            assert!(
                held.is_none(),
                "{run_id} was started while {held:?} held the host"
            );
            held = Some(run_id.to_owned());
            started += 1;
        } else if let Some(run_id) = call.strip_prefix("release ")
            && held.as_deref() == Some(run_id)
        {
            held = None;
        }
    }
    assert_eq!(started, 3);
    assert!(held.is_none(), "the last job gave the host back");
}
