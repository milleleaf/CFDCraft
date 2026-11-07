//! A stop request on a running job is an edit of the control file on the host, a run that is cut
//! short is recorded as stopped with its latest time, and the lock is given back.

use std::cell::Cell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_drain::finalize::{JobReport, ReportOutcome};
use cfdcraft_drain::{Drain, DrainConfig};
use cfdcraft_ops::queue_ops::{queue_cancel, queue_stop};
use cfdcraft_queue::{Action, JobState, StopCause, StopMode};
use cfdcraft_remote::{
    ContainerState, Holder, Host, HostError, ReleaseOutcome, RemoteFile, TimeDir,
};

mod support;
use support::*;

const CONTROL: &str = "system/controlDict";

/// A started job on a host whose solver follows a stop when `obeys` is on. Returns the job id, the
/// run name and the drain.
fn running(w: &World, obeys: bool) -> (String, String, Drain<Shared>) {
    let id = w.add();
    let mut drain = w.drain();
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Running);
    w.shared.inner.borrow_mut().solver_obeys_stop(obeys);
    let run_id = w.run_id(&id);
    (id, run_id, drain)
}

/// What a solver that is cut short at `time` has left in the run: the mesh, the logs, and the
/// fields of that time. The solver log does not end yet; the part of the solver that follows a
/// stop adds its end.
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

/// Ticks until the job has ended, and returns its state.
fn settle<H: Host>(w: &World, drain: &mut Drain<H>, id: &str) -> JobState {
    for _ in 0..40 {
        drain.tick().unwrap();
        if w.state(id).is_terminal() {
            break;
        }
    }
    w.state(id)
}

fn report_of(w: &World, job: &str) -> JobReport {
    let text = fs::read_to_string(w.live(job).join("report.json")).expect("a report was written");
    serde_json::from_str(&text).expect("the report reads")
}

fn control_of(w: &World, run_id: &str) -> String {
    String::from_utf8(w.shared.file(run_id, CONTROL).expect("the control file")).unwrap()
}

fn request(w: &World, id: &str, mode: StopMode, cause: StopCause) {
    let id = id.to_owned();
    w.file()
        .update(|_| Ok(vec![Action::RequestStop { id, mode, cause }]))
        .unwrap();
}

#[test]
fn a_user_stop_ends_with_a_stopped_run_that_can_be_continued() {
    let w = world("stop-user");
    let (id, run_id, mut drain) = running(&w, true);
    put_partial(&w, &run_id, 1200);
    passed(&queue_stop(&w.doc, &id).report);

    drain.tick().unwrap();
    assert_ne!(w.state(&id), JobState::Running, "the stop was carried out");
    assert!(control_of(&w, &run_id).contains("stopAt writeNow;"));

    assert_eq!(settle(&w, &mut drain, &id), JobState::Stopped);
    assert_eq!(w.job(&id).run.as_deref(), Some("run-1"));
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, "run-1");
    assert!(runs[0].result.is_none(), "a stopped run holds no result");
    assert_eq!(
        runs[0].checkpoint.as_ref().map(|c| c.time.as_str()),
        Some("1200")
    );
    assert_eq!(runs[0].np, Some(1));

    let report = report_of(&w, &id);
    assert_eq!(report.outcome, ReportOutcome::Stopped);
    assert_eq!(report.run.as_deref(), Some("run-1"));
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("run-1 is a stopped run (latest time 1200)")),
        "{:?}",
        report.lines
    );
    assert!(w.shared.holder().is_none(), "the lock was given back");
    assert_eq!(w.shared.container(&run_id), ContainerState::None);
}

/// A stopped run whose report could not be written is found again in the document, not recorded
/// a second time.
#[cfg(unix)]
#[test]
fn a_stopped_run_whose_report_failed_is_not_recorded_twice() {
    let w = world("stop-once");
    let (id, run_id, mut drain) = running(&w, true);
    put_partial(&w, &run_id, 1200);
    passed(&queue_stop(&w.doc, &id).report);
    for _ in 0..10 {
        if w.state(&id) == JobState::Finishing {
            break;
        }
        drain.tick().unwrap();
    }
    assert_eq!(w.state(&id), JobState::Finishing);

    let report = w.live(&id).join("report.json");
    fs::create_dir_all(&report).unwrap();
    assert!(drain.tick().is_err(), "the report could not be written");
    assert_eq!(w.runs().len(), 1, "the run was recorded before the failure");
    assert_eq!(w.state(&id), JobState::Finishing);

    fs::remove_dir_all(&report).unwrap();
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Stopped);
    assert_eq!(w.runs().len(), 1, "still one run");
    let done = report_of(&w, &id);
    assert_eq!(done.outcome, ReportOutcome::Stopped);
    assert_eq!(done.run.as_deref(), Some("run-1"));
    assert!(w.shared.holder().is_none(), "the lock was given back");
}

