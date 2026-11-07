//! The queue operations on one document: adding the study, listing, and the refusals that leave
//! the file as it was. No test needs a compute host or a container engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_doc::store_dir;
use cfdcraft_ops::queue_file::QueueFile;
use cfdcraft_ops::queue_ops::{
    AddRequest, queue_add, queue_cancel, queue_list, queue_reorder, queue_stop,
};
use cfdcraft_ops::{DocOutcome, ExitClass, QueueData, Report, Repro};
use cfdcraft_queue::{Action, JobKind, NewJob, Outcome, Queue, StopCause, StopMode, StopRequest};
use cfdcraft_text::Line;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

fn cavity(tag: &str) -> PathBuf {
    new_document(&scratch(tag), "cavity", CAVITY)
}

fn queue_of(report: &Report) -> &QueueData {
    match &report.repro {
        Some(Repro::Queue(data)) => data.as_ref(),
        other => panic!("expected the queue, found {other:?}"),
    }
}

fn queue_path(doc: &Path) -> PathBuf {
    store_dir(doc).join("queue.json")
}

#[test]
fn adding_the_study_creates_one_queued_job() {
    let doc = cavity("qo-add-study");
    let added = queue_add(&doc, &AddRequest::default());
    passed(&added.report);
    let line = added.line.expect("a sentence for the person");
    assert_eq!(say(&line), "Added 1 jobs to the queue; 1 are waiting.");

    let listed = queue_list(&doc);
    passed(&listed.report);
    let data = queue_of(&listed.report);
    assert_eq!(data.jobs.len(), 1);
    let job = &data.jobs[0];
    assert_eq!(job.id, "q-001");
    assert_eq!(job.state, "queued");
    assert_eq!(job.scenario, None);
    assert_eq!(job.kind, "solve");
    assert_eq!(data.active, None);
    assert!(!data.drain);
}

#[test]
fn queue_json_sits_in_the_store_folder_and_is_valid() {
    let doc = cavity("qo-file");
    passed(&queue_add(&doc, &AddRequest::default()).report);
    let text = fs::read_to_string(queue_path(&doc)).expect("queue.json is in the store folder");
    let queue = Queue::from_json(&text).expect("the file is a valid queue");
    assert_eq!(queue.jobs.len(), 1);

    let mut names: Vec<String> = fs::read_dir(store_dir(&doc))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["queue.json", "queue.lock"],
        "no staging file is left"
    );
}

#[test]
fn a_pack_or_a_missing_document_is_refused() {
    let dir = scratch("qo-refused");
    let pack = dir.join("shared.cfdpack");
    let missing = dir.join("nothing.cfdcraft");
    for out in [queue_add(&pack, &AddRequest::default()), queue_list(&pack)] {
        assert_eq!(codes(&out.report), ["CFD-QUEUE-001"]);
        assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    }
    for out in [
        queue_add(&missing, &AddRequest::default()),
        queue_list(&missing),
    ] {
        assert_eq!(codes(&out.report), ["CFD-DOC-001"]);
        assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    }
    assert!(
        !store_dir(&missing).exists(),
        "a refused request makes no folder"
    );
}

#[test]
fn the_report_names_no_folder() {
    let doc = cavity("qo-no-folder");
    for out in [queue_add(&doc, &AddRequest::default()), queue_list(&doc)] {
        let text = out.report.render_text();
        let json = out.report.to_json();
        for shown in [&text, &json] {
            assert!(!shown.contains("cfdstore"), "{shown}");
            assert!(!shown.contains("queue.json"), "{shown}");
            assert!(!shown.contains("queue.lock"), "{shown}");
        }
        assert!(matches!(out.line, None | Some(Line::QueueAdded { .. })));
    }
}

// Moving, cancelling and stopping jobs. The jobs are put in the queue and moved along their life
// with the same moves the background runner makes, because no runner is started here.

