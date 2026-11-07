//! The world the drain tests run in: a saved cavity document in a scratch folder, an in-memory
//! compute host that a test shares with the drain, and helpers that play the part of the solver.
//! Nothing here needs a compute host, a container engine or a clock.
#![allow(dead_code)]

use std::cell::RefCell;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_drain::{Drain, DrainConfig};
use cfdcraft_ops::queue_file::QueueFile;
use cfdcraft_ops::queue_ops::{AddRequest, queue_add};
use cfdcraft_ops::{ExitClass, Report, save_document};
use cfdcraft_queue::{Job, JobState, Queue};
use cfdcraft_remote::fake::FakeBox;
use cfdcraft_remote::{
    ContainerState, Holder, Host, HostError, ReleaseOutcome, RemoteFile, TimeDir,
};
use cfdcraft_runs::RunRecord;
use serde_json::Value;

/// The cavity study, at the schema the files use now.
pub const CAVITY: &str = include_str!("../../../cfdcraft-model/tests/fixtures/cavity_v1.json");

/// A fresh, empty folder under the build folder, named by the process and the tag.
pub fn scratch(tag: &str) -> PathBuf {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("drain-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn passed(report: &Report) {
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
}

/// A new document called `name` in `dir`, made from the study text.
pub fn new_document(dir: &Path, name: &str, study_json: &str) -> PathBuf {
    let path = dir.join(format!("{name}.cfdcraft"));
    passed(&save_document(study_json, &path, false).report);
    path
}

/// The study text with one change made to it as JSON.
pub fn edit(study_json: &str, change: impl FnOnce(&mut Value)) -> String {
    let mut study: Value = serde_json::from_str(study_json).unwrap();
    change(&mut study);
    study.to_string()
}

/// The cavity with another speed for the lid.
pub fn with_lid_speed(speed: f64) -> String {
    edit(CAVITY, |study| {
        study["boundary_conditions"]["lid"]["velocity"]["value"] = Value::from(speed);
    })
}

type Hook = Rc<RefCell<Option<Box<dyn FnMut(&str)>>>>;

fn fire(hook: &Hook, run_id: &str) {
    if let Some(f) = hook.borrow_mut().as_mut() {
        f(run_id);
    }
}

/// An in-memory compute host that the drain and the test both hold. A function can be set to run
/// when a start is made: before the host is touched, or after the container exists.
#[derive(Clone)]
pub struct Shared {
    pub inner: Rc<RefCell<FakeBox>>,
    before_start: Hook,
    after_start: Hook,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(FakeBox::new())),
            before_start: Rc::new(RefCell::new(None)),
            after_start: Rc::new(RefCell::new(None)),
        }
    }

    pub fn on_before_start(&self, f: impl FnMut(&str) + 'static) {
        *self.before_start.borrow_mut() = Some(Box::new(f));
    }

    pub fn on_after_start(&self, f: impl FnMut(&str) + 'static) {
        *self.after_start.borrow_mut() = Some(Box::new(f));
    }

    /// Every call the host has seen, in order.
    pub fn calls(&self) -> Vec<String> {
        self.inner.borrow().calls().to_vec()
    }

    pub fn file(&self, run_id: &str, rel: &str) -> Option<Vec<u8>> {
        self.inner.borrow().file(run_id, rel).map(<[u8]>::to_vec)
    }

    /// Adds text to the end of a file of the run: the solver writing.
    pub fn append(&self, run_id: &str, rel: &str, text: &str) {
        self.inner.borrow_mut().append(run_id, rel, text);
    }

    /// Replaces a file of the run.
    pub fn replace(&self, run_id: &str, rel: &str, text: &str) {
        self.inner
            .borrow_mut()
            .write_text(run_id, rel, text)
            .unwrap();
    }

    pub fn set_container(&self, run_id: &str, state: ContainerState) {
        self.inner.borrow_mut().set_container(run_id, state);
    }

    pub fn container(&self, run_id: &str) -> ContainerState {
        self.inner.borrow_mut().inspect(run_id).unwrap()
    }

    pub fn holder(&self) -> Option<Holder> {
        self.inner.borrow_mut().holder().unwrap()
    }

    /// The solver runs to its end: the files a good run of the cavity leaves are put into the
    /// run, and the container exits. `scratch` is where the files are made first.
    pub fn play_solver(&self, scratch: &Path, run_id: &str) {
        let dir = scratch.join("played");
        let _ = fs::remove_dir_all(&dir);
        let spec = FixtureSpec {
            end_time: 5000,
            cells: 16384,
            samples: 64,
            shift: 0.0,
        };
        write_results(&dir, &spec).unwrap();
        write_monitor_files(&dir);
        for rel in files_under(&dir) {
            let text = fs::read_to_string(dir.join(&rel)).unwrap();
            self.replace(run_id, &rel, &text);
        }
        self.set_container(
            run_id,
            ContainerState::Exited {
                code: 0,
                oom_killed: false,
            },
        );
    }

    /// Adds iterations `from..=to` of a healthy log to the solver log of the run.
    pub fn grow_log(&self, run_id: &str, from: u64, to: u64) {
        self.append(run_id, "log.simpleFoam", &cavity_log(from, to));
    }

    /// Adds steps `from..=to` to the five velocity probes of the run.
    pub fn grow_probes(&self, run_id: &str, from: u64, to: u64) {
        for index in 0..5_i32 {
            let rel = format!("postProcessing/probe_{}/0/U", index + 1);
            let mut text = String::new();
            if self.file(run_id, &rel).is_none() {
                text.push_str("# Probe 0 (0.05 0.05 0.025)\n#       Probe 0\n#          Time\n");
            }
            for step in from..=to {
                let t = step as f64;
                let ux = 0.01 * f64::from(index + 1) + 1e-5 / t;
                let uy = -0.02 * f64::from(index + 1) + 1e-5 / t;
                let _ = writeln!(text, "{step} ({ux:.12} {uy:.12} 0)");
            }
            self.append(run_id, &rel, &text);
        }
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for Shared {
    fn start(&mut self, run_id: &str, case: &Path, doc: &str, np: u32) -> Result<(), HostError> {
        fire(&self.before_start, run_id);
        let result = self.inner.borrow_mut().start(run_id, case, doc, np);
        if result.is_ok() {
            fire(&self.after_start, run_id);
        }
        result
    }

    fn inspect(&mut self, run_id: &str) -> Result<ContainerState, HostError> {
        self.inner.borrow_mut().inspect(run_id)
    }

    fn holder(&mut self) -> Result<Option<Holder>, HostError> {
        self.inner.borrow_mut().holder()
    }

    fn tail(
        &mut self,
        run_id: &str,
        rel: &str,
        offset: u64,
        max: u64,
    ) -> Result<Vec<u8>, HostError> {
        self.inner.borrow_mut().tail(run_id, rel, offset, max)
    }

    fn files(&mut self, run_id: &str, dir: &str) -> Result<Vec<RemoteFile>, HostError> {
        self.inner.borrow_mut().files(run_id, dir)
    }

    fn time_dirs(&mut self, run_id: &str) -> Result<Vec<TimeDir>, HostError> {
        self.inner.borrow_mut().time_dirs(run_id)
    }

    fn free_kb(&mut self) -> Result<u64, HostError> {
        self.inner.borrow_mut().free_kb()
    }

    fn read_text(&mut self, run_id: &str, rel: &str) -> Result<String, HostError> {
        self.inner.borrow_mut().read_text(run_id, rel)
    }

    fn write_text(&mut self, run_id: &str, rel: &str, text: &str) -> Result<(), HostError> {
        self.inner.borrow_mut().write_text(run_id, rel, text)
    }

    fn stop_container(&mut self, run_id: &str, grace_seconds: u32) -> Result<(), HostError> {
        self.inner
            .borrow_mut()
            .stop_container(run_id, grace_seconds)
    }

    fn download(&mut self, run_id: &str, dest: &Path) -> Result<(), HostError> {
        self.inner.borrow_mut().download(run_id, dest)
    }

    fn remove_container(&mut self, run_id: &str) -> Result<(), HostError> {
        self.inner.borrow_mut().remove_container(run_id)
    }

    fn remove_run_dir(&mut self, run_id: &str) -> Result<(), HostError> {
        self.inner.borrow_mut().remove_run_dir(run_id)
    }

    fn release(&mut self, run_id: &str) -> Result<ReleaseOutcome, HostError> {
        self.inner.borrow_mut().release(run_id)
    }
}

/// Iterations `from..=to` of a healthy cavity log; the residuals fall by 1 % each step.
pub fn cavity_log(from: u64, to: u64) -> String {
    let mut text = String::new();
    for i in from..=to {
        let r = 1e-2 * 0.99_f64.powi(i32::try_from(i).unwrap_or(i32::MAX));
        let _ = writeln!(text, "Time = {i}\n");
        for (solver, field) in [
            ("smoothSolver", "Ux"),
            ("smoothSolver", "Uy"),
            ("DICPCG", "p"),
        ] {
            let _ = writeln!(
                text,
                "{solver}:  Solving for {field}, Initial residual = {r:.12e}, Final residual = {:.12e}, No Iterations 2",
                r * 1e-3
            );
        }
        text.push('\n');
    }
    text
}

/// The flux files of the cavity's two patches and the files of its five default probes, with
/// values that level off. Numbers are written with twelve decimals.
pub fn write_monitor_files(dir: &Path) {
    for index in 0..5_i32 {
        let mut text = "# Probe 0 (0.05 0.05 0.025)\n#       Probe 0\n#          Time\n".to_owned();
        for step in 1..=300_i32 {
            let t = f64::from(step);
            let ux = 0.01 * f64::from(index + 1) + 1e-5 / t;
            let uy = -0.02 * f64::from(index + 1) + 1e-5 / t;
            let _ = writeln!(text, "{step} ({ux:.12} {uy:.12} 0)");
        }
        let folder = dir.join(format!("postProcessing/probe_{}/0", index + 1));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("U"), text).unwrap();
    }
    for patch in ["movingWall", "fixedWalls"] {
        let text = format!(
            "# Region type : patch {patch}\n# Faces       : 128\n# Area        : 0.005\n# Time        sum(phi)\n1 0.000000000000\n2 0.000000000000\n"
        );
        let folder = dir.join(format!("postProcessing/balance_{patch}/0"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("surfaceFieldValue.dat"), text).unwrap();
    }
}

/// Every plain file below `root`, as `/`-separated names relative to it, sorted.
pub fn files_under(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                let rel = entry.path().strip_prefix(root).unwrap().to_owned();
                let parts: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.push(parts.join("/"));
            }
        }
    }
    found.sort();
    found
}

