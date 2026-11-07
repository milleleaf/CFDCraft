//! A queued job runs on the in-memory host, from the first tick to a run recorded in the
//! document, and every state change is in the queue file after the tick that made it.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use cfdcraft_drain::TickOutcome;
use cfdcraft_drain::finalize::{JobReport, ReportOutcome};
use cfdcraft_ops::queue_file::QueueFile;
use cfdcraft_ops::queue_ops::{AddRequest, queue_add};
use cfdcraft_ops::scenario_ops::scenario_set;
use cfdcraft_queue::{JobState, Queue, check_queue};
use cfdcraft_remote::ContainerState;
use cfdcraft_scenarios::Override;
use serde_json::json;

mod support;
use support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";

fn report_of(w: &World, job: &str) -> JobReport {
    let text = fs::read_to_string(w.live(job).join("report.json")).expect("a report was written");
    serde_json::from_str(&text).expect("the report reads")
}

/// The state of the only job, after reading the file the way a reader would and checking every
/// rule of a queue.
fn checked_state(queue_json: &Path) -> JobState {
    let text = fs::read_to_string(queue_json).unwrap();
    let queue = Queue::from_json(&text).expect("queue.json reads");
    check_queue(&queue).expect("queue.json keeps the rules");
    queue.jobs[0].state
}

#[test]
fn one_queued_job_runs_to_a_recorded_run() {
    let w = world("happy-run");
    let id = w.add();
    let mut drain = w.drain();

    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Running);
    let run_id = w.run_id(&id);
    let control = w
        .shared
        .file(&run_id, "system/controlDict")
        .expect("the case was uploaded");
    assert!(String::from_utf8_lossy(&control).contains("probe_1"));
    assert!(w.shared.file(&run_id, "Allrun").is_some());
    assert!(w.live(&id).join("spec.json").is_file());

    w.play(&run_id);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    assert_eq!(w.state(&id), JobState::Finishing);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);

    let job = w.job(&id);
    assert_eq!(job.state, JobState::Finished);
    assert_eq!(job.run.as_deref(), Some("run-1"));
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, "run-1");
    assert!(runs[0].result.is_some());
    assert!(runs[0].verdict.is_some(), "the verdict is recorded");
    assert_eq!(runs[0].np, Some(1));

    assert!(w.shared.holder().is_none(), "the lock was given back");
    assert_eq!(w.shared.container(&run_id), ContainerState::None);
    let report = report_of(&w, &id);
    assert_eq!(report.outcome, ReportOutcome::Finished);
    assert_eq!(report.run.as_deref(), Some("run-1"));
    let text = fs::read_to_string(w.live(&id).join("report.json")).unwrap();
    assert!(
        !text.contains("cfdcraft-cavity"),
        "no container name: {text}"
    );
}

#[test]
fn every_state_change_is_in_queue_json_after_the_tick_that_made_it() {
    let w = world("happy-states");
    let id = w.add();
    let mut seen = vec![w.state(&id)];
    let doc = w.doc.clone();
    let hooked = Rc::new(RefCell::new(Vec::new()));
    let sink = hooked.clone();
    w.shared.on_before_start(move |_| {
        let queue = QueueFile::new(&doc).read().unwrap();
        sink.borrow_mut().push(queue.jobs[0].state);
    });
    let mut drain = w.drain();
    let queue_json = cfdcraft_doc::store_dir(&w.doc).join("queue.json");

    drain.tick().unwrap();
    seen.push(checked_state(&queue_json));
    let run_id = w.run_id(&id);
    drain.tick().unwrap();
    seen.push(checked_state(&queue_json));
    w.play(&run_id);
    drain.tick().unwrap();
    seen.push(checked_state(&queue_json));
    drain.tick().unwrap();
    seen.push(checked_state(&queue_json));

    let mut all = vec![seen[0]];
    all.extend(hooked.borrow().iter().copied());
    all.extend(seen[1..].iter().copied());
    all.dedup();
    assert_eq!(
        all,
        [
            JobState::Queued,
            JobState::Starting,
            JobState::Running,
            JobState::Finishing,
            JobState::Finished
        ]
    );
}

