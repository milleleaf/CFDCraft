//! Picking a job up again after the process that ran it ended, and waiting for a host that does
//! not answer.
//!
//! A new process never trusts the queue file alone. For a job that was active it asks the host
//! who holds the lock and what became of the container, and [`reconcile`] decides from the two:
//!
//! | lock | container | job was | what happens |
//! |------|-----------|---------|--------------|
//! | ours | running | any | carry on from the mirror length |
//! | ours | exited | any | the job goes on to finishing |
//! | ours | none | past starting | the job goes on to finishing; the files that exist are judged |
//! | ours | none | starting | the lock is given back (by exact run name), the start is retried twice, then fails |
//! | someone else's or none | none | starting | the start is retried twice, then fails; nothing is released |
//! | someone else's or none | any | past starting | interrupted with `CFD-INFRA-008`; the host is left as it is |
//!
//! An inconsistent state is reported, never repaired by force: no container is stopped or removed
//! and no lock is released. A host that does not answer changes nothing at all.
//!
//! The cleaning of tunnel socket folders only touches folders named `socket-<digits>-<tag>`, only
//! real directories (a link is never followed), and only when the process that made them is gone.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use cfdcraft_queue::{Action, Job, JobState, MAX_ATTEMPTS, Outcome, Progress, active};
use cfdcraft_remote::{ContainerState, Holder, Host, HostError};
use cfdcraft_text::Line;

use crate::tick::{Drain, run_id_of};
use crate::{DrainError, finalize};

/// The code of a start the host never carried out.
const START_FAILED: &str = "CFD-INFRA-007";
/// The code of a host whose state does not match the queue.
pub const HOST_MISMATCH: &str = "CFD-INFRA-008";
/// While a job is unreachable the host is asked on every this-many-th tick.
pub const PROBE_EVERY: u64 = 4;

/// What [`reconcile`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconciled {
    /// The host did not answer; nothing was changed.
    Unreachable,
    /// The container is running; the job goes on from its mirror.
    Resumed,
    /// The container ended; the job goes on to finishing.
    Ended,
    /// The start never happened; the job is queued again.
    Requeued,
    /// The start never happened and the tries are used up; the job failed.
    Failed,
    /// The host does not match the queue; the job is interrupted and the host left alone.
    Inconsistent,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Who holds the lock and what became of the container of `run_id`.
fn look<H: Host>(
    host: &mut H,
    run_id: &str,
) -> Result<(Option<Holder>, ContainerState), HostError> {
    Ok((host.holder()?, host.inspect(run_id)?))
}

/// Decides from the lock and the container what became of an active job, and acts on it.
pub fn reconcile<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<Reconciled, DrainError> {
    let run_id = run_id_of(job)?.to_owned();
    let (holder, container) = match look(&mut ctx.host, &run_id) {
        Ok(seen) => seen,
        Err(HostError::Unreachable) => return Ok(Reconciled::Unreachable),
        Err(error) => return Err(error.into()),
    };
    let ours = holder.is_some_and(|h| h.run_id == run_id);
    let starting = job.state == JobState::Starting;
    match container {
        ContainerState::Running if ours => resume(ctx, job),
        ContainerState::Exited { code, oom_killed } if ours => {
            ctx.scratch.oom_killed = oom_killed;
            ctx.scratch.exit_code = Some(code);
            end(ctx, job)
        }
        ContainerState::None if ours && !starting => end(ctx, job),
        ContainerState::None if ours => {
            ctx.host.release(&run_id)?;
            start_lost(ctx, job)
        }
        ContainerState::None if starting => start_lost(ctx, job),
        _ => inconsistent(ctx, job),
    }
}

/// Brings a job that was starting or unreachable to running, as the host shows it is.
fn to_running<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<(), DrainError> {
    let action = match job.state {
        JobState::Starting => Action::Started { id: job.id.clone() },
        JobState::Unreachable => Action::Reachable { id: job.id.clone() },
        _ => return Ok(()),
    };
    ctx.persist(action)?;
    Ok(())
}

fn resume<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<Reconciled, DrainError> {
    to_running(ctx, job)?;
    note(
        ctx,
        job,
        &Line::JobReattached {
            job: job.id.clone(),
        },
    )?;
    Ok(Reconciled::Resumed)
}

fn end<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<Reconciled, DrainError> {
    to_running(ctx, job)?;
    ctx.persist(Action::ContainerEnded { id: job.id.clone() })?;
    Ok(Reconciled::Ended)
}