/// A saved cavity document, an empty shared host, and the helpers a test needs.
pub struct World {
    pub dir: PathBuf,
    pub doc: PathBuf,
    pub shared: Shared,
}

pub fn world(tag: &str) -> World {
    let dir = scratch(tag);
    let doc = new_document(&dir, "cavity", CAVITY);
    World {
        dir,
        doc,
        shared: Shared::new(),
    }
}

impl World {
    /// The set-up of the drain: no pause between ticks.
    pub fn config(&self) -> DrainConfig {
        DrainConfig {
            poll: Duration::ZERO,
            ..DrainConfig::new(self.doc.clone(), "test-host")
        }
    }

    pub fn drain(&self) -> Drain<Shared> {
        Drain::new(self.config(), self.shared.clone())
    }

    pub fn file(&self) -> QueueFile {
        QueueFile::new(&self.doc)
    }

    /// Adds the study to the queue and returns the id of the job made.
    pub fn add(&self) -> String {
        passed(&queue_add(&self.doc, &AddRequest::default()).report);
        self.queue().jobs.last().unwrap().id.clone()
    }

    pub fn queue(&self) -> Queue {
        self.file().read().unwrap()
    }

    pub fn job(&self, id: &str) -> Job {
        self.queue()
            .jobs
            .into_iter()
            .find(|j| j.id == id)
            .unwrap_or_else(|| panic!("no job {id}"))
    }

    pub fn state(&self, id: &str) -> JobState {
        self.job(id).state
    }

    pub fn run_id(&self, id: &str) -> String {
        self.job(id).run_id.expect("the job has a run name")
    }

    pub fn live(&self, id: &str) -> PathBuf {
        self.file().live_dir(id).unwrap()
    }

    /// Where the played solver's files are made.
    pub fn solver_dir(&self) -> PathBuf {
        self.dir.join("solver")
    }

    pub fn play(&self, run_id: &str) {
        self.shared.play_solver(&self.solver_dir(), run_id);
    }

    pub fn runs(&self) -> Vec<RunRecord> {
        cfdcraft_doc::open(&self.doc)
            .unwrap()
            .document
            .artifacts
            .runs
    }
}