fn study_job() -> NewJob {
    NewJob {
        scenario: None,
        kind: JobKind::Solve,
        origin: None,
        backend: None,
    }
}

/// Apply `moves` to the queue of the document.
fn play(doc: &Path, moves: Vec<Action>) {
    QueueFile::new(doc).update(|_| Ok(moves)).unwrap();
}

/// A document whose queue holds `count` waiting jobs for the study.
fn seeded(tag: &str, count: usize) -> PathBuf {
    let doc = cavity(tag);
    let jobs = (0..count).map(|_| study_job()).collect();
    play(&doc, vec![Action::Enqueue { jobs }]);
    doc
}

/// Make the first waiting job `id` a running one.
fn start(doc: &Path, id: &str) {
    let id = id.to_owned();
    let moves = vec![
        Action::Begin {
            id: id.clone(),
            run_id: "run-abc".to_owned(),
        },
        Action::Started { id },
    ];
    play(doc, moves);
}

fn state_of(doc: &Path, id: &str) -> String {
    let listed = queue_list(doc);
    let data = queue_of(&listed.report);
    let job = data.jobs.iter().find(|job| job.id == id).unwrap();
    job.state.clone()
}

fn order_of(out: &DocOutcome) -> Vec<&str> {
    let jobs = &queue_of(&out.report).jobs;
    jobs.iter().map(|job| job.id.as_str()).collect()
}

fn refused_with(out: &DocOutcome, code: &str) {
    assert_eq!(codes(&out.report), [code]);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert!(out.line.is_none(), "a refusal has no success sentence");
}

fn bytes(doc: &Path) -> Vec<u8> {
    fs::read(queue_path(doc)).unwrap()
}

#[test]
fn reorder_moves_a_waiting_job() {
    let doc = seeded("qo-reorder", 3);
    let out = queue_reorder(&doc, "q-003", 0);
    passed(&out.report);
    assert_eq!(order_of(&out), ["q-003", "q-001", "q-002"]);
    let line = out.line.as_ref().expect("a sentence for the person");
    assert_eq!(
        say(line),
        "Moved q-003 to position 1 among the waiting jobs."
    );

    // A place past the end is the last place.
    let out = queue_reorder(&doc, "q-003", 99);
    passed(&out.report);
    assert_eq!(order_of(&out), ["q-001", "q-002", "q-003"]);
    let line = out.line.as_ref().expect("a sentence for the person");
    assert_eq!(
        say(line),
        "Moved q-003 to position 3 among the waiting jobs."
    );
}

#[test]
fn a_running_job_cannot_be_reordered() {
    let doc = seeded("qo-reorder-running", 2);
    start(&doc, "q-001");
    let out = queue_reorder(&doc, "q-001", 1);
    refused_with(&out, "CFD-QUEUE-003");
    let said = Line::QueueNotAllowed {
        job: "q-001".to_owned(),
        state: "running".to_owned(),
        action: "moved".to_owned(),
    };
    assert_eq!(out.report.diagnostics[0].message, say(&said));
}

#[test]
fn an_unknown_job_is_cfd_queue_002() {
    let doc = seeded("qo-unknown", 1);
    let before = bytes(&doc);
    for out in [
        queue_reorder(&doc, "q-009", 0),
        queue_cancel(&doc, "q-009"),
        queue_stop(&doc, "q-009"),
        queue_cancel(&doc, "q-0\n9"),
    ] {
        refused_with(&out, "CFD-QUEUE-002");
        let message = &out.report.diagnostics[0].message;
        assert!(!message.chars().any(char::is_control), "{message:?}");
    }
    assert_eq!(bytes(&doc), before);
}

#[test]
fn cancelling_a_waiting_job_ends_it() {
    let doc = seeded("qo-cancel-waiting", 2);
    let out = queue_cancel(&doc, "q-002");
    passed(&out.report);
    assert_eq!(state_of(&doc, "q-002"), "cancelled");
    assert_eq!(state_of(&doc, "q-001"), "queued");
    let line = out.line.as_ref().expect("a sentence for the person");
    assert_eq!(say(line), "Cancelled q-002.");
}

