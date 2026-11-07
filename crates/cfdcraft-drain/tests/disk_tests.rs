//! A job is not started below the disk floor of the compute host, and a running job is asked to
//! write now and stop before its next write would cross it.

use cfdcraft_drain::disk::{
    self, BELOW_FLOOR_CODE, DiskRefusal, KB_PER_GB, STOPPED_CODE, below_floor, needs_write_now,
};
use cfdcraft_drain::finalize::JobReport;
use cfdcraft_drain::{Drain, TickOutcome};
use cfdcraft_queue::{Action, JobState, StopCause, StopMode, StopRequest};
use cfdcraft_remote::HostError;

mod support;
use support::*;

const FLOOR_GB: u64 = 20;
const FLOOR_KB: u64 = FLOOR_GB * KB_PER_GB;
const MIB_KB: u64 = 1024;

const WRITE_NOW: StopRequest = StopRequest {
    mode: StopMode::WriteNow,
    cause: StopCause::Disk,
};

fn set_free(w: &World, kb: u64) {
    w.shared.inner.borrow_mut().set_free_kb(kb);
}

/// A world with one job that has been started on a host with plenty of room.
fn started(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    assert_eq!(w.state(&id), JobState::Running);
    let run_id = w.run_id(&id);
    (w, drain, id, run_id)
}

/// Writes a time folder of `mib` mebibytes into the run: the next write is taken to be that big.
fn write_time_folder(w: &World, run_id: &str, mib: usize) {
    w.shared
        .append(run_id, "100/U", &"x".repeat(mib * 1024 * 1024));
}

fn started_calls(w: &World) -> Vec<String> {
    w.shared
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("start "))
        .collect()
}

#[test]
fn a_job_is_not_started_below_the_floor() {
    let w = world("disk-below");
    let id = w.add();
    set_free(&w, 10 * KB_PER_GB);
    let mut drain = w.drain();
    drain.tick().unwrap();

    let job = w.job(&id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some(BELOW_FLOOR_CODE));
    assert_eq!(BELOW_FLOOR_CODE, "CFD-DISK-003");
    assert!(started_calls(&w).is_empty(), "{:?}", w.shared.calls());
    assert!(w.shared.holder().is_none(), "no lock was taken");
    let text = std::fs::read_to_string(w.live(&id).join("report.json")).unwrap();
    let report: JobReport = serde_json::from_str(&text).unwrap();
    assert_eq!(
        report.lines,
        ["The compute host has 10 GB free, below its floor of 20 GB, so the run was not started."]
    );
}

#[test]
fn a_job_is_started_at_or_above_the_floor() {
    let w = world("disk-at-floor");
    let id = w.add();
    set_free(&w, FLOOR_KB);
    let mut drain = w.drain();
    assert!(disk::check_before_start(&mut drain).is_ok());
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Running);
}

#[test]
fn a_running_job_is_asked_to_write_now_before_the_floor_is_crossed() {
    let (w, mut drain, id, run_id) = started("disk-running");
    set_free(&w, 30 * KB_PER_GB);
    write_time_folder(&w, &run_id, 2);
    drain.tick().unwrap();
    assert_eq!(w.job(&id).stop, None, "30 GB is far above the floor");

    set_free(&w, FLOOR_KB + MIB_KB);
    drain.tick().unwrap();
    assert_eq!(w.job(&id).stop, Some(WRITE_NOW));
    assert!(drain.scratch().disk_stop);
}

#[test]
fn one_write_of_headroom_is_kept() {
    let gb = KB_PER_GB;
    assert!(needs_write_now(22 * gb, 20 * gb, 3 * gb));
    assert!(!needs_write_now(24 * gb, 20 * gb, 3 * gb));
    assert!(
        !needs_write_now(23 * gb, 20 * gb, 3 * gb),
        "exactly one write left"
    );
    assert!(needs_write_now(23 * gb - 1, 20 * gb, 3 * gb));
    assert!(
        !needs_write_now(u64::MAX, u64::MAX, 1),
        "the sum does not wrap"
    );

    // The same rule through the drain, with a write of 3 MiB.
    let (w, mut drain, id, run_id) = started("disk-headroom");
    write_time_folder(&w, &run_id, 3);
    set_free(&w, FLOOR_KB + 4 * MIB_KB);
    drain.tick().unwrap();
    assert_eq!(w.job(&id).stop, None, "room for the write");
    set_free(&w, FLOOR_KB + 2 * MIB_KB);
    drain.tick().unwrap();
    assert_eq!(w.job(&id).stop, Some(WRITE_NOW));
}

