//! A drain that is killed at any step is replaced by a new one that finishes the job, and a new
//! drain never releases, stops or removes anything that is not plainly its own.

use std::fs;

use cfdcraft_drain::reattach::{HOST_MISMATCH, on_startup};
use cfdcraft_drain::{Drain, TickOutcome};
use cfdcraft_queue::{Action, Job, JobState, StopCause, StopMode};
use cfdcraft_remote::{ContainerState, Holder};
use cfdcraft_text::{Line, Text};

mod support;
use support::*;

const NO_MUTATION: [&str; 3] = ["stop_container", "remove_container", "release"];

/// A job added and started: the drain, the id and the run name.
fn started(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    let run_id = w.run_id(&id);
    (w, drain, id, run_id)
}

/// Writes the queue file the way a process that died one step earlier would have left it.
fn put_state(w: &World, state: JobState) {
    let mut queue = w.queue();
    queue.jobs[0].state = state;
    fs::write(
        cfdcraft_doc::store_dir(&w.doc).join("queue.json"),
        queue.to_json(),
    )
    .unwrap();
}

fn mirror_len(w: &World, id: &str, rel: &str) -> u64 {
    fs::metadata(w.live(id).join(rel)).map_or(0, |m| m.len())
}

/// The calls the host has seen after the first `seen`.
fn calls_since(w: &World, seen: usize) -> Vec<String> {
    w.shared.calls().split_off(seen)
}

/// Panics when any of the calls changed the lock or the container.
fn assert_untouched(calls: &[String]) {
    for call in calls {
        for word in NO_MUTATION {
            assert!(!call.starts_with(word), "the host was changed: {call}");
        }
    }
}

fn summary(job: &Job) -> String {
    job.progress
        .as_ref()
        .and_then(|p| p.summary.clone())
        .unwrap_or_default()
}

fn reattached(id: &str) -> String {
    Text::english().render(&Line::JobReattached { job: id.to_owned() })
}

fn someone_else() -> Holder {
    Holder::new("someone-else-1", "someone-else-1", 0, 3600, "other").unwrap()
}

/// Ticks until the job is in a state that ends, at most `limit` times.
fn tick_to_the_end(drain: &mut Drain<Shared>, w: &World, id: &str, limit: usize) {
    for _ in 0..limit {
        if w.state(id).is_terminal() {
            return;
        }
        drain.tick().unwrap();
    }
}

#[test]
fn a_restarted_drain_resumes_polling_from_the_mirror_length() {
    let (w, mut drain, id, run_id) = started("reattach-resume");
    for (from, to) in [(1, 3), (4, 6), (7, 9)] {
        w.shared.grow_log(&run_id, from, to);
        drain.tick().unwrap();
    }
    let before = mirror_len(&w, &id, "log.simpleFoam");
    assert!(before > 0, "the mirror holds the log so far");

    // The kill: the process is gone, the box and the files stay.
    drop(drain);
    let seen = w.shared.calls().len();
    w.shared.grow_log(&run_id, 10, 12);
    let mut drain = w.drain();
    drain.tick().unwrap();
    w.shared.grow_log(&run_id, 13, 15);
    drain.tick().unwrap();

    // The solver goes on and ends; its log keeps what was written before.
    let written = w.shared.file(&run_id, "log.simpleFoam").unwrap();
    w.play(&run_id);
    let ending = w.shared.file(&run_id, "log.simpleFoam").unwrap();
    let mut whole = written;
    whole.extend_from_slice(&ending);
    w.shared.replace(
        &run_id,
        "log.simpleFoam",
        std::str::from_utf8(&whole).unwrap(),
    );
    tick_to_the_end(&mut drain, &w, &id, 6);

    let job = w.job(&id);
    assert_eq!(job.state, JobState::Finished);
    assert_eq!(job.run.as_deref(), Some("run-1"));
    assert_eq!(w.runs().len(), 1, "the run is recorded once");
    let box_log = w.shared.file(&run_id, "log.simpleFoam").unwrap();
    assert_eq!(
        fs::read(w.live(&id).join("log.simpleFoam")).unwrap(),
        box_log,
        "the mirror equals the log on the box, byte for byte"
    );

    let after = calls_since(&w, seen);
    assert!(
        after.iter().all(|call| !call.starts_with("start ")),
        "nothing was started again: {after:?}"
    );
    let prefix = format!("tail {run_id} log.simpleFoam ");
    let first: u64 = after
        .iter()
        .find_map(|call| call.strip_prefix(&prefix))
        .expect("the log was fetched again")
        .parse()
        .unwrap();
    assert_eq!(first, before, "fetching resumed at the mirror length");
}

