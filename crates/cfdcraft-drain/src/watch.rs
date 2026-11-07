//! Watching a job from its mirror. Everything here is read-only and never touches the compute
//! host: the numbers come from the files the drain has already copied, and the specification they
//! are judged by was written when the job started.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use cfdcraft_assess::{Assessment, Spec, assess};
use cfdcraft_backend_esi::Dialect;
use cfdcraft_doc::{open, store_dir};
use cfdcraft_logscan::redact;
use cfdcraft_ops::convergence::{assess_dir, files_from_bundle, settings_for, spec_for};
use cfdcraft_ops::prepare;
use cfdcraft_ops::queue_file::QueueFile;
use cfdcraft_queue::JobState;
use cfdcraft_runs::{find_run, is_plain_name};
use cfdcraft_settle::{Balance, BalanceStatus, NOT_FINISHED, Settings, Status, Verdict};
use cfdcraft_store::Store;
use serde::{Deserialize, Serialize};

use crate::tick::SPEC;

/// The solver log of a job whose specification names none.
const DEFAULT_SOLVER_LOG: &str = "log.simpleFoam";
/// How far back from an offset to look for the start of its line.
const LINE_WINDOW: u64 = 4096;
/// The most bytes one view of the log reads.
const READ_CAP: u64 = 4 * 1024 * 1024;
/// The most lines one view of the log holds.
pub const MAX_LINES: usize = 500;

/// What the verdict on a job is made from, written when the job starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveSpec {
    pub spec: Spec,
    pub settings: Settings,
}

/// Why a job could not be watched.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WatchError {
    /// The text is not a job id.
    #[error("\"{job}\" is not a job id")]
    BadJob { job: String },
    /// The queue holds no such job.
    #[error("there is no job {job} in the queue")]
    UnknownJob { job: String },
    /// The job has not started, so there is nothing of it to watch yet.
    #[error("the job {job} has no live view yet")]
    NoMirror { job: String },
    /// The document records no run of that name.
    #[error("there is no run {run} in the document")]
    UnknownRun { run: String },
    /// The run kept no stored result to read: it recorded none, or it was pruned.
    #[error("the run {run} has no stored result to read")]
    NoResult { run: String },
    /// The queue file could not be read.
    #[error("the queue could not be read: {reason}")]
    Queue { reason: String },
    /// A file of the live view could not be read.
    #[error("the live view could not be read: {reason}")]
    Io { reason: String },
}

/// One monitored quantity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowView {
    /// The name in the case, for example `probe_1.Ux`.
    pub name: String,
    /// The study's own name for the monitor, with the same ending, when it has one.
    pub label: Option<String>,
    pub last: f64,
    pub mean: f64,
    pub spread: f64,
    pub state: Status,
}

/// One balance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BalanceView {
    pub status: BalanceStatus,
    pub imbalance: Option<f64>,
}

impl BalanceView {
    fn of(balance: &Balance) -> Self {
        Self {
            status: balance.status,
            imbalance: balance.imbalance,
        }
    }
}

/// The table of a job: its quantities, balances and verdict as the files stand now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorsView {
    pub job: String,
    pub state: JobState,
    pub iterations: u64,
    pub last_time: Option<f64>,
    pub rows: Vec<RowView>,
    pub mass: BalanceView,
    pub energy: BalanceView,
    pub verdict: Verdict,
    /// Whether the solver log has its end line.
    pub ended: bool,
}

/// The table of a recorded run: its quantities, balances and verdict, made again from its stored
/// result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunView {
    pub run: String,
    pub iterations: u64,
    pub last_time: Option<f64>,
    pub rows: Vec<RowView>,
    pub mass: BalanceView,
    pub energy: BalanceView,
    /// The verdict made now from the stored files.
    pub verdict: Verdict,
    /// The verdict recorded with the run, when it has one.
    pub recorded: Option<Verdict>,
    /// Whether the solver log has its end line.
    pub ended: bool,
}

/// A piece of the solver log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogView {
    /// The lines, with host paths and run names hidden.
    pub text: String,
    /// Where the next piece begins.
    pub next_offset: u64,
}

/// The name of the solver log of the job whose live folder is `live`, from the specification
/// written when the job started. A specification that is missing, unreadable, from before the log
/// had a name, or that names something other than a plain file name gives the default.
pub fn solver_log_of(live: &Path) -> String {
    let named = fs::read_to_string(live.join(SPEC))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .pointer("/spec/log_file")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        });
    match named {
        Some(name) if is_plain_file_name(&name) => name,
        _ => DEFAULT_SOLVER_LOG.to_owned(),
    }
}

/// Whether `name` is one file name: not empty, no separator, not a dot name, no control character.
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