#[test]
fn the_run_id_is_known_before_the_host_is_touched() {
    let w = world("happy-runid");
    let id = w.add();
    let doc = w.doc.clone();
    let found = Rc::new(RefCell::new(None));
    let sink = found.clone();
    let job_id = id.clone();
    w.shared.on_before_start(move |run_id| {
        let queue = QueueFile::new(&doc).read().unwrap();
        let job = queue.jobs.iter().find(|j| j.id == job_id).unwrap();
        *sink.borrow_mut() = Some((job.state, job.run_id.clone(), run_id.to_owned()));
    });
    w.drain().tick().unwrap();

    let (state, stored, passed_to_host) = found.borrow().clone().expect("the host was called");
    assert_eq!(state, JobState::Starting);
    assert_eq!(stored.as_deref(), Some(passed_to_host.as_str()));
}

#[test]
fn a_second_job_waits_for_the_first() {
    let w = world("happy-two");
    let first = w.add();
    let change = Override::Set {
        setting: LID.to_owned(),
        value: json!(0.2),
    };
    passed(&scenario_set(&w.doc, "fast-lid", Some("base"), &[change], None).report);
    let request = AddRequest {
        scenario: Some("fast-lid".to_owned()),
        ..AddRequest::default()
    };
    passed(&queue_add(&w.doc, &request).report);
    let second = w.queue().jobs.last().unwrap().id.clone();
    assert_ne!(first, second);

    let mut drain = w.drain();
    drain.tick().unwrap();
    let run_id = w.run_id(&first);
    for _ in 0..3 {
        drain.tick().unwrap();
        assert_eq!(w.state(&second), JobState::Queued);
    }
    w.play(&run_id);
    drain.tick().unwrap();
    drain.tick().unwrap();
    assert_eq!(w.state(&first), JobState::Finished);
    assert_eq!(w.state(&second), JobState::Queued);

    drain.tick().unwrap();
    assert_eq!(w.state(&second), JobState::Running);
    assert_ne!(w.run_id(&second), run_id);
}

#[test]
fn an_empty_queue_is_idle() {
    let w = world("happy-idle");
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Idle);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Idle);
    assert!(w.shared.calls().is_empty(), "{:?}", w.shared.calls());
}

#[test]
fn a_finished_run_that_failed_the_evidence_is_a_failed_job_and_records_nothing() {
    let w = world("happy-evidence");
    let id = w.add();
    let mut drain = w.drain();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    w.play(&run_id);
    w.shared
        .append(&run_id, "log.simpleFoam", "--> FOAM FATAL ERROR:\nbroken\n");
    drain.tick().unwrap();
    drain.tick().unwrap();

    let job = w.job(&id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some("CFD-SOLVER-099"));
    assert!(w.runs().is_empty(), "nothing was recorded");
    assert!(w.shared.holder().is_none());
    assert_eq!(w.shared.container(&run_id), ContainerState::None);
    assert!(
        w.live(&id).join("log.simpleFoam").is_file(),
        "the mirror is kept"
    );
    let report = report_of(&w, &id);
    assert_eq!(report.outcome, ReportOutcome::Failed);
    assert_eq!(report.code.as_deref(), Some("CFD-SOLVER-099"));
}

#[test]
fn the_document_changing_while_the_case_runs_fails_the_job_but_keeps_the_results_folder() {
    let w = world("happy-changed");
    let id = w.add();
    let mut drain = w.drain();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);

    passed(&cfdcraft_ops::save_document(&with_lid_speed(0.2), &w.doc, true).report);
    w.play(&run_id);
    drain.tick().unwrap();
    drain.tick().unwrap();

    let job = w.job(&id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some("CFD-REPRO-009"));
    assert!(
        w.runs().is_empty(),
        "the run is not recorded for another study"
    );
    assert!(w.live(&id).join("results").is_dir());
    assert!(w.shared.holder().is_none());
}

#[test]
fn the_drain_only_acts_on_its_own_document() {
    let w = world("happy-own");
    let other = new_document(&w.dir, "other", &with_lid_speed(0.3));
    passed(&queue_add(&other, &AddRequest::default()).report);
    let other_queue = QueueFile::new(&other);
    let before = fs::read(w.dir.join("other.cfdstore").join("queue.json")).ok();
    let other_text = other_queue.read().unwrap().to_json();

    let id = w.add();
    let mut drain = w.drain();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    w.play(&run_id);
    drain.tick().unwrap();
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finished);

    assert_eq!(other_queue.read().unwrap().to_json(), other_text);
    assert_eq!(
        fs::read(w.dir.join("other.cfdstore").join("queue.json")).ok(),
        before
    );
    assert!(!other_queue.live_dir("q-001").unwrap().exists());
    for call in w.shared.calls() {
        assert!(!call.contains("other"), "{call}");
    }
}