#[test]
fn cancelling_a_running_job_asks_for_a_write_now_stop() {
    let doc = seeded("qo-cancel-running", 1);
    start(&doc, "q-001");
    let out = queue_cancel(&doc, "q-001");
    passed(&out.report);
    assert_eq!(
        state_of(&doc, "q-001"),
        "running",
        "it ends when it has stopped"
    );
    let queue = QueueFile::new(&doc).read().unwrap();
    assert_eq!(
        queue.jobs[0].stop,
        Some(StopRequest {
            mode: StopMode::WriteNow,
            cause: StopCause::Cancel,
        })
    );
    let line = out.line.as_ref().expect("a sentence for the person");
    assert_eq!(
        say(line),
        "Asked q-001 to write its latest results and stop."
    );
}

#[test]
fn stop_needs_a_running_job() {
    let doc = seeded("qo-stop", 1);
    refused_with(&queue_stop(&doc, "q-001"), "CFD-QUEUE-003");

    start(&doc, "q-001");
    let first = queue_stop(&doc, "q-001");
    passed(&first.report);
    let stop = QueueFile::new(&doc).read().unwrap().jobs[0].stop;
    assert_eq!(
        stop,
        Some(StopRequest {
            mode: StopMode::WriteNow,
            cause: StopCause::User,
        })
    );

    // Asked again, it is accepted and nothing changes.
    let before = bytes(&doc);
    passed(&queue_stop(&doc, "q-001").report);
    assert_eq!(bytes(&doc), before);

    // The same once the runner has carried the stop out and the job is stopping.
    play(
        &doc,
        vec![Action::StopApplied {
            id: "q-001".to_owned(),
        }],
    );
    let before = bytes(&doc);
    passed(&queue_stop(&doc, "q-001").report);
    assert_eq!(state_of(&doc, "q-001"), "stopping");
    assert_eq!(bytes(&doc), before);
}

/// A document whose first job ended as finished and whose second is waiting.
fn one_finished(tag: &str) -> PathBuf {
    let doc = seeded(tag, 2);
    start(&doc, "q-001");
    let id = "q-001".to_owned();
    let run = "run-1".to_owned();
    play(
        &doc,
        vec![
            Action::ContainerEnded { id: id.clone() },
            Action::Complete {
                id,
                outcome: Outcome::Finished { run },
            },
        ],
    );
    doc
}

#[test]
fn a_finished_job_cannot_be_cancelled_or_stopped() {
    let doc = one_finished("qo-finished");
    assert_eq!(state_of(&doc, "q-001"), "finished");
    refused_with(&queue_cancel(&doc, "q-001"), "CFD-QUEUE-003");
    refused_with(&queue_stop(&doc, "q-001"), "CFD-QUEUE-003");
    refused_with(&queue_reorder(&doc, "q-001", 0), "CFD-QUEUE-003");
    assert_eq!(state_of(&doc, "q-001"), "finished");
}

#[test]
fn every_refusal_leaves_the_file_unchanged() {
    let doc = one_finished("qo-unchanged");
    start(&doc, "q-002");
    let before = bytes(&doc);
    let refusals = [
        queue_reorder(&doc, "q-002", 0),
        queue_reorder(&doc, "q-001", 0),
        queue_reorder(&doc, "q-077", 0),
        queue_cancel(&doc, "q-001"),
        queue_cancel(&doc, "q-077"),
        queue_stop(&doc, "q-001"),
        queue_stop(&doc, "q-077"),
    ];
    for out in &refusals {
        assert_eq!(out.report.class(), ExitClass::StudyInvalid);
        assert_eq!(bytes(&doc), before);
    }
}