/// A start that left nothing running: tried again while there are tries left, then failed. The
/// lock was given back before this is called when it was ours; it is never touched here.
fn start_lost<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<Reconciled, DrainError> {
    if job.attempts < MAX_ATTEMPTS {
        ctx.persist(Action::Retry { id: job.id.clone() })?;
        return Ok(Reconciled::Requeued);
    }
    let live = ctx.live_dir(job)?;
    std::fs::create_dir_all(&live).map_err(|e| DrainError::io(&e))?;
    let sentence = ctx.text.render(&Line::JobFailed {
        job: job.id.clone(),
        reason: "the compute host did not start it".to_owned(),
    });
    finalize::fail(ctx, job, START_FAILED, &[sentence], false)?;
    Ok(Reconciled::Failed)
}

/// The host does not match the queue. It is left exactly as it is: no stop, no remove, no release.
fn inconsistent<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<Reconciled, DrainError> {
    note(
        ctx,
        job,
        &Line::JobInterrupted {
            job: job.id.clone(),
            reason: "the compute host does not match the queue, so it was left as it is".to_owned(),
        },
    )?;
    ctx.persist(Action::Complete {
        id: job.id.clone(),
        outcome: Outcome::Interrupted {
            code: HOST_MISMATCH.to_owned(),
        },
    })?;
    Ok(Reconciled::Inconsistent)
}

/// Puts a sentence into the progress summary of the job, keeping the numbers already there.
pub(crate) fn note<H: Host>(ctx: &mut Drain<H>, job: &Job, line: &Line) -> Result<(), DrainError> {
    let progress = Progress {
        summary: Some(ctx.text.render(line)),
        updated: now(),
        ..job.progress.clone().unwrap_or_default()
    };
    ctx.persist(Action::Progress {
        id: job.id.clone(),
        progress,
    })?;
    Ok(())
}

/// Runs once, before the first tick, for the job that was active when the process started. A job
/// found starting is dealt with at the top of the tick by [`resume_starting`].
pub fn on_startup<H: Host>(ctx: &mut Drain<H>) -> Result<(), DrainError> {
    if let Some(base) = ctx.config.socket_dir.clone() {
        clean_stale_sockets(&base);
    }
    let queue = ctx.queue.read()?;
    if let Some(job) = active(&queue)
        && matches!(
            job.state,
            JobState::Running | JobState::Stopping | JobState::Unreachable
        )
    {
        let job = job.clone();
        reconcile(ctx, &job)?;
    }
    Ok(())
}

/// Called for a job found starting at the top of a tick, for example after a crash. Answers
/// whether it dealt with the job; a host that did not answer leaves it for the next tick.
pub fn resume_starting<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<bool, DrainError> {
    Ok(reconcile(ctx, job)? != Reconciled::Unreachable)
}

/// Called for a job the host has stopped answering for. The host is asked on every fourth tick
/// only; when it answers the job is brought back to where the host shows it is. The container
/// and the lock are never touched while it stays quiet.
pub fn retry_unreachable<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<(), DrainError> {
    if ctx.scratch.tick_no.is_multiple_of(PROBE_EVERY) {
        reconcile(ctx, job)?;
    }
    Ok(())
}

/// Removes the tunnel socket folders that processes which are gone left in `base`, and returns
/// how many. Only a real directory named `socket-<digits>-<tag>` is considered, never a link;
/// the folder of this process or of a process that still exists is kept. Where a process cannot
/// be asked about, nothing is removed.
pub fn clean_stale_sockets(base: &Path) -> usize {
    #[cfg(unix)]
    {
        clean_dead(base)
    }
    #[cfg(not(unix))]
    {
        let _ = base;
        0
    }
}

#[cfg(unix)]
fn clean_dead(base: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(base) else {
        return 0;
    };
    let me = std::process::id();
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(pid) = socket_pid(name) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let path = base.join(name);
        let is_dir = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_dir());
        if is_dir && !process_exists(pid) && std::fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// The process number in a folder name of the form `socket-<digits>-<tag>`, where the tag is
/// plain letters and digits.
#[cfg(unix)]
fn socket_pid(name: &str) -> Option<u32> {
    let (digits, tag) = name.strip_prefix("socket-")?.split_once('-')?;
    let plain_tag =
        !tag.is_empty() && tag.len() <= 32 && tag.chars().all(|c| c.is_ascii_alphanumeric());
    let plain_pid =
        !digits.is_empty() && digits.len() <= 10 && digits.chars().all(|c| c.is_ascii_digit());
    if !(plain_tag && plain_pid) {
        return None;
    }
    // 0 and 1 are never a tunnel's process, and a number above the signed range names no process.
    digits
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid >= 2 && i32::try_from(*pid).is_ok())
}

/// Whether the system knows the process. When it cannot be asked, the answer is yes, so nothing
/// is removed on a guess.
#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    use std::process::{Command, Stdio};
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(true)
}
