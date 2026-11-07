//! A run can be told to write now and stop, a stopped run is kept and can be continued from its
//! latest time, and a large mesh runs on more than one process within the cores the host
//! allows. Everything is driven through the real operations and the real drain.

use cfdcraft_backend_esi::export::{allrun_script, allrun_script_for};
use cfdcraft_drain::{Drain, DrainConfig};
use cfdcraft_ops::queue_ops::{AddRequest, queue_add, queue_stop};
use cfdcraft_queue::JobState;

mod support;
use support::*;

#[path = "support/acceptance.rs"]
mod acceptance;
use acceptance::*;

/// A drain over the world's host that may use `np_cap` cores.
fn capped(w: &World, np_cap: u32) -> Drain<Shared> {
    let config = DrainConfig {
        np_cap,
        ..w.config()
    };
    Drain::new(config, w.shared.clone())
}

#[test]
fn write_now_stops_a_run_and_it_continues_later_from_the_latest_time() {
    // Criterion 3: write now and stop, a stopped run kept, a continue from it.
    let (w, mut drain, id, run_id) = started_job("sc3-stop-continue");
    w.shared.inner.borrow_mut().solver_obeys_stop(true);
    put_partial(&w, &run_id, 1200, 16384);
    passed(&queue_stop(&w.doc, &id).report);

    // The control file is told to write now; the run ends stopped with its latest time kept.
    assert_eq!(settle(&w, &mut drain, &id, 40), JobState::Stopped);
    assert!(control_of(&w, &run_id).contains("stopAt writeNow;"));
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert!(runs[0].result.is_none(), "a stopped run holds no result");
    assert_eq!(
        runs[0].checkpoint.as_ref().map(|c| c.time.as_str()),
        Some("1200")
    );
    assert_ne!(
        status(&w.doc).result.state,
        "fresh",
        "a stopped run does not make the result up to date"
    );
    assert!(w.shared.holder().is_none(), "the host was given back");

    // Later: a continue job starts from the saved time.
    let request = AddRequest {
        continue_run: Some("run-1".to_owned()),
        ..AddRequest::default()
    };
    passed(&queue_add(&w.doc, &request).report);
    let next = w.queue().jobs.last().unwrap().id.clone();
    let again = start_job(&w, &mut drain, &next);
    assert!(w.shared.file(&again, "1200/U").is_some());
    assert!(w.shared.file(&again, "1200/p").is_some());
    let control = control_of(&w, &again);
    assert!(control.contains("startFrom latestTime;"), "{control}");

    play_on(&w, &again);
    assert_eq!(settle(&w, &mut drain, &next, 8), JobState::Finished);
    let runs = w.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].id, "run-1");
    assert_eq!(runs[1].id, "run-2");
    let parent = runs[1]
        .continued_from
        .as_ref()
        .expect("the new run names its parent");
    assert_eq!(
        (parent.run.as_str(), parent.time.as_str()),
        ("run-1", "1200")
    );
    assert!(runs[1].result.is_some());
    assert_eq!(status(&w.doc).result.state, "fresh");
}

#[test]
fn a_large_mesh_decomposes_and_reconstructs_within_the_core_cap() {
    // Criterion 3: a parallel run inside the core cap. 90 000 cells are worth four processes at
    // 20 000 cells each; the cap decides how many are used.
    let w = big_world("sc3-parallel-two");
    let id = w.add();
    let mut drain = capped(&w, 2);
    let run_id = start_job(&w, &mut drain, &id);
    assert!(w.shared.file(&run_id, "system/decomposeParDict").is_some());
    let script = text_of(&w, &run_id, "Allrun");
    assert_eq!(script, allrun_script_for(2));
    for step in ["decomposePar", "mpirun -np 2", "reconstructPar -latestTime"] {
        assert!(script.contains(step), "{step} in {script}");
    }
    play_cells(&w, &run_id, BIG_CELLS, true);
    assert_eq!(settle(&w, &mut drain, &id, 8), JobState::Finished);
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].np, Some(2));
    let report = report_of(&w, &id);
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("Running on 2 processes (90000 cells).")),
        "{:?}",
        report.lines
    );

    // With one core allowed the same study runs serially.
    let w = big_world("sc3-parallel-one");
    let id = w.add();
    let mut drain = capped(&w, 1);
    let run_id = start_job(&w, &mut drain, &id);
    assert!(w.shared.file(&run_id, "system/decomposeParDict").is_none());
    assert_eq!(text_of(&w, &run_id, "Allrun"), allrun_script());
    play_cells(&w, &run_id, BIG_CELLS, false);
    assert_eq!(settle(&w, &mut drain, &id, 8), JobState::Finished);
    assert_eq!(w.runs()[0].np, Some(1));

    // The 16 384-cell cavity is serial whatever the cap.
    for cap in [1, 2, 4] {
        let w = world(&format!("sc3-parallel-small-{cap}"));
        let id = w.add();
        let mut drain = capped(&w, cap);
        let run_id = start_job(&w, &mut drain, &id);
        assert!(
            w.shared.file(&run_id, "system/decomposeParDict").is_none(),
            "cap {cap}"
        );
        assert_eq!(text_of(&w, &run_id, "Allrun"), allrun_script(), "cap {cap}");
    }
}

#[test]
fn a_parallel_run_can_be_stopped_and_continued_on_the_same_processes() {
    // Criterion 3: stop and continue work for a run that was split across processes.
    let w = big_world("sc3-parallel-continue");
    let id = w.add();
    let mut drain = capped(&w, 2);
    let run_id = start_job(&w, &mut drain, &id);
    w.shared.inner.borrow_mut().solver_obeys_stop(true);
    put_partial(&w, &run_id, 1200, BIG_CELLS);
    passed(&queue_stop(&w.doc, &id).report);
    assert_eq!(settle(&w, &mut drain, &id, 40), JobState::Stopped);
    assert_eq!(w.runs()[0].np, Some(2));

    let request = AddRequest {
        continue_run: Some("run-1".to_owned()),
        ..AddRequest::default()
    };
    passed(&queue_add(&w.doc, &request).report);
    let next = w.queue().jobs.last().unwrap().id.clone();
    let again = start_job(&w, &mut drain, &next);
    assert!(w.shared.file(&again, "system/decomposeParDict").is_some());
    assert!(w.shared.file(&again, "1200/U").is_some());
    assert!(text_of(&w, &again, "Allrun").contains("mpirun -np 2"));
    assert!(control_of(&w, &again).contains("startFrom latestTime;"));
}
