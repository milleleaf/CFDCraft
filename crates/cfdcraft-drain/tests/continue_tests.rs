//! A continue job starts a new case from the saved state of a stopped run and records a run that
//! names its parent.

use std::fs;

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_drain::Drain;
use cfdcraft_drain::finalize::JobReport;
use cfdcraft_ops::queue_ops::{AddRequest, queue_add, queue_stop};
use cfdcraft_queue::JobState;
use cfdcraft_store::Store;

mod support;
use support::*;

/// What a solver that is cut short at `time` has left in the run: the mesh, the logs, and the
/// fields of that time. The solver log does not end yet; a host that follows a stop adds its end.
fn put_partial(w: &World, run_id: &str, time: u64) {
    let dir = w.dir.join("partial");
    let _ = fs::remove_dir_all(&dir);
    let spec = FixtureSpec {
        end_time: time,
        cells: 16384,
        samples: 64,
        shift: 0.0,
    };
    write_results(&dir, &spec).unwrap();
    for rel in files_under(&dir) {
        let mut text = fs::read_to_string(dir.join(&rel)).unwrap();
        if rel == "log.simpleFoam" {
            text = text.trim_end().strip_suffix("End").unwrap().to_owned();
        }
        w.shared.replace(run_id, &rel, &text);
    }
}

/// Runs the study, stops it at time 1200 and returns the drain with the stopped run recorded as
/// `run-1`.
fn stopped(w: &World) -> Drain<Shared> {
    let id = w.add();
    let mut drain = w.drain();
    drain.tick().unwrap();
    w.shared.inner.borrow_mut().solver_obeys_stop(true);
    let run_id = w.run_id(&id);
    put_partial(w, &run_id, 1200);
    passed(&queue_stop(&w.doc, &id).report);
    for _ in 0..6 {
        drain.tick().unwrap();
        if w.state(&id).is_terminal() {
            break;
        }
    }
    assert_eq!(w.state(&id), JobState::Stopped);
    drain
}

/// Queues a job that continues the run `run` and returns its id.
fn add_continue(w: &World, run: &str) -> String {
    let request = AddRequest {
        continue_run: Some(run.to_owned()),
        ..AddRequest::default()
    };
    passed(&queue_add(&w.doc, &request).report);
    w.queue().jobs.last().unwrap().id.clone()
}

/// Continues `run-1` and lets the solver go to its end. Returns the id of the continue job and the
/// run name it carried.
fn continued(w: &World, drain: &mut Drain<Shared>) -> (String, String) {
    let id = add_continue(w, "run-1");
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Running);
    let run_id = w.run_id(&id);
    w.play(&run_id);
    for _ in 0..6 {
        drain.tick().unwrap();
        if w.state(&id).is_terminal() {
            break;
        }
    }
    (id, run_id)
}

#[test]
fn a_continue_job_starts_a_case_from_the_checkpoint() {
    let w = world("cont-start");
    let mut drain = stopped(&w);
    let before = w.runs();
    assert_eq!(before.len(), 1);

    let id = add_continue(&w, "run-1");
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Running);
    let run_id = w.run_id(&id);
    assert!(w.shared.file(&run_id, "1200/U").is_some());
    assert!(w.shared.file(&run_id, "1200/p").is_some());
    let control = String::from_utf8(w.shared.file(&run_id, "system/controlDict").unwrap()).unwrap();
    assert!(control.contains("startFrom latestTime;"), "{control}");

    w.play(&run_id);
    for _ in 0..6 {
        drain.tick().unwrap();
        if w.state(&id).is_terminal() {
            break;
        }
    }
    assert_eq!(w.state(&id), JobState::Finished);
    let runs = w.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0], before[0], "the stopped run is unchanged");
    let parent = runs[1]
        .continued_from
        .as_ref()
        .expect("the parent is named");
    assert_eq!(
        (parent.run.as_str(), parent.time.as_str()),
        ("run-1", "1200")
    );
    assert!(runs[1].result.is_some());
    assert!(runs[1].verdict.is_some());
}

#[test]
fn the_continue_line_names_the_parent() {
    let w = world("cont-line");
    let mut drain = stopped(&w);
    let (id, _) = continued(&w, &mut drain);
    assert_eq!(w.state(&id), JobState::Finished);
    let text = fs::read_to_string(w.live(&id).join("report.json")).unwrap();
    let report: JobReport = serde_json::from_str(&text).unwrap();
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("Continuing run-1 from time 1200.")),
        "{:?}",
        report.lines
    );
}

#[test]
fn a_missing_checkpoint_blob_fails_the_job_with_the_continue_code() {
    let w = world("cont-missing");
    let mut drain = stopped(&w);
    let blob = w.runs()[0].checkpoint.as_ref().expect("a checkpoint").blob;
    let id = add_continue(&w, "run-1");
    let store = Store::open(cfdcraft_doc::store_dir(&w.doc)).unwrap();
    assert!(store.remove(&blob).unwrap(), "the blob was there");

    let starts = || {
        w.shared
            .calls()
            .iter()
            .filter(|c| c.starts_with("start "))
            .count()
    };
    assert_eq!(starts(), 1, "only the first job was started");
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Failed);
    assert_eq!(w.job(&id).code.as_deref(), Some("CFD-CONT-001"));
    assert_eq!(starts(), 1, "nothing was sent to the host for it");
    assert_eq!(w.runs().len(), 1);
}
