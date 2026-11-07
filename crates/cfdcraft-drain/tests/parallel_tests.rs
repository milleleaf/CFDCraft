//! A case is written for as many processes as its cells justify and the host allows, and no more.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use cfdcraft_backend_esi::export::{allrun_script, allrun_script_for};
use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_drain::finalize::JobReport;
use cfdcraft_drain::{Drain, DrainConfig};
use cfdcraft_ops::queue_ops::{AddRequest, queue_add, queue_stop};
use cfdcraft_queue::JobState;
use cfdcraft_remote::{
    ContainerState, Holder, Host, HostError, ReleaseOutcome, RemoteFile, TimeDir,
};
use serde_json::Value;

mod support;
use support::*;

/// The cells of the large cavity: four processes' worth at 20 000 cells each, so a cap decides.
const BIG_CELLS: u64 = 90_000;

/// A document of the cavity with 300 x 300 cells, and a host to run it on.
fn big_world(tag: &str) -> World {
    let study = edit(CAVITY, |s| {
        s["mesh"]["cells_x"] = Value::from(300);
        s["mesh"]["cells_y"] = Value::from(300);
    });
    let dir = scratch(tag);
    let doc = new_document(&dir, "cavity", &study);
    World {
        dir,
        doc,
        shared: Shared::new(),
    }
}

/// The processes every start asked for, in order.
type Starts = Rc<RefCell<Vec<u32>>>;

/// A drain over the shared host that notes the process count of each start.
fn spied(w: &World, np_cap: u32) -> (Drain<Spy>, Starts) {
    let starts: Starts = Rc::new(RefCell::new(Vec::new()));
    let spy = Spy {
        inner: w.shared.clone(),
        starts: starts.clone(),
    };
    let config = DrainConfig {
        np_cap,
        ..w.config()
    };
    (Drain::new(config, spy), starts)
}

fn text_of(w: &World, run_id: &str, rel: &str) -> String {
    String::from_utf8(
        w.shared
            .file(run_id, rel)
            .unwrap_or_else(|| panic!("no {rel}")),
    )
    .unwrap()
}

/// The solver goes to its end on a mesh of `cells`: the files a good run leaves are put into the
/// run and the container exits. A parallel run's log has one more line after `End`.
fn play_cells(w: &World, run_id: &str, cells: u64, trailer: bool) {
    let dir = w.dir.join("played-cells");
    let _ = fs::remove_dir_all(&dir);
    let spec = FixtureSpec {
        end_time: 5000,
        cells,
        samples: 64,
        shift: 0.0,
    };
    write_results(&dir, &spec).unwrap();
    write_monitor_files(&dir);
    for rel in files_under(&dir) {
        let mut text = fs::read_to_string(dir.join(&rel)).unwrap();
        if trailer && rel == "log.simpleFoam" {
            text.push_str("Finalising parallel run\n");
        }
        w.shared.replace(run_id, &rel, &text);
    }
    w.shared.set_container(
        run_id,
        ContainerState::Exited {
            code: 0,
            oom_killed: false,
        },
    );
}