#[test]
fn a_parallel_run_is_sized_by_all_its_processors() {
    // The solver of a parallel run writes into one folder per process. One write takes the room of
    // all of them, so four processes of 2 MiB each need 8 MiB, not 2.
    let (w, mut drain, id, run_id) = started("disk-parallel");
    for n in 0..4 {
        w.shared.append(
            &run_id,
            &format!("processor{n}/100/U"),
            &"x".repeat(2 * 1024 * 1024),
        );
    }
    set_free(&w, FLOOR_KB + 12 * MIB_KB);
    drain.tick().unwrap();
    assert_eq!(w.job(&id).stop, None, "room for 8 MiB");
    set_free(&w, FLOOR_KB + 5 * MIB_KB);
    drain.tick().unwrap();
    assert_eq!(w.job(&id).stop, Some(WRITE_NOW));
}

#[test]
fn the_request_is_made_once_and_a_user_stop_is_kept() {
    let (w, mut drain, id, run_id) = started("disk-once");
    write_time_folder(&w, &run_id, 1);
    set_free(&w, FLOOR_KB);
    drain.tick().unwrap();
    assert_eq!(w.job(&id).stop, Some(WRITE_NOW));
    // the next tick applies the request (the job moves to stopping); after that nothing changes
    drain.tick().unwrap();
    let queue = w.queue();
    drain.tick().unwrap();
    drain.tick().unwrap();
    assert_eq!(w.queue(), queue, "later ticks change nothing");

    let (w, mut drain, id, run_id) = started("disk-user");
    let user = StopRequest {
        mode: StopMode::WriteNow,
        cause: StopCause::User,
    };
    drain
        .persist(Action::RequestStop {
            id: id.clone(),
            mode: user.mode,
            cause: user.cause,
        })
        .unwrap();
    write_time_folder(&w, &run_id, 1);
    set_free(&w, FLOOR_KB);
    drain.tick().unwrap();
    assert_eq!(w.job(&id).stop, Some(user), "the first request stays");
    assert!(!drain.scratch().disk_stop, "the stop was not for the disk");
}

#[test]
fn a_disk_stopped_run_is_recorded_as_stopped_not_failed() {
    // Carrying out the stop and recording the run belong to the stop module; here the request
    // and the code the job will carry.
    assert_eq!(STOPPED_CODE, "CFD-DISK-004");
    let (w, mut drain, id, run_id) = started("disk-stopped");
    write_time_folder(&w, &run_id, 1);
    set_free(&w, FLOOR_KB + 1);
    drain.tick().unwrap();
    let job = w.job(&id);
    assert_eq!(job.stop, Some(WRITE_NOW));
    assert_ne!(job.state, JobState::Failed);
}

#[test]
fn an_unreadable_df_is_not_a_pass() {
    let w = world("disk-unreadable");
    let mut drain = w.drain();
    w.shared.inner.borrow_mut().fail_next(1);
    assert_eq!(
        disk::check_before_start(&mut drain),
        Err(DiskRefusal::Host(HostError::Unreachable))
    );

    // Through a start: the first tick has no job, so the host's silence lands on the start.
    assert_eq!(drain.tick().unwrap(), TickOutcome::Idle);
    let id = w.add();
    w.shared.inner.borrow_mut().fail_next(1);
    drain.tick().unwrap();
    assert!(started_calls(&w).is_empty(), "{:?}", w.shared.calls());
    assert_ne!(w.state(&id), JobState::Running);
    assert_ne!(w.state(&id), JobState::Finished);
}

#[test]
fn kilobytes_and_gigabytes_are_not_confused() {
    assert_eq!(KB_PER_GB, 1_048_576);
    assert_eq!(FLOOR_KB, 20_971_520);
    assert!(below_floor(FLOOR_KB - 1, FLOOR_KB));
    assert!(!below_floor(FLOOR_KB, FLOOR_KB));

    let w = world("disk-units");
    let mut drain = w.drain();
    set_free(&w, 20_971_519);
    assert_eq!(
        disk::check_before_start(&mut drain),
        Err(DiskRefusal::BelowFloor {
            free_gb: 19,
            floor_gb: 20
        })
    );
    set_free(&w, 20_971_520);
    assert_eq!(disk::check_before_start(&mut drain), Ok(()));
}
