//! The queue file: where it lives, how it is read and changed under its lock, and the right to run
//! it. No test needs a compute host or a container engine.

use std::fs;
use std::path::PathBuf;
use std::thread;

use cfdcraft_doc::store_dir;
use cfdcraft_ops::ExitClass;
use cfdcraft_ops::queue_file::{QueueFile, QueueFileError};
use cfdcraft_ops::queue_ops::{queue_add, queue_list};
use cfdcraft_queue::{Action, JobKind, NewJob, Queue, QueueError};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

fn cavity(tag: &str) -> PathBuf {
    new_document(&scratch(tag), "cavity", CAVITY)
}

fn one_job() -> Action {
    Action::Enqueue {
        jobs: vec![NewJob {
            scenario: None,
            kind: JobKind::Solve,
            origin: None,
            backend: None,
        }],
    }
}

fn queue_path(doc: &std::path::Path) -> PathBuf {
    store_dir(doc).join("queue.json")
}

#[test]
fn a_missing_queue_file_reads_as_an_empty_queue() {
    let doc = cavity("qf-missing");
    let file = QueueFile::new(&doc);
    assert_eq!(file.read().unwrap(), Queue::new());
    assert!(!queue_path(&doc).exists(), "reading makes no file");
    let listed = queue_list(&doc);
    passed(&listed.report);
    assert!(!queue_path(&doc).exists(), "listing makes no file");
}

#[test]
fn a_damaged_or_newer_queue_file_is_refused_by_name() {
    for (tag, text) in [
        ("garbage", "this is not a queue"),
        ("newer", "{\"schema_version\": 2, \"jobs\": []}\n"),
    ] {
        let doc = cavity(&format!("qf-damaged-{tag}"));
        fs::create_dir_all(store_dir(&doc)).unwrap();
        fs::write(queue_path(&doc), text).unwrap();
        let file = QueueFile::new(&doc);

        assert!(matches!(
            file.read(),
            Err(QueueFileError::Queue {
                source: QueueError::Damaged { .. } | QueueError::NewerSchema { .. }
            })
        ));
        let update = file.update(|_| Ok(vec![one_job()]));
        assert!(update.is_err(), "{tag}: a damaged file is not changed");

        for out in [queue_list(&doc), queue_add(&doc, &Default::default())] {
            assert_eq!(codes(&out.report), ["CFD-QUEUE-004"], "{tag}");
            assert_eq!(out.report.class(), ExitClass::Infrastructure, "{tag}");
        }
        let after = fs::read_to_string(queue_path(&doc)).unwrap();
        assert_eq!(after, text, "{tag}: nothing is overwritten");
    }
}

#[test]
fn two_updates_under_the_lock_do_not_lose_either() {
    let doc = cavity("qf-hammer");
    let file = QueueFile::new(&doc);
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let file = file.clone();
            thread::spawn(move || {
                for _ in 0..20 {
                    file.update(|_| Ok(vec![one_job()])).unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let queue = file.read().unwrap();
    let ids: Vec<String> = queue.jobs.iter().map(|job| job.id.clone()).collect();
    let expected: Vec<String> = (1..=40).map(|n| format!("q-{n:03}")).collect();
    assert_eq!(ids, expected, "no gap and no repeat");
}

#[test]
fn a_move_that_fails_writes_nothing() {
    let doc = cavity("qf-failed-move");
    let file = QueueFile::new(&doc);
    file.update(|_| Ok(vec![one_job()])).unwrap();
    let before = fs::read(queue_path(&doc)).unwrap();
    let cancel = Action::Cancel {
        id: "q-009".to_owned(),
    };
    let failed = file.update(|_| Ok(vec![one_job(), cancel]));
    assert!(matches!(
        failed,
        Err(QueueFileError::Queue {
            source: QueueError::Unknown { .. }
        })
    ));
    assert_eq!(fs::read(queue_path(&doc)).unwrap(), before);
}

#[test]
fn the_drain_lock_is_taken_once() {
    let doc = cavity("qf-drain");
    let file = QueueFile::new(&doc);
    assert!(!file.drain_running(), "no runner has been started");

    let first = file.drain_lock().unwrap();
    assert!(first.is_some());
    assert!(file.drain_running());
    assert!(
        file.drain_lock().unwrap().is_none(),
        "the second finds it taken"
    );

    drop(first);
    assert!(!file.drain_running());
    let third = file.drain_lock().unwrap();
    assert!(third.is_some(), "free again once the first is dropped");
    assert!(file.drain_running());
}

#[test]
fn a_job_folder_is_named_only_for_a_job_id() {
    let doc = cavity("qf-live");
    let file = QueueFile::new(&doc);
    let live = file.live_dir("q-007").unwrap();
    assert_eq!(live, store_dir(&doc).join("live").join("q-007"));
    for bad in [
        "",
        "q-",
        "q-x",
        "q-1/../..",
        "../q-001",
        "q-12345678901",
        "Q-001",
    ] {
        assert!(file.live_dir(bad).is_none(), "{bad:?}");
    }
}
