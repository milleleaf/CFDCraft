//! The free-disk floor of the compute host. A job is not started below the floor, and a running
//! job is asked to write now and stop when its next write would take the host below it. Nothing
//! on the host is deleted here: the case writes with `purgeWrite`, which keeps only the latest
//! times.

use cfdcraft_queue::{Job, JobState, StopCause, StopMode};
use cfdcraft_remote::{Host, HostError};

use crate::DrainError;
use crate::tick::{Drain, run_id_of};
use crate::watchdog::request_stop_once;

/// Kilobytes in a gigabyte, as the host counts them.
pub const KB_PER_GB: u64 = 1_048_576;
/// The code of a job that was not started for lack of disk.
pub const BELOW_FLOOR_CODE: &str = "CFD-DISK-003";
/// The code a job carries when it was stopped for lack of disk.
pub const STOPPED_CODE: &str = "CFD-DISK-004";

/// Why a job must not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskRefusal {
    /// The free disk is below the floor; both numbers are whole gigabytes.
    BelowFloor { free_gb: u64, floor_gb: u64 },
    /// The host could not say how much is free.
    Host(HostError),
}

fn floor_kb<H: Host>(ctx: &Drain<H>) -> u64 {
    u64::from(ctx.config.min_free_gb).saturating_mul(KB_PER_GB)
}

/// Whether the free disk is below the floor.
pub fn below_floor(free_kb: u64, floor_kb: u64) -> bool {
    free_kb < floor_kb
}

/// Whether the next write of `write_kb` would take the free disk to the floor or under it.
pub fn needs_write_now(free_kb: u64, floor_kb: u64, write_kb: u64) -> bool {
    free_kb < floor_kb.saturating_add(write_kb)
}

/// Whether there is room to start a job. A host that cannot say how much is free is a refusal,
/// never a pass.
pub fn check_before_start<H: Host>(ctx: &mut Drain<H>) -> Result<(), DiskRefusal> {
    let floor = floor_kb(ctx);
    let free = ctx.host.free_kb().map_err(DiskRefusal::Host)?;
    if below_floor(free, floor) {
        return Err(DiskRefusal::BelowFloor {
            free_gb: free / KB_PER_GB,
            floor_gb: floor / KB_PER_GB,
        });
    }
    Ok(())
}

/// Whether a running job still has room for its next write: the largest time folder so far is
/// taken as the size of that write. When it does not, the job is asked once to write now and end.
pub fn check_running<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<(), DrainError> {
    if job.state != JobState::Running || job.stop.is_some() {
        return Ok(());
    }
    let run_id = run_id_of(job)?.to_owned();
    let free = ctx.host.free_kb()?;
    let write = ctx
        .host
        .time_dirs(&run_id)?
        .iter()
        .map(|dir| dir.kb)
        .max()
        .unwrap_or(0);
    if needs_write_now(free, floor_kb(ctx), write)
        && request_stop_once(ctx, job, StopMode::WriteNow, StopCause::Disk)?
    {
        ctx.scratch.disk_stop = true;
    }
    Ok(())
}
