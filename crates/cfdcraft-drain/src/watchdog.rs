//! Watches the mirrored log of a running job for a solution that is going wrong.
//!
//! A run is asked to stop, without writing, when its log shows a floating-point trap, a residual
//! over both thresholds, a Courant number over the limit (time-stepping runs only) or a
//! continuity error that keeps growing. Only the request is made here: the stop is carried out
//! by [`crate::stop`], and the code and the sentence are worked out again from the same log when
//! the job ends (see [`crate::explain::classify`]).

use cfdcraft_logscan::{Divergence, Limits, LogState, Scanner, divergence};
use cfdcraft_queue::{Action, Job, JobState, StopCause, StopMode};
use cfdcraft_remote::Host;

use crate::DrainError;
use crate::tick::Drain;

/// The solver log in the mirror.
pub(crate) const SOLVER_LOG: &str = "log.simpleFoam";

/// What the mirrored solver log shows so far. The mirror holds only whole lines.
// ponytail: the whole mirror is read again every tick (at most 64 MiB); keep a scanner and its
// offset in memory if a long run makes this slow.
pub(crate) fn scan_log<H: Host>(ctx: &Drain<H>, job: &Job) -> Result<LogState, DrainError> {
    let text = ctx.mirror(job)?.read_to_string(SOLVER_LOG)?;
    let mut scanner = Scanner::new();
    scanner.feed(&text);
    Ok(scanner.state().clone())
}

/// The first rule the mirrored log breaks, if any. `transient` is true for a time-stepping run.
pub fn find<H: Host>(
    ctx: &Drain<H>,
    job: &Job,
    transient: bool,
) -> Result<Option<Divergence>, DrainError> {
    let state = scan_log(ctx, job)?;
    Ok(divergence(&state, &Limits::default(), transient))
}

/// Looks at the mirrored log of a running job and asks for a stop if the run has diverged. The
/// only solver kind in a study is steady, so the Courant rule is off.
pub fn check<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<(), DrainError> {
    check_with(ctx, job, false)
}

/// [`check`] for a run that is, or is not, time-stepping.
pub fn check_with<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    transient: bool,
) -> Result<(), DrainError> {
    if job.state != JobState::Running || job.stop.is_some() {
        return Ok(());
    }
    if find(ctx, job, transient)?.is_none() {
        return Ok(());
    }
    request_stop_once(ctx, job, StopMode::NoWrite, StopCause::Diverged)?;
    Ok(())
}

/// Persists a stop request unless the job is no longer running or already has one. The queue is
/// read again, because the job a tick holds is the one it saw at the start. Returns whether the
/// request was made.
pub(crate) fn request_stop_once<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    mode: StopMode,
    cause: StopCause,
) -> Result<bool, DrainError> {
    let queue = ctx.queue.read()?;
    let open = queue
        .jobs
        .iter()
        .any(|j| j.id == job.id && j.state == JobState::Running && j.stop.is_none());
    if !open {
        return Ok(false);
    }
    ctx.persist(Action::RequestStop {
        id: job.id.clone(),
        mode,
        cause,
    })?;
    Ok(true)
}