#[test]
fn the_edit_changes_exactly_one_entry_and_passes_the_scan() {
    let w = world("stop-one-entry");
    let (id, run_id, mut drain) = running(&w, true);
    put_partial(&w, &run_id, 1200);
    let before = control_of(&w, &run_id);
    assert!(before.contains("stopAt endTime;"));
    passed(&queue_stop(&w.doc, &id).report);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Stopped);
    assert_eq!(
        control_of(&w, &run_id),
        before.replacen("stopAt endTime;", "stopAt writeNow;", 1)
    );

    // A control file the safety scan would refuse is not written, and the run is not stopped by
    // the edit: it is stopped by force and ends interrupted.
    let w = world("stop-refused");
    let (id, run_id, mut drain) = running(&w, true);
    let tainted = format!("{}\nlibs (\"libother.so\");\n", control_of(&w, &run_id));
    w.shared.replace(&run_id, CONTROL, &tainted);
    passed(&queue_stop(&w.doc, &id).report);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Interrupted);
    assert_eq!(w.job(&id).code.as_deref(), Some("CFD-RUN-008"));
    assert_eq!(control_of(&w, &run_id), tainted, "nothing was written");
    assert!(w.runs().is_empty());
    assert!(w.shared.holder().is_none());
}

#[test]
fn cancel_on_a_running_job_is_the_same_graceful_stop() {
    let w = world("stop-cancel");
    let (id, run_id, mut drain) = running(&w, true);
    put_partial(&w, &run_id, 1200);
    passed(&queue_cancel(&w.doc, &id).report);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Stopped);
    assert_eq!(w.runs().len(), 1);
    assert!(control_of(&w, &run_id).contains("stopAt writeNow;"));
}

#[test]
fn a_disk_stop_is_recorded_stopped_with_the_disk_sentence() {
    let w = world("stop-disk");
    let (id, run_id, mut drain) = running(&w, true);
    put_partial(&w, &run_id, 1200);
    request(&w, &id, StopMode::WriteNow, StopCause::Disk);
    w.shared.inner.borrow_mut().set_free_kb(19 * 1_048_576);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Stopped);
    let report = report_of(&w, &id);
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("fell to 19 GB, the floor being 20 GB")),
        "{:?}",
        report.lines
    );
    assert_eq!(w.runs().len(), 1);
}

#[test]
fn a_no_write_request_is_an_edit_to_no_write_now_and_not_a_stopped_run() {
    let w = world("stop-nowrite");
    let (id, run_id, mut drain) = running(&w, true);
    w.shared.grow_log(&run_id, 1, 50);
    request(&w, &id, StopMode::NoWrite, StopCause::Diverged);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Failed);
    assert!(control_of(&w, &run_id).contains("stopAt noWriteNow;"));
    assert!(w.runs().is_empty(), "no stopped run is recorded");
}

#[test]
fn the_stop_survives_a_drain_restart() {
    let w = world("stop-restart");
    let (id, run_id, mut drain) = running(&w, false);
    put_partial(&w, &run_id, 1200);
    passed(&queue_stop(&w.doc, &id).report);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Stopping);
    drop(drain);

    // The solver follows the stop while no drain is running.
    w.shared.append(&run_id, "log.simpleFoam", "End\n");
    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 0,
            oom_killed: false,
        },
    );
    let mut again = w.drain();
    assert_eq!(settle(&w, &mut again, &id), JobState::Stopped);
    assert_eq!(w.runs().len(), 1);
}

