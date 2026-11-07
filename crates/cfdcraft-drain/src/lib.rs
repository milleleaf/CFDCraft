//! The loop that advances the queue of one document through a compute host.
//!
//! A tick reads `queue.json` and does one step for the job that is active, or starts the next
//! queued job. Every state change is written to the queue file before the next step, so a process
//! that is killed at any point is replaced by a new one that reads the file and carries on. The
//! loop is generic over [`cfdcraft_remote::Host`]: the in-memory box and the real host run the
//! same code.
//!
//! What a person sees of a running job is a mirror under `live/<job>/` in the document's store
//! folder: the logs and monitor files, appended byte for byte, so the length of a mirror file is
//! the offset to resume from. The watcher in [`watch`] reads only that mirror and never the host.

pub mod case;
pub mod disk;
pub mod explain;
pub mod finalize;
pub mod lifecycle;
pub mod mirror;
pub mod reattach;
pub mod stop;
pub mod tick;
pub mod watch;
pub mod watchdog;

use std::path::PathBuf;
use std::time::Duration;

use cfdcraft_ops::queue_file::QueueFileError;
use cfdcraft_queue::QueueError;
use cfdcraft_remote::HostError;
use cfdcraft_text::Line;

pub use lifecycle::{DrainEnd, run_drain, spawn_detached};
pub use mirror::{Mirror, MirrorError};
pub use tick::{Drain, Scratch, TickOutcome};
pub use watch::{LiveSpec, LogView, MonitorsView, WatchError};

/// How the drain is set up. [`DrainConfig::new`] gives every member but the document and the host
/// label its documented default, so a caller writes `DrainConfig { poll: .., ..DrainConfig::new(..) }`
/// and a member added later never breaks it.
#[derive(Debug, Clone)]
pub struct DrainConfig {
    /// The document whose queue is advanced.
    pub doc: PathBuf,
    /// The label of the compute host recorded on a run. It is a name, never an address.
    pub profile_name: String,
    /// The most processes the host allows one job.
    pub np_cap: u32,
    /// The free disk, in whole gigabytes, below which nothing is started.
    pub min_free_gb: u32,
    /// How long to wait between two ticks.
    pub poll: Duration,
    /// How many idle ticks in a row end the drain.
    pub idle_exit_after: u32,
    /// How many ticks a stopping job gets to end before the container is stopped.
    pub stop_wait_ticks: u32,
    /// The folder the tunnel sockets are made in, when the caller chose one.
    pub socket_dir: Option<PathBuf>,
    /// Remove the folder of a run on the host once its results are recorded. Off unless the
    /// host profile switches it on.
    pub clean_run_dirs: bool,
}

impl DrainConfig {
    /// A set-up for `doc` and the host called `profile_name`, with the documented defaults: a
    /// poll every 5 seconds, 12 idle ticks to exit, one process, a 20 GB floor, 12 ticks for a
    /// stop and no removal of run folders on the host.
    pub fn new(doc: impl Into<PathBuf>, profile_name: impl Into<String>) -> Self {
        Self {
            doc: doc.into(),
            profile_name: profile_name.into(),
            np_cap: 1,
            min_free_gb: 20,
            poll: Duration::from_secs(5),
            idle_exit_after: 12,
            stop_wait_ticks: 12,
            socket_dir: None,
            clean_run_dirs: false,
        }
    }
}

/// Why a step of the drain did not complete.
#[derive(Debug, thiserror::Error)]
pub enum DrainError {
    /// The queue file could not be read or changed.
    #[error("{source}")]
    Queue {
        #[from]
        source: QueueFileError,
    },
    /// A call to the compute host failed.
    #[error("{source}")]
    Host {
        #[from]
        source: HostError,
    },
    /// The live view of a job could not be written.
    #[error("{source}")]
    Mirror {
        #[from]
        source: MirrorError,
    },
    /// A local file could not be used.
    #[error("a local file could not be used: {reason}")]
    Io { reason: String },
    /// A job cannot start; `code` says why and `line` is the sentence for a person.
    #[error("the job was refused ({code})")]
    Refused { code: &'static str, line: Box<Line> },
    /// A step that a later change to its module fills in.
    #[error("this step is not handled")]
    NotHandled,
}

impl DrainError {
    /// An input or output failure, with the reason in words.
    pub fn io(error: &std::io::Error) -> Self {
        Self::Io {
            reason: error.to_string(),
        }
    }

    /// A job that cannot start, with the stable code and the sentence.
    pub fn refused(code: &'static str, line: Line) -> Self {
        Self::Refused {
            code,
            line: Box::new(line),
        }
    }

    /// Whether the compute host simply did not answer. That is a missed tick, never a failure.
    pub fn is_unreachable(&self) -> bool {
        matches!(
            self,
            Self::Host {
                source: HostError::Unreachable
            }
        )
    }

    /// The code that ends the drain when the queue file itself is unusable: it is damaged, newer
    /// than this build, or cannot be read. Any other error leaves the drain going.
    pub fn ends_drain(&self) -> Option<&'static str> {
        const UNUSABLE: &str = "CFD-QUEUE-004";
        match self {
            Self::Queue {
                source: QueueFileError::Io { .. },
            } => Some(UNUSABLE),
            Self::Queue {
                source:
                    QueueFileError::Queue {
                        source: QueueError::Damaged { .. } | QueueError::NewerSchema { .. },
                    },
            } => Some(UNUSABLE),
            _ => None,
        }
    }

    /// A stable code for the error, for a drain that gives up.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Queue { .. } => "CFD-QUEUE-004",
            Self::Host {
                source: HostError::Busy { .. },
            } => "CFD-INFRA-001",
            Self::Host {
                source: HostError::Unreachable,
            } => "CFD-INFRA-009",
            Self::Host {
                source: HostError::Failed { class },
            } => class,
            Self::Refused { code, .. } => code,
            _ => "CFD-INFRA-005",
        }
    }
}
