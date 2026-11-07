//! The job queue of a study: the shape of the list, and the rules that move it.
//!
//! A queue is an ordered list of jobs. A solve, a continued solve and each point of a sweep become
//! jobs in one list, and the list outlives the app that filled it. Every job goes from `queued`
//! through `starting`, `running` and `finishing` to one of the end states, and every move is an
//! [`Action`] applied to an immutable [`Queue`] by [`apply`], which returns a new value.
//!
//! This crate is pure on purpose. It opens no file, reads no clock and no environment, and starts
//! no process: the caller reads and writes the file, and passes in any number that depends on
//! time. The CLI, the MCP tools and the background drain all use these rules, so none of them can
//! disagree about what a legal move is, and a damaged or hand-edited file is refused by name by
//! [`Queue::from_json`] before anything acts on it.

mod check;
mod model;
mod reducer;

pub use check::{MAX_CODE, MAX_JOBS, MAX_RUN_ID, check_queue};
pub use model::{
    Job, JobKind, JobState, MAX_BYTES, NewJob, Origin, Progress, QUEUE_SCHEMA, Queue, StopCause,
    StopMode, StopRequest,
};
pub use reducer::{Action, MAX_ATTEMPTS, Outcome, QueueError, active, apply, next_to_start};
