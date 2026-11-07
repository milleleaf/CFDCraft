//! The run folder on the host is removed after the results of a run are recorded and the report
//! is written, only when the drain was set up to do it, and never for a run that was not recorded
//! or that this drain did not finish.

use std::fs;
use std::path::Path;

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_drain::finalize::{JobReport, ReportOutcome};
use cfdcraft_drain::{Drain, DrainConfig};
use cfdcraft_ops::queue_ops::queue_stop;
use cfdcraft_queue::JobState;
use cfdcraft_remote::{
    ContainerState, Holder, Host, HostError, ReleaseOutcome, RemoteFile, TimeDir,
};

mod support;
use support::*;

/// A file the upload put into the run folder.
const PROBE: &str = "Allrun";

fn cleaning(w: &World) -> Drain<Shared> {
    let config = DrainConfig {
        clean_run_dirs: true,
        ..w.config()
    };
    Drain::new(config, w.shared.clone())
}

fn present(w: &World, run_id: &str) -> bool {
    w.shared.file(run_id, PROBE).is_some()
}

fn removals(w: &World) -> usize {
    w.shared
        .calls()
        .iter()
        .filter(|c| c.starts_with("remove_run_dir"))
        .count()
}

fn report_of(w: &World, id: &str) -> JobReport {
    let text = fs::read_to_string(w.live(id).join("report.json")).expect("a report was written");
    serde_json::from_str(&text).expect("the report reads")
}

/// A job that has started, on a drain that cleans.
fn started(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = cleaning(&w);
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    assert!(present(&w, &run_id));
    (w, drain, id, run_id)
}

/// The position of the first call that starts with `what`.
fn at(calls: &[String], what: &str) -> usize {
    calls
        .iter()
        .position(|c| c.starts_with(what))
        .unwrap_or_else(|| panic!("no call {what}: {calls:?}"))
}

#[test]
fn the_drain_removes_the_folder_after_the_run_is_recorded() {
    let (w, mut drain, id, run_id) = started("clean-done");
    // A folder of another run on the same box.
    w.shared
        .append("cfdcraft-other-1-1", "log.simpleFoam", "other\n");
    w.play(&run_id);
    drain.tick().unwrap();
    drain.tick().unwrap();

    assert_eq!(w.state(&id), JobState::Finished);
    assert_eq!(w.runs().len(), 1, "the run is recorded");
    assert!(w.live(&id).join("results").is_dir(), "the results are kept");
    assert!(w.live(&id).join("log.simpleFoam").is_file());
    assert!(!present(&w, &run_id), "the folder on the host is gone");
    assert!(w.shared.file(&run_id, "log.simpleFoam").is_none());
    assert!(
        w.shared
            .file("cfdcraft-other-1-1", "log.simpleFoam")
            .is_some(),
        "another run's folder stays"
    );
    assert_eq!(removals(&w), 1);

    let calls = w.shared.calls();
    let order = [
        at(&calls, "download"),
        at(&calls, "release"),
        at(&calls, "remove_container"),
        at(&calls, "remove_run_dir"),
    ];
    assert!(order.is_sorted(), "{order:?} in {calls:?}");
    assert_eq!(report_of(&w, &id).outcome, ReportOutcome::Finished);
}

#[test]
fn with_the_switch_off_nothing_is_removed() {
    let w = world("clean-off");
    let id = w.add();
    let mut drain = w.drain();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    w.play(&run_id);
    drain.tick().unwrap();
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finished);
    assert!(present(&w, &run_id));
    assert_eq!(removals(&w), 0, "{:?}", w.shared.calls());
}