/// What a solver that is cut short at `time` has left in the run, on a mesh of `cells`. The solver
/// log does not end yet.
fn put_partial(w: &World, run_id: &str, time: u64, cells: u64) {
    let dir = w.dir.join("partial");
    let _ = fs::remove_dir_all(&dir);
    let spec = FixtureSpec {
        end_time: time,
        cells,
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

fn settle<H: Host>(w: &World, drain: &mut Drain<H>, id: &str) -> JobState {
    for _ in 0..8 {
        drain.tick().unwrap();
        if w.state(id).is_terminal() {
            break;
        }
    }
    w.state(id)
}

#[test]
fn a_small_mesh_is_always_serial() {
    let w = world("par-small");
    let (mut drain, starts) = spied(&w, 2);
    let id = w.add();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    assert_eq!(*starts.borrow(), [1]);
    assert!(w.shared.file(&run_id, "system/decomposeParDict").is_none());
    assert_eq!(text_of(&w, &run_id, "Allrun"), allrun_script());

    // The same case with the core cap at its default is the same case.
    let other = world("par-small-default");
    let id = other.add();
    other.drain().tick().unwrap();
    let other_run = other.run_id(&id);
    assert_eq!(
        text_of(&other, &other_run, "system/controlDict"),
        text_of(&w, &run_id, "system/controlDict")
    );
    assert_eq!(
        text_of(&other, &other_run, "Allrun"),
        text_of(&w, &run_id, "Allrun")
    );
}

#[test]
fn a_large_mesh_decomposes_within_the_cap() {
    let w = big_world("par-large");
    let (mut drain, starts) = spied(&w, 2);
    let id = w.add();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    assert_eq!(*starts.borrow(), [2]);
    assert!(w.shared.file(&run_id, "system/decomposeParDict").is_some());
    let script = text_of(&w, &run_id, "Allrun");
    assert!(script.contains("mpirun -np 2"), "{script}");
    assert_eq!(script, allrun_script_for(2));

    play_cells(&w, &run_id, BIG_CELLS, false);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Finished);
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].np, Some(2));
    let text = fs::read_to_string(w.live(&id).join("report.json")).unwrap();
    let report: JobReport = serde_json::from_str(&text).unwrap();
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("Running on 2 processes (90000 cells).")),
        "{:?}",
        report.lines
    );
}

#[test]
fn the_cap_is_a_ceiling() {
    let w = big_world("par-cap");
    let (mut drain, starts) = spied(&w, 1);
    let id = w.add();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    assert_eq!(*starts.borrow(), [1]);
    assert!(w.shared.file(&run_id, "system/decomposeParDict").is_none());
    assert_eq!(text_of(&w, &run_id, "Allrun"), allrun_script());
}

#[test]
fn a_parallel_log_with_the_trailer_is_judged_finished() {
    let w = big_world("par-trailer");
    let (mut drain, _starts) = spied(&w, 2);
    let id = w.add();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    play_cells(&w, &run_id, BIG_CELLS, true);
    let log = text_of(&w, &run_id, "log.simpleFoam");
    assert!(log.trim_end().ends_with("Finalising parallel run"));
    assert_eq!(settle(&w, &mut drain, &id), JobState::Finished);
    assert_eq!(w.runs().len(), 1);
}

#[test]
fn a_continue_of_a_parallel_run_keeps_working() {
    let w = big_world("par-continue");
    let (mut drain, starts) = spied(&w, 2);
    let id = w.add();
    drain.tick().unwrap();
    w.shared.inner.borrow_mut().solver_obeys_stop(true);
    let run_id = w.run_id(&id);
    put_partial(&w, &run_id, 1200, BIG_CELLS);
    passed(&queue_stop(&w.doc, &id).report);
    assert_eq!(settle(&w, &mut drain, &id), JobState::Stopped);
    assert_eq!(w.runs()[0].np, Some(2));

    let request = AddRequest {
        continue_run: Some("run-1".to_owned()),
        ..AddRequest::default()
    };
    passed(&queue_add(&w.doc, &request).report);
    let next = w.queue().jobs.last().unwrap().id.clone();
    drain.tick().unwrap();
    assert_eq!(w.state(&next), JobState::Running);
    assert_eq!(*starts.borrow(), [2, 2]);
    let again = w.run_id(&next);
    assert!(w.shared.file(&again, "system/decomposeParDict").is_some());
    assert!(w.shared.file(&again, "1200/U").is_some());
    assert!(w.shared.file(&again, "1200/p").is_some());
    let control = text_of(&w, &again, "system/controlDict");
    assert!(control.contains("startFrom latestTime;"), "{control}");
    assert!(text_of(&w, &again, "Allrun").contains("mpirun -np 2"));
}

/// The shared host, noting the process count of every start.
struct Spy {
    inner: Shared,
    starts: Starts,
}

impl Host for Spy {
    fn start(&mut self, run_id: &str, case: &Path, doc: &str, np: u32) -> Result<(), HostError> {
        self.starts.borrow_mut().push(np);
        self.inner.start(run_id, case, doc, np)
    }

    fn inspect(&mut self, run_id: &str) -> Result<ContainerState, HostError> {
        self.inner.inspect(run_id)
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
