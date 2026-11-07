//! One step of the loop: read the queue, then advance the job that is active or start the next
//! one that is queued.
//!
//! Every state change is written to `queue.json` before the next step is taken, and the run name
//! is written before the host is touched, so a process killed at any point leaves a file a new
//! process can carry on from. The loop is generic over [`Host`]; the in-memory box and the real
//! host run the same code.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cfdcraft_ops::DocOutcome;
use cfdcraft_ops::convergence::assess_dir;
use cfdcraft_ops::jobs::{DocJob, plan_scenario_job_with};
use cfdcraft_ops::queue_file::QueueFile;
use cfdcraft_queue::{
    Action, Job, JobState, MAX_ATTEMPTS, Outcome, Progress, Queue, active, next_to_start,
};
use cfdcraft_remote::{ContainerState, Host, HostError, valid_rel};
use cfdcraft_settle::Status;
use cfdcraft_text::{Line, Text};

use crate::disk::DiskRefusal;
use crate::finalize::first_error_code;
use crate::mirror::{Mirror, MirrorError};
use crate::watch::{LiveSpec, solver_log_of};
use crate::{DrainConfig, DrainError, case, disk, explain, finalize, reattach, stop, watchdog};

/// The file in the live folder that holds what the verdict is made from.
pub const SPEC: &str = "spec.json";
/// The file in the live folder that holds the study the case was made from.
pub const STUDY: &str = "study.json";
/// How many ticks in a row the host may stay silent before the job is called unreachable.
pub const MISS_LIMIT: u32 = 6;
/// The most bytes fetched from one file in one tick.
pub const FETCH_CAP: u64 = 1024 * 1024;
/// The log files mirrored, in the order they are written; `solver_log` is the job's own.
fn logs_of(solver_log: &str) -> [String; 3] {
    [
        "log.blockMesh".to_owned(),
        "log.checkMesh".to_owned(),
        solver_log.to_owned(),
    ]
}
/// The folder of the host that holds the monitor files.
const POST: &str = "postProcessing";

/// What a tick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOutcome {
    /// There was nothing to do: no job is active and none is waiting.
    Idle,
    /// A job was started or advanced, or is being waited for.
    Advanced,
}

/// What the drain keeps in memory between ticks. Nothing here is needed to carry on after a
/// restart: the queue file and the mirror hold everything that matters.
#[derive(Debug, Clone, Default)]
pub struct Scratch {
    /// The ticks made so far by this process.
    pub tick_no: u64,
    /// Whether the start-up step has run.
    pub started: bool,
    /// The ticks in a row in which the host did not answer.
    pub misses: u32,
    /// The processes the running job was started with.
    pub np: Option<u32>,
    /// Whether the container was killed for lack of memory, as the last inspection said.
    pub oom_killed: bool,
    /// The exit status of the container, as the last inspection said once it had ended.
    pub exit_code: Option<i32>,
    /// Whether the stop being carried out was asked for because the disk is nearly full.
    pub disk_stop: bool,
    /// The tick at which a stop was written to the case.
    pub stop_applied_tick: Option<u64>,
    /// How many times the stop was written again.
    pub rewrites: u32,
    /// No queued job is started before this tick: the compute host was busy at the last try.
    pub start_after: u64,
}

/// The drain of one document over one host.
pub struct Drain<H: Host> {
    pub(crate) config: DrainConfig,
    pub(crate) host: H,
    pub(crate) queue: QueueFile,
    pub(crate) text: Text,
    pub(crate) scratch: Scratch,
}

/// The run name of a job that has one.
pub fn run_id_of(job: &Job) -> Result<&str, DrainError> {
    job.run_id.as_deref().ok_or_else(|| DrainError::Io {
        reason: "the job has no run name".to_owned(),
    })
}