#[test]
fn a_failed_run_is_cleaned_after_its_logs_are_in_the_mirror() {
    let (w, mut drain, id, run_id) = started("clean-failed");
    w.shared.append(
        &run_id,
        "log.simpleFoam",
        "--> FOAM FATAL IO ERROR:\nCould not open \"/w/system/fvSchemes\"\n\nFOAM exiting\n",
    );
    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 1,
            oom_killed: false,
        },
    );
    drain.tick().unwrap();
    drain.tick().unwrap();

    assert_eq!(w.state(&id), JobState::Failed);
    assert!(w.runs().is_empty());
    let mirrored = fs::read_to_string(w.live(&id).join("log.simpleFoam")).unwrap();
    assert!(mirrored.contains("FOAM FATAL IO ERROR"), "{mirrored}");
    assert!(!present(&w, &run_id));
    assert_eq!(removals(&w), 1);
}

/// What a solver that is cut short at `time` leaves in the run.
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

#[test]
fn a_stopped_run_is_cleaned_only_after_the_checkpoint_is_recorded() {
    let (w, mut drain, id, run_id) = started("clean-stop");
    w.shared.inner.borrow_mut().solver_obeys_stop(true);
    put_partial(&w, &run_id, 1200);
    passed(&queue_stop(&w.doc, &id).report);
    for _ in 0..40 {
        drain.tick().unwrap();
        if w.state(&id).is_terminal() {
            break;
        }
        assert!(present(&w, &run_id), "nothing is removed while it ends");
    }
    assert_eq!(w.state(&id), JobState::Stopped);
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0].checkpoint.as_ref().map(|c| c.time.as_str()),
        Some("1200")
    );
    assert!(!present(&w, &run_id));
    assert_eq!(removals(&w), 1);
    let calls = w.shared.calls();
    assert!(at(&calls, "download") < at(&calls, "remove_run_dir"));
}

#[test]
fn nothing_is_removed_when_the_record_failed() {
    // The document changed while the case ran.
    let (w, mut drain, id, run_id) = started("clean-changed");
    passed(&cfdcraft_ops::save_document(&with_lid_speed(0.2), &w.doc, true).report);
    w.play(&run_id);
    drain.tick().unwrap();
    drain.tick().unwrap();
    assert_eq!(w.job(&id).code.as_deref(), Some("CFD-REPRO-009"));
    assert!(present(&w, &run_id), "the folder stays");
    assert_eq!(removals(&w), 0);

    // The files do not show a finished run.
    let (w, mut drain, id, run_id) = started("clean-evidence");
    w.play(&run_id);
    w.shared
        .append(&run_id, "log.simpleFoam", "--> FOAM FATAL ERROR:\nbroken\n");
    drain.tick().unwrap();
    drain.tick().unwrap();
    assert_eq!(w.job(&id).code.as_deref(), Some("CFD-SOLVER-099"));
    assert!(present(&w, &run_id));
    assert_eq!(removals(&w), 0);
}

#[test]
fn nothing_is_removed_when_the_download_failed() {
    let (w, mut drain, id, run_id) = started("clean-download");
    w.play(&run_id);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finishing);
    // The next calls do not get through: the download is the first one that matters.
    w.shared.inner.borrow_mut().fail_next(100);
    let _ = drain.tick();
    assert_eq!(w.state(&id), JobState::Finishing);
    assert!(present(&w, &run_id));
    assert_eq!(removals(&w), 0, "{:?}", w.shared.calls());
    assert!(w.runs().is_empty());
}

#[test]
fn a_cut_short_finish_removes_the_folder_on_the_next_try_and_not_before() {
    let (w, mut drain, id, run_id) = started("clean-retry");
    w.play(&run_id);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finishing);

    // The report cannot be written: the run is recorded, but the folder must stay.
    let report = w.live(&id).join("report.json");
    fs::create_dir_all(&report).unwrap();
    assert!(drain.tick().is_err());
    assert_eq!(w.runs().len(), 1);
    assert!(
        present(&w, &run_id),
        "no removal before the report is written"
    );
    assert_eq!(removals(&w), 0);

    fs::remove_dir_all(&report).unwrap();
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finished);
    assert_eq!(w.runs().len(), 1, "still one run");
    assert!(!present(&w, &run_id));
    assert_eq!(removals(&w), 1);
}