#[test]
fn lock_ours_container_running_resumes() {
    // Killed after the host started the container, before the start was written down.
    let (w, drain, id, run_id) = started("reattach-run-start");
    drop(drain);
    put_state(&w, JobState::Starting);
    let seen = w.shared.calls().len();
    let mut drain = w.drain();
    drain.tick().unwrap();
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Running);
    assert_eq!(summary(&job), reattached(&id));
    assert_eq!(w.shared.holder().unwrap().run_id, run_id);
    assert_eq!(w.shared.container(&run_id), ContainerState::Running);
    assert_untouched(&calls_since(&w, seen));

    // Killed while running.
    let (w, drain, id, run_id) = started("reattach-run-run");
    drop(drain);
    let seen = w.shared.calls().len();
    let mut drain = w.drain();
    on_startup(&mut drain).unwrap();
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Running);
    assert_eq!(summary(&job), reattached(&id));
    assert_eq!(w.shared.holder().unwrap().run_id, run_id);
    assert_untouched(&calls_since(&w, seen));
}

#[test]
fn lock_ours_container_exited_goes_to_finishing() {
    for from in ["starting", "running", "stopping"] {
        let (w, drain, id, run_id) = started(&format!("reattach-exit-{from}"));
        match from {
            "starting" => put_state(&w, JobState::Starting),
            "stopping" => {
                let stop = Action::RequestStop {
                    id: id.clone(),
                    mode: StopMode::WriteNow,
                    cause: StopCause::User,
                };
                drain.persist(stop).unwrap();
                drain
                    .persist(Action::StopApplied { id: id.clone() })
                    .unwrap();
                assert_eq!(w.state(&id), JobState::Stopping);
            }
            _ => {}
        }
        drop(drain);
        w.play(&run_id);

        let mut drain = w.drain();
        if from == "starting" {
            drain.tick().unwrap();
        } else {
            on_startup(&mut drain).unwrap();
        }
        assert_eq!(w.state(&id), JobState::Finishing, "from {from}");
        if from != "stopping" {
            tick_to_the_end(&mut drain, &w, &id, 4);
            assert_eq!(w.state(&id), JobState::Finished, "from {from}");
            assert_eq!(w.runs().len(), 1, "from {from}");
            assert!(w.shared.holder().is_none(), "from {from}: the lock is back");
        }
    }
}

#[test]
fn lock_ours_no_container_after_start_is_finalised_from_what_the_box_holds() {
    let (w, drain, id, run_id) = started("reattach-vanished");
    drop(drain);
    w.shared.inner.borrow_mut().drop_container(&run_id);

    let mut drain = w.drain();
    tick_to_the_end(&mut drain, &w, &id, 4);
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some("CFD-SOLVER-099"));
    assert!(w.runs().is_empty());
    assert!(w.shared.holder().is_none(), "the lock was released");
}

