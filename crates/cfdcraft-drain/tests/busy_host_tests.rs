//! A job that meets a busy compute host waits its turn: it is not failed, no try is used up, and
//! the host is not asked again on every tick.

use cfdcraft_queue::{Action, JobState};
use cfdcraft_remote::Holder;

mod support;
use support::*;

fn someone_else() -> Holder {
    Holder::new("someone-else-1", "someone-else-1", 0, 3600, "other").unwrap()
}

fn starts(w: &World) -> usize {
    w.shared
        .calls()
        .iter()
        .filter(|c| c.starts_with("start "))
        .count()
}

#[test]
fn a_busy_host_keeps_the_job_waiting_and_it_starts_when_the_host_is_free() {
    let w = world("busy-host");
    let id = w.add();
    w.shared.inner.borrow_mut().set_holder(Some(someone_else()));
    let mut drain = w.drain();

    // Far more ticks than the two tries a failing start gets.
    for _ in 0..40 {
        drain.tick().unwrap();
        assert_eq!(w.state(&id), JobState::Queued);
    }
    let job = w.job(&id);
    assert_eq!(job.attempts, 0, "no try is used up");
    assert!(job.run_id.is_none());
    let said = job
        .progress
        .and_then(|p| p.summary)
        .expect("the job says it is waiting");
    assert!(said.contains("waiting"), "{said}");
    assert!(
        starts(&w) <= 11,
        "the host is asked every few ticks, not every tick: {}",
        starts(&w)
    );

    // The host frees up; the job starts and runs.
    w.shared.inner.borrow_mut().set_holder(None);
    for _ in 0..8 {
        drain.tick().unwrap();
    }
    assert_eq!(w.state(&id), JobState::Running);
    assert_eq!(w.job(&id).attempts, 0);
}

#[test]
fn a_cancel_asked_for_while_the_start_meets_a_busy_host_ends_the_job() {
    let w = world("busy-cancel");
    let id = w.add();
    w.shared.inner.borrow_mut().set_holder(Some(someone_else()));
    let mut drain = w.drain();
    // The cancel arrives while the start is under way: before the host answers.
    let file = w.file();
    let cancel_id = id.clone();
    w.shared.on_before_start(move |_| {
        file.update(|_| {
            Ok(vec![Action::Cancel {
                id: cancel_id.clone(),
            }])
        })
        .unwrap();
    });
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Cancelled);
}