#[test]
fn a_run_that_was_not_started_by_this_drain_is_never_cleaned() {
    // Inconsistent: the container is ours and running, but nobody holds the lock.
    let (w, drain, id, run_id) = started("clean-inconsistent");
    drop(drain);
    w.shared.inner.borrow_mut().set_holder(None);
    let mut drain = cleaning(&w);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Interrupted);
    assert!(present(&w, &run_id));

    // Interrupted: neither lock nor container is left.
    let (w, drain, id, run_id) = started("clean-interrupted");
    drop(drain);
    {
        let mut host = w.shared.inner.borrow_mut();
        host.set_holder(None);
        host.drop_container(&run_id);
    }
    let mut drain = cleaning(&w);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Interrupted);
    assert!(present(&w, &run_id));
    assert_eq!(removals(&w), 0, "{:?}", w.shared.calls());
}

/// A host that does everything the box does but refuses to remove a run folder.
#[derive(Clone)]
struct KeepsFolders(Shared);

impl Host for KeepsFolders {
    fn start(&mut self, run_id: &str, case: &Path, doc: &str, np: u32) -> Result<(), HostError> {
        self.0.start(run_id, case, doc, np)
    }
    fn inspect(&mut self, run_id: &str) -> Result<ContainerState, HostError> {
        self.0.inspect(run_id)
    }
    fn holder(&mut self) -> Result<Option<Holder>, HostError> {
        Host::holder(&mut self.0)
    }
    fn tail(
        &mut self,
        run_id: &str,
        rel: &str,
        offset: u64,
        max: u64,
    ) -> Result<Vec<u8>, HostError> {
        self.0.tail(run_id, rel, offset, max)
    }
    fn files(&mut self, run_id: &str, dir: &str) -> Result<Vec<RemoteFile>, HostError> {
        self.0.files(run_id, dir)
    }
    fn time_dirs(&mut self, run_id: &str) -> Result<Vec<TimeDir>, HostError> {
        self.0.time_dirs(run_id)
    }
    fn free_kb(&mut self) -> Result<u64, HostError> {
        self.0.free_kb()
    }
    fn read_text(&mut self, run_id: &str, rel: &str) -> Result<String, HostError> {
        self.0.read_text(run_id, rel)
    }
    fn write_text(&mut self, run_id: &str, rel: &str, text: &str) -> Result<(), HostError> {
        self.0.write_text(run_id, rel, text)
    }
    fn stop_container(&mut self, run_id: &str, grace_seconds: u32) -> Result<(), HostError> {
        self.0.stop_container(run_id, grace_seconds)
    }
    fn download(&mut self, run_id: &str, dest: &Path) -> Result<(), HostError> {
        self.0.download(run_id, dest)
    }
    fn remove_container(&mut self, run_id: &str) -> Result<(), HostError> {
        self.0.remove_container(run_id)
    }
    fn remove_run_dir(&mut self, _run_id: &str) -> Result<(), HostError> {
        Err(HostError::Unreachable)
    }
    fn release(&mut self, run_id: &str) -> Result<ReleaseOutcome, HostError> {
        self.0.release(run_id)
    }
}

#[test]
fn a_removal_that_fails_is_a_note_in_the_report_and_never_a_failed_job() {
    let w = world("clean-note");
    let id = w.add();
    let config = DrainConfig {
        clean_run_dirs: true,
        ..w.config()
    };
    let mut drain = Drain::new(config, KeepsFolders(w.shared.clone()));
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    w.play(&run_id);
    drain.tick().unwrap();
    drain.tick().unwrap();

    assert_eq!(w.state(&id), JobState::Finished);
    assert_eq!(w.runs().len(), 1, "recorded once");
    assert!(present(&w, &run_id));
    let report = report_of(&w, &id);
    assert_eq!(report.outcome, ReportOutcome::Finished);
    assert_eq!(report.run.as_deref(), Some("run-1"));
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("could not be removed")),
        "{:?}",
        report.lines
    );
    let text = fs::read_to_string(w.live(&id).join("report.json")).unwrap();
    assert!(!text.contains("cfdcraft-cavity"), "no run name: {text}");
}
