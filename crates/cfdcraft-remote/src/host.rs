//! The one narrow trait everything the drain does to the compute host goes
//! through. The real implementation is `SshHost`; the tests use `FakeBox`.

use cfdcraft_executor::{Holder, ReleaseOutcome};
use std::path::Path;

/// The longest relative path a host call accepts.
pub const MAX_REL: usize = 200;
/// The prefix every container this tool starts carries.
pub const OWNED_PREFIX: &str = "cfdcraft-";

/// A run id: `[A-Za-z0-9-]`, 1 to 128 characters.
pub fn valid_run_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A run id that names a container this tool may touch.
pub fn is_owned(id: &str) -> bool {
    valid_run_id(id) && id.starts_with(OWNED_PREFIX)
}

/// A path below a run folder: `[A-Za-z0-9_./-]`, 1 to 200 characters, no leading
/// `/`, no leading `-` (a tool would read it as an option) and no `..`.
pub fn valid_rel(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_REL
        && !path.starts_with(['/', '-'])
        && !path.contains("..")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c))
}

/// A solver time folder name: starts with a digit and reads as a number.
pub fn is_time_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_digit()) && name.parse::<f64>().is_ok()
}

/// What the container service says about one container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerState {
    /// There is no container of that name.
    None,
    Running,
    Exited {
        code: i32,
        oom_killed: bool,
    },
}

/// Why a host call did not give an answer. The sentences name no host, key or
/// folder.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    /// The connection failed or timed out. The run itself is not judged by it.
    #[error("the compute host could not be reached")]
    Unreachable,
    /// Another job holds the host.
    #[error("the compute host is busy")]
    Busy { stale: bool },
    /// The request was not made: a piece of it is not allowed.
    #[error("the request was refused: {reason}")]
    Refused { reason: &'static str },
    /// The host or its container service failed; `class` is a stable code.
    #[error("the compute host failed ({class})")]
    Failed { class: &'static str },
}

/// One file under a run folder, with its size in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub rel: String,
    pub size: u64,
}

/// One time folder of a run, with its size in kilobytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeDir {
    pub name: String,
    pub kb: u64,
}

/// Everything the drain does to the compute host.
pub trait Host {
    /// Upload `case`, start the labelled container and keep the lock for `run_id`.
    fn start(&mut self, run_id: &str, case: &Path, doc: &str, np: u32) -> Result<(), HostError>;
    fn inspect(&mut self, run_id: &str) -> Result<ContainerState, HostError>;
    /// Who holds the lock, if anyone.
    fn holder(&mut self) -> Result<Option<Holder>, HostError>;
    /// At most `max` bytes of a run file after `offset`; nothing for a missing file.
    fn tail(
        &mut self,
        run_id: &str,
        rel: &str,
        offset: u64,
        max: u64,
    ) -> Result<Vec<u8>, HostError>;
    /// Every file under `dir` in the run folder, with its size.
    fn files(&mut self, run_id: &str, dir: &str) -> Result<Vec<RemoteFile>, HostError>;
    /// The numeric folders of the run, with their sizes.
    fn time_dirs(&mut self, run_id: &str) -> Result<Vec<TimeDir>, HostError>;
    /// Free kilobytes on the work disk.
    fn free_kb(&mut self) -> Result<u64, HostError>;
    fn read_text(&mut self, run_id: &str, rel: &str) -> Result<String, HostError>;
    /// Replace a file by writing a temporary name and renaming it on the host.
    fn write_text(&mut self, run_id: &str, rel: &str, text: &str) -> Result<(), HostError>;
    /// Stop the container of `run_id`; nothing to stop is not an error.
    fn stop_container(&mut self, run_id: &str, grace_seconds: u32) -> Result<(), HostError>;
    /// Copy the run folder, without the per-process folders, into `dest`.
    fn download(&mut self, run_id: &str, dest: &Path) -> Result<(), HostError>;
    fn remove_container(&mut self, run_id: &str) -> Result<(), HostError>;
    /// Remove the run folder of `run_id`. Absent is not an error. A host that does not implement
    /// it refuses, so a test double never deletes by accident.
    fn remove_run_dir(&mut self, _run_id: &str) -> Result<(), HostError> {
        Err(HostError::Refused {
            reason: "removing a run folder is not supported",
        })
    }
    /// Give the lock back, only if `run_id` holds it.
    fn release(&mut self, run_id: &str) -> Result<ReleaseOutcome, HostError>;
}