/// What the document is called to the host: its file name without the extension, with every
/// character that is not a letter, digit or hyphen turned into a hyphen, at most 40 characters.
pub fn doc_label(doc: &Path) -> String {
    let stem = doc
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut label: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(40)
        .collect();
    if label.is_empty() {
        label.push_str("doc");
    }
    label
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn io_reason(reason: &str) -> DrainError {
    DrainError::Io {
        reason: reason.to_owned(),
    }
}

impl<H: Host> Drain<H> {
    /// A drain over the document and host named by `config` and `host`.
    pub fn new(config: DrainConfig, host: H) -> Self {
        let queue = QueueFile::new(&config.doc);
        Self {
            config,
            host,
            queue,
            text: Text::english(),
            scratch: Scratch::default(),
        }
    }

    pub fn config(&self) -> &DrainConfig {
        &self.config
    }

    pub fn host(&self) -> &H {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut H {
        &mut self.host
    }

    pub fn scratch(&self) -> &Scratch {
        &self.scratch
    }

    pub fn scratch_mut(&mut self) -> &mut Scratch {
        &mut self.scratch
    }

    pub fn queue_file(&self) -> &QueueFile {
        &self.queue
    }

    pub fn text(&self) -> &Text {
        &self.text
    }

    /// Writes one move to the queue file and returns the queue as it is then.
    pub fn persist(&self, action: Action) -> Result<Queue, DrainError> {
        Ok(self.queue.update(|_| Ok(vec![action]))?)
    }

    /// The live folder of a job.
    pub fn live_dir(&self, job: &Job) -> Result<PathBuf, DrainError> {
        self.queue
            .live_dir(&job.id)
            .ok_or_else(|| io_reason("the job id names no folder"))
    }

    /// The name of the job's solver log, from its live specification.
    pub(crate) fn solver_log(&self, job: &Job) -> Result<String, DrainError> {
        Ok(solver_log_of(&self.live_dir(job)?))
    }

    /// The mirror of a job.
    pub fn mirror(&self, job: &Job) -> Result<Mirror, DrainError> {
        Ok(Mirror::open(&self.live_dir(job)?))
    }

    /// What the document is called to the host.
    pub fn label(&self) -> String {
        doc_label(&self.config.doc)
    }

    /// Does one step. A host that does not answer is a missed tick, not an error.
    pub fn tick(&mut self) -> Result<TickOutcome, DrainError> {
        match self.step() {
            Err(error) if error.is_unreachable() => Ok(TickOutcome::Advanced),
            other => other,
        }
    }

    fn step(&mut self) -> Result<TickOutcome, DrainError> {
        self.scratch.tick_no += 1;
        if !self.scratch.started {
            self.scratch.started = true;
            reattach::on_startup(self)?;
        }
        let queue = self.queue.read()?;
        if let Some(job) = active(&queue) {
            let job = job.clone();
            self.advance(&job)?;
            return Ok(TickOutcome::Advanced);
        }
        if let Some(job) = next_to_start(&queue) {
            if self.scratch.tick_no < self.scratch.start_after {
                return Ok(TickOutcome::Advanced);
            }
            let job = job.clone();
            self.start(&job)?;
            return Ok(TickOutcome::Advanced);
        }
        Ok(TickOutcome::Idle)
    }

    fn advance(&mut self, job: &Job) -> Result<(), DrainError> {
        match job.state {
            JobState::Starting => {
                reattach::resume_starting(self, job)?;
                Ok(())
            }
            JobState::Running | JobState::Stopping => self.watch(job),
            JobState::Finishing => finalize::finish(self, job),
            JobState::Unreachable => reattach::retry_unreachable(self, job),
            _ => Ok(()),
        }
    }

    /// One pass over a running job. Silence from the host is counted, and at the sixth tick in a
    /// row the job is called unreachable; nothing else is done to it meanwhile.
    fn watch(&mut self, job: &Job) -> Result<(), DrainError> {
        match self.poll(job) {
            Ok(()) => {
                self.scratch.misses = 0;
                Ok(())
            }
            Err(error) if error.is_unreachable() => {
                self.scratch.misses += 1;
                if self.scratch.misses >= MISS_LIMIT {
                    self.persist(Action::Unreachable { id: job.id.clone() })?;
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn poll(&mut self, job: &Job) -> Result<(), DrainError> {
        let run_id = run_id_of(job)?.to_owned();
        pull(self, job)?;
        self.progress(job)?;
        watchdog::check(self, job)?;
        disk::check_running(self, job)?;
        stop::apply_requests(self, job)?;
        match self.host.inspect(&run_id)? {
            ContainerState::Running => {}
            ContainerState::Exited { code, oom_killed } => {
                self.scratch.oom_killed = oom_killed;
                self.scratch.exit_code = Some(code);
                self.persist(Action::ContainerEnded { id: job.id.clone() })?;
            }
            ContainerState::None => {
                self.persist(Action::ContainerEnded { id: job.id.clone() })?;
            }
        }
        Ok(())
    }

    /// Writes the numbers of the live view into the job, unless they are the ones already there.
    fn progress(&mut self, job: &Job) -> Result<(), DrainError> {
        let live = self.live_dir(job)?;
        let Ok(text) = std::fs::read_to_string(live.join(SPEC)) else {
            return Ok(());
        };
        let Ok(spec) = serde_json::from_str::<LiveSpec>(&text) else {
            return Ok(());
        };
        let found = assess_dir(&live, &spec.spec, &spec.settings);
        let count = |wanted: &[Status]| {
            let n = found
                .quantities
                .iter()
                .filter(|q| wanted.contains(&q.status))
                .count();
            u32::try_from(n).unwrap_or(u32::MAX)
        };
        let monitors = u32::try_from(found.quantities.len()).unwrap_or(u32::MAX);
        let settled = count(&[Status::Settled]);
        let drifting = count(&[Status::Drifting, Status::Oscillating]);
        let next = Progress {
            time: found.last_time.filter(|t| t.is_finite()),
            iterations: found.iterations,
            settled,
            drifting,
            monitors,
            updated: 0,
            summary: Some(format!("settled {settled} of {monitors}")),
        };
        let same = job.progress.as_ref().is_some_and(|old| {
            Progress {
                updated: 0,
                ..old.clone()
            } == next
        });
        if same {
            return Ok(());
        }
        let progress = Progress {
            updated: now(),
            ..next
        };
        self.persist(Action::Progress {
            id: job.id.clone(),
            progress,
        })?;
        Ok(())
    }

    /// Starts a queued job: its run name is written to the queue first, then the case is made
    /// and uploaded and the container started.
    fn start(&mut self, job: &Job) -> Result<(), DrainError> {
        let label = self.label();
        let run_id = cfdcraft_executor::run::run_id(&label)
            .map_err(|_| io_reason("the run name could not be made"))?;
        self.persist(Action::Begin {
            id: job.id.clone(),
            run_id: run_id.clone(),
        })?;
        self.scratch = Scratch {
            tick_no: self.scratch.tick_no,
            started: true,
            ..Scratch::default()
        };
        let started = Job {
            state: JobState::Starting,
            run_id: Some(run_id.clone()),
            ..job.clone()
        };
        let planned = match plan_scenario_job_with(
            &self.config.doc,
            job.scenario.as_deref(),
            finalize::action_of(job),
            &finalize::PLAN_OPTIONS,
        ) {
            Ok(planned) => planned,
            Err(outcome) => return self.end_unplanned(&started, &outcome),
        };
        let (live, np) = match self.set_up(&started, &planned) {
            Ok(done) => done,
            Err(DrainError::Refused { code, line }) => {
                return explain::finish_failed(self, &started, code, *line);
            }
            Err(error) if error.is_unreachable() => return Err(error),
            Err(error) => return finalize::fail(self, &started, error.code(), &[], false),
        };
        match self.host.start(&run_id, &live.join("case"), &label, np) {
            Ok(()) => {}
            Err(HostError::Unreachable) => return Err(HostError::Unreachable.into()),
            Err(HostError::Busy { .. }) => return self.host_busy(&started),
            Err(_) => return self.start_failed(&started),
        }
        self.persist(Action::Started { id: job.id.clone() })?;
        Ok(())
    }

    /// A job the planner has nothing to run for is cancelled; one it refuses fails with the
    /// planner's own code.
    fn end_unplanned(&mut self, job: &Job, outcome: &DocOutcome) -> Result<(), DrainError> {
        if matches!(outcome.line, Some(Line::NothingToRun)) {
            self.persist(Action::Complete {
                id: job.id.clone(),
                outcome: Outcome::Cancelled,
            })?;
            return Ok(());
        }
        let code =
            first_error_code(&outcome.report).unwrap_or_else(|| finalize::NOT_RECORDED.to_owned());
        finalize::fail(self, job, &code, &[], false)
    }

    /// The compute host is held by another job or too loaded: the job goes back to the queue
    /// without using up a try, says so, and is tried again a few ticks later.
    fn host_busy(&mut self, job: &Job) -> Result<(), DrainError> {
        reattach::note(
            self,
            job,
            &Line::JobQueued {
                job: job.id.clone(),
            },
        )?;
        self.persist(Action::Requeue { id: job.id.clone() })?;
        self.scratch.start_after = self.scratch.tick_no + reattach::PROBE_EVERY;
        Ok(())
    }

    /// A start the host refused or could not make: tried again twice, then failed.
    fn start_failed(&mut self, job: &Job) -> Result<(), DrainError> {
        if job.attempts < MAX_ATTEMPTS {
            self.persist(Action::Retry { id: job.id.clone() })?;
            return Ok(());
        }
        finalize::fail(self, job, "CFD-INFRA-007", &[], true)
    }

    /// Everything a start needs that does not touch the compute host's lock: a clean live folder,
    /// the specification of the live view, the free disk, and the case written for upload.
    fn set_up(&mut self, job: &Job, planned: &DocJob) -> Result<(PathBuf, u32), DrainError> {
        let live = self.live_dir(job)?;
        match std::fs::remove_dir_all(&live) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(DrainError::io(&e)),
        }
        std::fs::create_dir_all(&live).map_err(|e| DrainError::io(&e))?;
        let (spec, settings) = planned.assess_inputs().clone();
        let text = serde_json::to_string_pretty(&LiveSpec { spec, settings }).map_err(|e| {
            DrainError::Io {
                reason: e.to_string(),
            }
        })?;
        craft_atomic::atomic_write(&live.join(SPEC), text.as_bytes())
            .map_err(|e| DrainError::io(&e))?;
        craft_atomic::atomic_write(&live.join(STUDY), planned.study_json().as_bytes())
            .map_err(|e| DrainError::io(&e))?;
        match disk::check_before_start(self) {
            Ok(()) => {}
            Err(DiskRefusal::BelowFloor { free_gb, floor_gb }) => {
                return Err(DrainError::refused(
                    "CFD-DISK-003",
                    Line::DiskBelowFloor {
                        free: free_gb.to_string(),
                        floor: floor_gb.to_string(),
                    },
                ));
            }
            Err(DiskRefusal::Host(error)) => return Err(error.into()),
        }
        let plan = case::prepare_start(self, job, planned)?;
        case::write_case(planned, &live, plan.np, &plan.seed)?;
        self.scratch.np = Some(plan.np);
        Ok((live, plan.np))
    }
}

/// Brings the mirror of a job up to date with the host: the three logs (the solver's under the name
/// the job's specification gives it), and every monitor file. Returns whether any file grew.
pub(crate) fn pull<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<bool, DrainError> {
    let run_id = run_id_of(job)?.to_owned();
    let mirror = ctx.mirror(job)?;
    let mut moved = false;
    let solver_log = ctx.solver_log(job)?;
    for rel in logs_of(&solver_log) {
        if rel != solver_log && ended(&mirror.read_to_string(&rel)?) {
            continue;
        }
        moved |= pull_file(&mut ctx.host, &mirror, &run_id, &rel, None)?;
    }
    for file in ctx.host.files(&run_id, POST)? {
        if valid_rel(&file.rel) {
            moved |= pull_file(&mut ctx.host, &mirror, &run_id, &file.rel, Some(file.size))?;
        }
    }
    Ok(moved)
}

/// Whether the last line of a log with text in it is `End`.
fn ended(text: &str) -> bool {
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.trim() == "End")
}

/// Fetches the next piece of one file. `size` is its size on the host when the listing gave one: a
/// file shorter than its mirror was replaced, so the mirror starts again from nothing. Only whole
/// lines are kept, so the mirror's length is always where the next piece begins.
fn pull_file<H: Host>(
    host: &mut H,
    mirror: &Mirror,
    run_id: &str,
    rel: &str,
    size: Option<u64>,
) -> Result<bool, DrainError> {
    let mut have = mirror.len(rel);
    let mut moved = false;
    if let Some(size) = size {
        if size < have {
            mirror.reset(rel)?;
            have = 0;
            moved = true;
        } else if size == have {
            return Ok(false);
        }
    }
    let bytes = host.tail(run_id, rel, have, FETCH_CAP)?;
    let keep = whole_lines(&bytes);
    if keep == 0 {
        return Ok(moved);
    }
    match mirror.append(rel, &bytes[..keep]) {
        Ok(()) => Ok(true),
        // A full mirror stops growing; the job itself goes on.
        Err(MirrorError::Full) => Ok(moved),
        Err(error) => Err(error.into()),
    }
}

/// How many leading bytes end at a newline. A piece with no newline at all is a line longer than
/// the fetch cap when it is as long as the cap, and is kept whole so the mirror cannot stall.
fn whole_lines(bytes: &[u8]) -> usize {
    match bytes.iter().rposition(|b| *b == b'\n') {
        Some(at) => at + 1,
        None if bytes.len() as u64 >= FETCH_CAP => bytes.len(),
        None => 0,
    }
}