#[test]
fn a_stop_for_a_job_that_already_ended_does_nothing() {
    // The run reached its end before the edit: it is a normal finish.
    let w = world("stop-late-end");
    let (id, run_id, mut drain) = running(&w, false);
    w.play(&run_id);
    passed(&queue_stop(&w.doc, &id).report);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Finished);
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert!(runs[0].result.is_some());
    assert!(runs[0].checkpoint.is_none());

    // The run ended short of its end before the edit: it is a stopped run.
    let w = world("stop-late-short");
    let (id, run_id, mut drain) = running(&w, false);
    put_partial(&w, &run_id, 1200);
    w.shared.append(&run_id, "log.simpleFoam", "End\n");
    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 0,
            oom_killed: false,
        },
    );
    passed(&queue_stop(&w.doc, &id).report);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Stopped);
    assert_eq!(
        w.runs()[0].checkpoint.as_ref().map(|c| c.time.as_str()),
        Some("1200")
    );
}

#[test]
fn the_partial_rules_judge_a_stopped_run() {
    let w = world("stop-trap");
    let (id, run_id, mut drain) = running(&w, true);
    put_partial(&w, &run_id, 1200);
    w.shared.append(
        &run_id,
        "log.simpleFoam",
        "FOAM FATAL ERROR: something broke\n",
    );
    passed(&queue_stop(&w.doc, &id).report);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Failed);
    let code = w.job(&id).code.expect("a failed job carries a code");
    assert!(code.starts_with("CFD-"), "{code}");
    assert!(w.runs().is_empty(), "nothing is recorded");
}

/// How many times the control file was written to the host.
fn writes(w: &World, run_id: &str) -> usize {
    let wanted = format!("write_text {run_id} {CONTROL}");
    w.shared.calls().iter().filter(|c| **c == wanted).count()
}

/// How many times the container was stopped by force.
fn forced_stops(w: &World, run_id: &str) -> usize {
    let wanted = format!("stop_container {run_id} 30");
    w.shared.calls().iter().filter(|c| **c == wanted).count()
}

/// The position of the first call that starts with `prefix`, from the end.
fn last_call(w: &World, prefix: &str) -> Option<usize> {
    w.shared.calls().iter().rposition(|c| c.starts_with(prefix))
}

/// A job told to stop on a host whose solver ignores it, after the tick that wrote the stop.
fn ignored(tag: &str) -> (World, String, String, Drain<Shared>) {
    let w = world(tag);
    let (id, run_id, mut drain) = running(&w, false);
    put_partial(&w, &run_id, 1200);
    passed(&queue_stop(&w.doc, &id).report);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Stopping);
    assert_eq!(writes(&w, &run_id), 1);
    (w, id, run_id, drain)
}

#[test]
fn an_ignored_edit_is_rewritten_twice_then_the_container_is_stopped() {
    let (w, id, run_id, mut drain) = ignored("stop-ignored");
    let mut seen = Vec::new();
    for _ in 0..12 {
        drain.tick().unwrap();
        seen.push((writes(&w, &run_id), forced_stops(&w, &run_id)));
    }
    let written: Vec<usize> = seen.iter().map(|s| s.0).collect();
    assert_eq!(written, [1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3]);
    assert_eq!(seen[10].1, 0, "not before the wait is over");
    assert_eq!(seen[11].1, 1, "at the twelfth tick");

    assert_eq!(settle(&w, &mut drain, &id), JobState::Interrupted);
    assert_eq!(w.job(&id).code.as_deref(), Some("CFD-RUN-008"));
    let report = report_of(&w, &id);
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("was interrupted: the run could not be confirmed stopped")),
        "{:?}",
        report.lines
    );
}

#[test]
fn the_lock_is_given_back_only_after_the_container_is_confirmed_gone() {
    let (w, id, run_id, mut drain) = ignored("stop-lock-order");
    assert_eq!(settle(&w, &mut drain, &id), JobState::Interrupted);
    let removed = last_call(&w, "remove_container").expect("the container was removed");
    let released = last_call(&w, "release").expect("the lock was given back");
    assert!(removed < released, "{:?}", w.shared.calls());
    assert!(w.shared.holder().is_none());
    assert_eq!(w.shared.container(&run_id), ContainerState::None);

    // A container that still answers after it was removed is not confirmed gone: the lock stays.
    let w = world("stop-lingers");
    let lingers = Rc::new(Cell::new(false));
    let host = Stubborn {
        inner: w.shared.clone(),
        lingers: lingers.clone(),
    };
    let id = w.add();
    let mut drain = Drain::new(w.config(), host);
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    put_partial(&w, &run_id, 1200);
    lingers.set(true);
    passed(&queue_stop(&w.doc, &id).report);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Interrupted);
    assert_eq!(w.job(&id).code.as_deref(), Some("CFD-RUN-008"));
    assert!(last_call(&w, "release").is_none(), "the lock is kept");
    assert!(w.shared.holder().is_some());
    let report = report_of(&w, &id);
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("may still be running")),
        "{:?}",
        report.lines
    );
}

