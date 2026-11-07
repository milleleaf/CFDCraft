//! One drain per document, an exit when idle, and a detached spawn.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cfdcraft_drain::DrainConfig;
use cfdcraft_drain::lifecycle::{DrainEnd, run_drain, run_drain_with, spawn_detached};
use cfdcraft_ops::queue_ops::{AddRequest, queue_add};
use cfdcraft_queue::JobState;

mod support;
use support::*;

fn flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[test]
fn a_second_drainer_for_the_same_document_does_not_start() {
    let w = world("life-second");
    let held = w.file().drain_lock().unwrap().expect("the lock was free");

    let end = run_drain(w.config(), w.shared.clone(), &flag());
    assert_eq!(end, DrainEnd::AlreadyRunning);
    assert!(w.shared.calls().is_empty());
    assert!(
        !cfdcraft_doc::store_dir(&w.doc).join("queue.json").exists(),
        "the queue was not even read into being"
    );
    drop(held);
    assert!(w.file().drain_lock().unwrap().is_some());
}

#[test]
fn the_drain_exits_after_the_idle_rounds() {
    let w = world("life-idle");
    let config = DrainConfig {
        idle_exit_after: 3,
        ..w.config()
    };
    let end = run_drain(config, w.shared.clone(), &flag());
    assert_eq!(end, DrainEnd::Idle);
    assert!(w.shared.calls().is_empty());
    assert!(
        w.file().drain_lock().unwrap().is_some(),
        "the lock was released"
    );
}

#[test]
fn a_job_added_during_the_exit_window_is_not_stranded() {
    let w = world("life-window");
    // The pretend solver finishes as soon as its container exists.
    let shared = w.shared.clone();
    let solver_dir = w.solver_dir();
    w.shared
        .on_after_start(move |run_id| shared.play_solver(&solver_dir, run_id));
    let config = DrainConfig {
        idle_exit_after: 2,
        ..w.config()
    };
    let doc = w.doc.clone();
    let mut added = false;
    let end = run_drain_with(config, w.shared.clone(), &flag(), || {
        if !added {
            added = true;
            passed(&queue_add(&doc, &AddRequest::default()).report);
        }
    });

    assert_eq!(end, DrainEnd::Idle);
    assert!(added, "the window was reached");
    let queue = w.queue();
    assert_eq!(queue.jobs.len(), 1);
    assert_eq!(queue.jobs[0].state, JobState::Finished);
    assert_eq!(w.runs().len(), 1);
}

#[test]
fn the_shutdown_flag_stops_between_ticks_without_touching_the_host() {
    let w = world("life-flag");
    w.add();
    let set = Arc::new(AtomicBool::new(true));
    assert_eq!(
        run_drain(w.config(), w.shared.clone(), &set),
        DrainEnd::Stopped
    );
    assert!(w.shared.calls().is_empty());

    // Set while the job is being started: the loop returns after that tick and leaves the job
    // running.
    let stop = flag();
    let setter = stop.clone();
    w.shared
        .on_after_start(move |_| setter.store(true, Ordering::SeqCst));
    assert_eq!(
        run_drain(w.config(), w.shared.clone(), &stop),
        DrainEnd::Stopped
    );
    assert_eq!(w.queue().jobs[0].state, JobState::Running);
}

#[test]
fn a_damaged_queue_file_ends_the_drain_with_the_queue_code() {
    let w = world("life-damaged");
    let path = cfdcraft_doc::store_dir(&w.doc).join("queue.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "this is not a queue").unwrap();

    let end = run_drain(w.config(), w.shared.clone(), &flag());
    assert_eq!(
        end,
        DrainEnd::Failed {
            code: "CFD-QUEUE-004".to_owned()
        }
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "this is not a queue");
    assert!(w.shared.calls().is_empty());
}

#[test]
fn a_runner_that_gives_up_tells_the_job_it_was_running() {
    let w = world("life-gave-up");
    let id = w.add();
    w.drain().tick().unwrap();
    assert_eq!(w.state(&id), JobState::Running);
    // A mirror file that cannot be read makes every tick fail for a reason that is not the host's.
    fs::create_dir_all(w.live(&id).join("log.blockMesh")).unwrap();

    let end = run_drain(w.config(), w.shared.clone(), &flag());
    assert!(matches!(end, DrainEnd::Failed { .. }), "{end:?}");

    let job = w.job(&id);
    assert_eq!(job.state, JobState::Running, "the run itself goes on");
    let said = job.progress.and_then(|p| p.summary).unwrap_or_default();
    assert!(said.contains("queue runner stopped"), "{said}");
    assert!(said.contains("CFD-INFRA-005"), "{said}");
    let listed = cfdcraft_ops::queue_ops::queue_list(&w.doc);
    let Some(cfdcraft_ops::Repro::Queue(data)) = listed.report.repro else {
        panic!("expected the queue");
    };
    let line = data.jobs[0].progress_line.clone().unwrap_or_default();
    assert!(line.contains("queue runner stopped"), "{line}");
    assert!(!w.file().drain_running(), "no runner holds the queue");
}

#[test]
fn the_poll_sleep_is_bounded_and_skippable() {
    // No pause at all when the poll is zero.
    let w = world("life-nosleep");
    let config = DrainConfig {
        idle_exit_after: 12,
        ..w.config()
    };
    let begun = Instant::now();
    assert_eq!(run_drain(config, w.shared.clone(), &flag()), DrainEnd::Idle);
    assert!(begun.elapsed() < Duration::from_secs(5));

    // A long poll is left early when the flag is set: the sleep is cut into short slices.
    let w = world("life-slices");
    let config = DrainConfig {
        poll: Duration::from_secs(30),
        idle_exit_after: 100,
        ..w.config()
    };
    let stop = flag();
    let setter = stop.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        setter.store(true, Ordering::SeqCst);
    });
    let begun = Instant::now();
    assert_eq!(
        run_drain(config, w.shared.clone(), &stop),
        DrainEnd::Stopped
    );
    assert!(begun.elapsed() < Duration::from_secs(5));
    thread.join().unwrap();
}

#[cfg(unix)]
#[test]
fn spawn_detached_returns_a_pid_and_the_child_outlives_the_parent_call() {
    use std::path::Path;

    let dir = scratch("life-spawn");
    let out = dir.join("out.txt");
    // With its input closed the child reads nothing: `wc` counts zero bytes.
    let script = format!("cat | wc -c > '{}'", out.display());
    let pid = spawn_detached(Path::new("/bin/sh"), &["-c".to_owned(), script]).unwrap();
    assert!(pid > 0);

    let begun = Instant::now();
    let text = loop {
        if let Ok(text) = fs::read_to_string(&out)
            && !text.trim().is_empty()
        {
            break text;
        }
        assert!(
            begun.elapsed() < Duration::from_secs(10),
            "the child never wrote"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(text.trim(), "0");
}

#[test]
fn spawn_detached_refuses_a_missing_program() {
    let dir = scratch("life-nospawn");
    let missing = dir.join("no-such-program");
    assert!(spawn_detached(&missing, &[]).is_err());
}