#[test]
fn starting_with_our_lock_and_no_container_is_requeued_twice_then_failed() {
    let w = world("reattach-lock-only");
    let id = w.add();
    w.shared.inner.borrow_mut().start_leaves_lock_only(true);
    let mut drain = w.drain();

    for attempts in 0..2 {
        // The start dies after taking the lock.
        drain.tick().unwrap();
        let job = w.job(&id);
        assert_eq!(job.state, JobState::Starting);
        assert_eq!(job.attempts, attempts);
        let run_id = w.run_id(&id);
        assert_eq!(w.shared.holder().unwrap().run_id, run_id);

        // The next tick finds that out and gives the lock back by name.
        let seen = w.shared.calls().len();
        drain.tick().unwrap();
        let job = w.job(&id);
        assert_eq!(job.state, JobState::Queued);
        assert_eq!(job.attempts, attempts + 1);
        assert!(job.run_id.is_none());
        assert!(w.shared.holder().is_none());
        let after = calls_since(&w, seen);
        assert!(
            after.contains(&format!("release {run_id}")),
            "the checked release was made: {after:?}"
        );
    }

    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Starting);
    drain.tick().unwrap();
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some("CFD-INFRA-007"));
    assert!(w.shared.holder().is_none(), "the lock is not left behind");
}

#[test]
fn starting_with_someone_elses_lock_is_requeued_without_releasing_it() {
    let (w, drain, id, run_id) = started("reattach-foreign-start");
    drop(drain);
    put_state(&w, JobState::Starting);
    {
        let mut host = w.shared.inner.borrow_mut();
        host.drop_container(&run_id);
        host.set_holder(Some(someone_else()));
    }
    let seen = w.shared.calls().len();

    let mut drain = w.drain();
    drain.tick().unwrap();
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Queued);
    assert_eq!(job.attempts, 1);
    assert_eq!(w.shared.holder().unwrap().run_id, "someone-else-1");
    assert_untouched(&calls_since(&w, seen));
}

/// A running job whose host no longer matches: the job is interrupted and the host is as it was.
fn left_alone(tag: &str, change: impl FnOnce(&World, &str)) -> (World, String, String, usize) {
    let (w, drain, id, run_id) = started(tag);
    drop(drain);
    change(&w, &run_id);
    let seen = w.shared.calls().len();
    let mut drain = w.drain();
    drain.tick().unwrap();
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Interrupted);
    assert_eq!(job.code.as_deref(), Some(HOST_MISMATCH));
    assert_untouched(&calls_since(&w, seen));
    (w, id, run_id, seen)
}

#[test]
fn no_lock_and_our_container_running_is_inconsistent_and_left_alone() {
    let (w, _, run_id, _) = left_alone("reattach-nolock", |w, _| {
        w.shared.inner.borrow_mut().set_holder(None);
    });
    assert!(w.shared.holder().is_none());
    assert_eq!(w.shared.container(&run_id), ContainerState::Running);
}

#[test]
fn someone_elses_lock_and_our_container_running_is_inconsistent_and_left_alone() {
    let (w, _, run_id, _) = left_alone("reattach-foreign-run", |w, _| {
        w.shared.inner.borrow_mut().set_holder(Some(someone_else()));
    });
    assert_eq!(w.shared.holder().unwrap().run_id, "someone-else-1");
    assert_eq!(w.shared.container(&run_id), ContainerState::Running);
}

#[test]
fn no_lock_and_no_container_while_running_is_interrupted() {
    let (w, id, _, _) = left_alone("reattach-nothing", |w, run_id| {
        let mut host = w.shared.inner.borrow_mut();
        host.set_holder(None);
        host.drop_container(run_id);
    });
    assert!(w.shared.holder().is_none());
    assert!(summary(&w.job(&id)).contains("interrupted"));
}

#[test]
fn a_restart_with_nothing_active_does_nothing() {
    let w = world("reattach-idle");
    let id = w.add();
    let mut drain = w.drain();
    drain.persist(Action::Cancel { id: id.clone() }).unwrap();
    assert_eq!(w.state(&id), JobState::Cancelled);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Idle);
    assert!(w.shared.calls().is_empty(), "{:?}", w.shared.calls());
}