fn clean(job: &str) -> String {
    job.chars()
        .take(40)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// The state of a job and the folder of its live view, for a job id that is in the queue.
fn locate(doc: &Path, job: &str) -> Result<(JobState, PathBuf), WatchError> {
    let file = QueueFile::new(doc);
    let live = file
        .live_dir(job)
        .ok_or_else(|| WatchError::BadJob { job: clean(job) })?;
    let queue = file.read().map_err(|e| WatchError::Queue {
        reason: e.to_string(),
    })?;
    let found = queue
        .jobs
        .iter()
        .find(|j| j.id == job)
        .ok_or_else(|| WatchError::UnknownJob { job: clean(job) })?;
    Ok((found.state, live))
}

/// The quantities of an assessment, each with the study's own name for it when it has one.
fn rows(found: &Assessment) -> Vec<RowView> {
    found
        .quantities
        .iter()
        .map(|q| {
            let fo = q.name.split('.').next().unwrap_or_default();
            let label = found
                .labels
                .get(fo)
                .map(|label| format!("{label}{}", &q.name[fo.len()..]));
            RowView {
                name: q.name.clone(),
                label,
                last: q.last,
                mean: q.mean,
                spread: q.spread,
                state: q.status,
            }
        })
        .collect()
}

/// The quantities, balances and verdict of a job, from its mirror.
pub fn monitors(doc: &Path, job: &str) -> Result<MonitorsView, WatchError> {
    let (state, live) = locate(doc, job)?;
    let none = || WatchError::NoMirror { job: clean(job) };
    let text = fs::read_to_string(live.join(SPEC)).map_err(|_| none())?;
    let given: LiveSpec = serde_json::from_str(&text).map_err(|_| none())?;
    let found = assess_dir(&live, &given.spec, &given.settings);
    let rows = rows(&found);
    // A job that failed, was stopped or was lost did not go to its end: it is never shown as
    // converged, however its monitors stand.
    let verdict = if state.is_terminal() && state != JobState::Finished {
        found.verdict.blocked_by(NOT_FINISHED)
    } else {
        found.verdict
    };
    Ok(MonitorsView {
        job: job.to_owned(),
        state,
        iterations: found.iterations,
        last_time: found.last_time,
        rows,
        mass: BalanceView::of(&found.mass),
        energy: BalanceView::of(&found.energy),
        verdict,
        ended: found.ended,
    })
}

/// The quantities, balances and verdict of the recorded run `run`, made again from the result the
/// document stored for it. Nothing is read from the compute host.
pub fn monitors_of_run(doc: &Path, run: &str) -> Result<RunView, WatchError> {
    let missing = || WatchError::UnknownRun { run: clean(run) };
    let no_result = || WatchError::NoResult { run: clean(run) };
    let unusable = |reason: &str| WatchError::Io {
        reason: reason.to_owned(),
    };
    if !is_plain_name(run) {
        return Err(missing());
    }
    let opened = open(doc).map_err(|_| unusable("the document cannot be opened"))?;
    let record = find_run(&opened.document.artifacts.runs, run).ok_or_else(missing)?;
    let blob = record
        .result
        .as_ref()
        .map(|out| out.blob)
        .ok_or_else(no_result)?;
    let bytes = Store::open(store_dir(doc))
        .and_then(|store| store.read(blob))
        .map_err(|_| no_result())?;
    let study = serde_json::to_string(&record.provenance.settings)
        .map_err(|_| unusable("the recorded settings cannot be read"))?;
    let prepared = prepare(&study, Dialect::EsiV2512)
        .map_err(|_| unusable("the recorded settings can no longer be prepared"))?;
    let spec = spec_for(&prepared.resolved);
    let settings = settings_for(&record.provenance.settings);
    let found = assess(&spec, &files_from_bundle(&bytes, &spec), &settings);
    Ok(RunView {
        run: record.id.clone(),
        iterations: found.iterations,
        last_time: found.last_time,
        rows: rows(&found),
        mass: BalanceView::of(&found.mass),
        energy: BalanceView::of(&found.energy),
        verdict: found.verdict,
        recorded: record.verdict.clone(),
        ended: found.ended,
    })
}

/// At most `lines` lines of the solver log, from the line that holds the byte `offset`, with host
/// paths and run names hidden, and the offset where the next piece begins.
pub fn log(doc: &Path, job: &str, offset: u64, lines: usize) -> Result<LogView, WatchError> {
    let (_, live) = locate(doc, job)?;
    if !live.is_dir() {
        return Err(WatchError::NoMirror { job: clean(job) });
    }
    let io = |e: std::io::Error| WatchError::Io {
        reason: e.to_string(),
    };
    let empty = LogView {
        text: String::new(),
        next_offset: offset,
    };
    let Ok(mut file) = fs::File::open(live.join(solver_log_of(&live))) else {
        return Ok(empty);
    };
    let len = file.metadata().map_err(io)?.len();
    let from = offset.min(len);
    let back = from.min(LINE_WINDOW);
    file.seek(SeekFrom::Start(from - back)).map_err(io)?;
    let mut buf = Vec::new();
    file.take(back + READ_CAP)
        .read_to_end(&mut buf)
        .map_err(io)?;

    let at = usize::try_from(back).unwrap_or(0).min(buf.len());
    let start = if at == 0 || buf[at - 1] == b'\n' {
        at
    } else {
        match buf[..at].iter().rposition(|b| *b == b'\n') {
            Some(i) => i + 1,
            // No line start in the window: at the start of the file this is the first line.
            None if from == back => 0,
            None => at,
        }
    };
    let first = (from - back) + start as u64;
    let wanted = lines.min(MAX_LINES);
    let mut end = 0;
    let mut taken = 0;
    if wanted > 0 {
        for (i, byte) in buf[start..].iter().enumerate() {
            if *byte == b'\n' {
                taken += 1;
                end = i + 1;
                if taken == wanted {
                    break;
                }
            }
        }
    }
    let text = redact(&String::from_utf8_lossy(&buf[start..start + end]));
    Ok(LogView {
        text,
        next_offset: first + end as u64,
    })
}