#[test]
fn a_solver_that_obeys_late_is_not_stopped_by_force() {
    let (w, id, run_id, mut drain) = ignored("stop-late-obey");
    for _ in 0..7 {
        drain.tick().unwrap();
    }
    // The solver reads the file the next time it is written.
    w.shared.inner.borrow_mut().solver_obeys_stop(true);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Stopped);
    assert_eq!(forced_stops(&w, &run_id), 0);
    assert_eq!(w.runs().len(), 1);
}

#[test]
fn an_interrupted_run_records_nothing() {
    let (w, id, _run_id, mut drain) = ignored("stop-nothing");
    assert_eq!(settle(&w, &mut drain, &id), JobState::Interrupted);
    assert!(w.runs().is_empty());
    assert!(w.job(&id).run.is_none());
}

#[test]
fn the_wait_is_counted_in_ticks_not_seconds() {
    let w = world("stop-ticks");
    let config = DrainConfig {
        stop_wait_ticks: 3,
        ..w.config()
    };
    let id = w.add();
    let mut drain = Drain::new(config, w.shared.clone());
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    put_partial(&w, &run_id, 1200);
    passed(&queue_stop(&w.doc, &id).report);
    drain.tick().unwrap();
    assert_eq!(writes(&w, &run_id), 1);
    drain.tick().unwrap();
    drain.tick().unwrap();
    assert_eq!(writes(&w, &run_id), 3);
    assert_eq!(forced_stops(&w, &run_id), 0);
    drain.tick().unwrap();
    assert_eq!(forced_stops(&w, &run_id), 1, "after three ticks");
    assert_eq!(settle(&w, &mut drain, &id), JobState::Interrupted);
}

/// A host that answers like the shared one, except that a container which was removed can be made
/// to go on answering, as one that could not be stopped would.
struct Stubborn {
    inner: Shared,
    lingers: Rc<Cell<bool>>,
}

impl Host for Stubborn {
    fn start(&mut self, run_id: &str, case: &Path, doc: &str, np: u32) -> Result<(), HostError> {
        self.inner.start(run_id, case, doc, np)
    }

    fn inspect(&mut self, run_id: &str) -> Result<ContainerState, HostError> {
        match self.inner.inspect(run_id)? {
            ContainerState::None if self.lingers.get() => Ok(ContainerState::Exited {
                code: 137,
                oom_killed: false,
            }),
            other => Ok(other),
        }
    }

    fn holder(&mut self) -> Result<Option<Holder>, HostError> {
        Ok(self.inner.holder())
    }

    fn tail(
        &mut self,
        run_id: &str,
        rel: &str,
        offset: u64,
        max: u64,
    ) -> Result<Vec<u8>, HostError> {
        self.inner.tail(run_id, rel, offset, max)
    }

    fn files(&mut self, run_id: &str, dir: &str) -> Result<Vec<RemoteFile>, HostError> {
        self.inner.files(run_id, dir)
    }

    fn time_dirs(&mut self, run_id: &str) -> Result<Vec<TimeDir>, HostError> {
        self.inner.time_dirs(run_id)
    }

    fn free_kb(&mut self) -> Result<u64, HostError> {
        self.inner.free_kb()
    }

    fn read_text(&mut self, run_id: &str, rel: &str) -> Result<String, HostError> {
        self.inner.read_text(run_id, rel)
    }

    fn write_text(&mut self, run_id: &str, rel: &str, text: &str) -> Result<(), HostError> {
        self.inner.write_text(run_id, rel, text)
    }

    fn stop_container(&mut self, run_id: &str, grace_seconds: u32) -> Result<(), HostError> {
        self.inner.stop_container(run_id, grace_seconds)
    }

    fn download(&mut self, run_id: &str, dest: &Path) -> Result<(), HostError> {
        self.inner.download(run_id, dest)
    }

    fn remove_container(&mut self, run_id: &str) -> Result<(), HostError> {
        self.inner.remove_container(run_id)
    }

    fn release(&mut self, run_id: &str) -> Result<ReleaseOutcome, HostError> {
        self.inner.release(run_id)
    }
}
