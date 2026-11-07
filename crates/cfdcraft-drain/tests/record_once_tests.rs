//! A run is recorded once, whatever fails after it was recorded and before the report is written:
//! the next try finds the run in the document and ends the job with it.
#![cfg(unix)]

use std::fs;

use cfdcraft_drain::finalize::{JobReport, ReportOutcome};
use cfdcraft_queue::JobState;

mod support;
use support::*;

fn report_of(w: &World, job: &str) -> JobReport {
    let text = fs::read_to_string(w.live(job).join("report.json")).expect("a report was written");
    serde_json::from_str(&text).expect("the report reads")
}

/// Makes the report impossible to write: a folder sits where the file must go.
fn block_report(w: &World, id: &str) {
    fs::create_dir_all(w.live(id).join("report.json")).unwrap();
}

fn unblock_report(w: &World, id: &str) {
    fs::remove_dir_all(w.live(id).join("report.json")).unwrap();
}

#[test]
fn a_finished_run_whose_report_failed_is_not_recorded_twice() {
    let w = world("once-finished");
    let id = w.add();
    let mut drain = w.drain();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    w.play(&run_id);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finishing);

    block_report(&w, &id);
    assert!(drain.tick().is_err(), "the report could not be written");
    assert_eq!(w.runs().len(), 1, "the run was recorded before the failure");
    assert_eq!(w.state(&id), JobState::Finishing);

    unblock_report(&w, &id);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finished);
    assert_eq!(w.job(&id).run.as_deref(), Some("run-1"));
    assert_eq!(w.runs().len(), 1, "still one run");
    let report = report_of(&w, &id);
    assert_eq!(report.outcome, ReportOutcome::Finished);
    assert_eq!(report.run.as_deref(), Some("run-1"));
    assert!(w.shared.holder().is_none(), "the lock was given back");
}
