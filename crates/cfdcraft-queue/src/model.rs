//! The persisted shape of a queue.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::check::check_queue;
use crate::reducer::QueueError;

/// The version of the queue file this crate reads and writes.
pub const QUEUE_SCHEMA: u32 = 1;

/// The largest queue file, in bytes, that [`Queue::from_json`] reads.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

fn first_number() -> u32 {
    1
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// The whole queue file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Queue {
    /// The version of this file's shape.
    pub schema_version: u32,
    /// The number the next job gets. It is never lowered, so clearing finished jobs never makes a
    /// job id come back.
    #[serde(default = "first_number")]
    pub next_number: u32,
    /// The jobs, in the order they will start.
    #[serde(default)]
    pub jobs: Vec<Job>,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

impl Queue {
    /// An empty queue.
    pub fn new() -> Self {
        Self {
            schema_version: QUEUE_SCHEMA,
            next_number: 1,
            jobs: Vec::new(),
        }
    }

    /// Reads a queue file: the version first, then the shape, then every rule of
    /// [`check_queue`]. A file that is too large, damaged, newer than this version or against the
    /// rules is refused by name.
    pub fn from_json(text: &str) -> Result<Self, QueueError> {
        if text.len() > MAX_BYTES {
            return Err(QueueError::Damaged {
                reason: "the file is larger than 4 MiB".to_owned(),
            });
        }
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| QueueError::Damaged {
                reason: short(&e.to_string()),
            })?;
        let found = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| QueueError::Damaged {
                reason: "schema_version is missing or is not a whole number".to_owned(),
            })?;
        if found > u64::from(QUEUE_SCHEMA) {
            return Err(QueueError::NewerSchema {
                found: u32::try_from(found).unwrap_or(u32::MAX),
            });
        }
        if found != u64::from(QUEUE_SCHEMA) {
            return Err(QueueError::Damaged {
                reason: format!("schema_version {found} is not a version this file ever had"),
            });
        }
        let queue: Self = serde_json::from_value(value).map_err(|e| QueueError::Damaged {
            reason: short(&e.to_string()),
        })?;
        check_queue(&queue)?;
        Ok(queue)
    }

    /// The file text: pretty JSON with a final newline.
    pub fn to_json(&self) -> String {
        // Serializing this type cannot fail: every key is a string, and serde_json writes a number
        // that is not finite as null (check_queue refuses such a time before it is stored).
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }
}

fn short(text: &str) -> String {
    text.chars().take(200).collect()
}

/// One job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Job {
    /// `q-` and a number of at least three digits, for example `q-001`.
    pub id: String,
    /// The scenario to solve; none means the study itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    /// What the job does.
    pub kind: JobKind,
    /// The sweep point the job came from, if it came from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Where the job is in its life.
    pub state: JobState,
    /// How many times a start was retried.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub attempts: u32,
    /// The name of the run (and of its container); set from `starting` until the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The run recorded for a finished or stopped job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// The stable code of a failed or interrupted job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// A stop that was asked for and not yet carried out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<StopRequest>,
    /// The last progress report of an active job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
}

/// What a job does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum JobKind {
    /// Solve from the start.
    Solve,
    /// Continue a stopped run from its latest written time.
    Continue {
        /// The stopped run to continue.
        run: String,
    },
}

/// The sweep point a job came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    /// The sweep id.
    pub sweep: String,
    /// The point number within the sweep.
    pub index: u32,
}

/// Where a job is in its life. The last five are end states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Waiting its turn.
    Queued,
    /// Being uploaded and started.
    Starting,
    /// Running on the compute host.
    Running,
    /// A stop was applied; waiting for the solver to write and end.
    Stopping,
    /// The container ended; the files are being fetched and judged.
    Finishing,
    /// The host cannot be reached; the run may still be going.
    Unreachable,
    /// Ended and recorded.
    Finished,
    /// Stopped on request, with a recorded checkpoint.
    Stopped,
    /// Ended with an error.
    Failed,
    /// Cancelled before it ran.
    Cancelled,
    /// Lost: the watch gave up.
    Interrupted,
}

impl JobState {
    /// The state as the word stored in the file.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Finishing => "finishing",
            Self::Unreachable => "unreachable",
            Self::Finished => "finished",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    /// Whether the job has ended.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Finished | Self::Stopped | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    /// Whether the job holds the one place that jobs may be in at a time.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::Stopping | Self::Finishing | Self::Unreachable
        )
    }
}

/// How a stop writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StopMode {
    /// Write the fields now, then end.
    WriteNow,
    /// End without writing (the fields cannot be trusted).
    NoWrite,
}

/// Why a stop was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StopCause {
    /// A person asked.
    User,
    /// The job was cancelled.
    Cancel,
    /// The solution diverged.
    Diverged,
    /// The disk is nearly full.
    Disk,
}

/// A stop that was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StopRequest {
    /// How to stop.
    pub mode: StopMode,
    /// Why.
    pub cause: StopCause,
}

/// A progress report, for display only. It holds no clock: `updated` is a number the caller
/// passes in. The resume offset of a mirrored file is the length of its mirror file, never stored
/// here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    /// The latest solver time, if the solver has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
    /// The iterations or time steps so far.
    #[serde(default)]
    pub iterations: u64,
    /// How many monitored quantities have settled.
    #[serde(default)]
    pub settled: u32,
    /// How many are still drifting.
    #[serde(default)]
    pub drifting: u32,
    /// How many are monitored.
    #[serde(default)]
    pub monitors: u32,
    /// When the report was made, as a number the caller chose.
    #[serde(default)]
    pub updated: u64,
    /// A one-line summary for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// A job to add to the queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewJob {
    /// The scenario to solve; none means the study itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    /// What the job does.
    pub kind: JobKind,
    /// The sweep point the job came from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
}
