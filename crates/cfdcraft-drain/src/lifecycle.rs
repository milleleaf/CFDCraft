//! The life of the drain process: one per document, running ticks until the queue has stayed empty
//! for a while, and a way to start it detached from the program that added the job.
//!
//! The right to run a queue is a lock on a file beside it, held for as long as the loop lives and
//! released by the system when the process ends, however it ends. Before the loop gives up the
//! lock it looks at the queue once more with the lock released, so a job added in that moment is
//! picked up and not left waiting for a drain that no longer exists.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cfdcraft_ops::queue_file::{DrainLock, QueueFile, QueueFileError};
use cfdcraft_queue::{Action, Progress, active, next_to_start};
use cfdcraft_remote::Host;
use cfdcraft_text::Line;

use crate::tick::{Drain, TickOutcome, now};
use crate::{DrainConfig, DrainError};

/// The longest the loop sleeps without looking at the shutdown flag.
const SLICE: Duration = Duration::from_millis(200);
/// How many ticks in a row may fail for a reason that is not the host's silence before the loop
/// gives up.
const MAX_ERRORS: u32 = 10;

/// How the drain ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrainEnd {
    /// The queue stayed empty and nothing was running.
    Idle,
    /// The shutdown flag was set.
    Stopped,
    /// Another drain holds this document's queue.
    AlreadyRunning,
    /// The drain gave up; `code` says why.
    Failed { code: String },
}

/// Runs the queue of `config.doc` over `host` until it is idle, `shutdown` is set, or the queue
/// file cannot be used.
pub fn run_drain<H: Host>(config: DrainConfig, host: H, shutdown: &AtomicBool) -> DrainEnd {
    run_drain_with(config, host, shutdown, || {})
}

/// [`run_drain`], with a function called in the moment between letting go of the lock and
/// looking at the queue once more. It exists so a test can add a job at exactly that moment.
#[doc(hidden)]
pub fn run_drain_with<H: Host>(
    config: DrainConfig,
    host: H,
    shutdown: &AtomicBool,
    mut in_exit_window: impl FnMut(),
) -> DrainEnd {
    let file = QueueFile::new(&config.doc);
    let mut held: Option<DrainLock> = match file.drain_lock() {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => return DrainEnd::AlreadyRunning,
        Err(error) => return unusable(error),
    };
    let (poll, idle_exit_after) = (config.poll, config.idle_exit_after);
    let mut drain = Drain::new(config, host);
    let (mut idle, mut errors) = (0_u32, 0_u32);
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return DrainEnd::Stopped;
        }
        match drain.tick() {
            Ok(TickOutcome::Idle) => {
                idle += 1;
                errors = 0;
            }
            Ok(TickOutcome::Advanced) => {
                idle = 0;
                errors = 0;
            }
            Err(error) => {
                if let Some(code) = error.ends_drain() {
                    return failed(code);
                }
                idle = 0;
                errors += 1;
                if errors >= MAX_ERRORS {
                    say_gave_up(&drain, error.code());
                    return failed(error.code());
                }
            }
        }
        if idle > 0 && idle >= idle_exit_after {
            drop(held.take());
            in_exit_window();
            match more_to_do(&file) {
                Ok(false) => return DrainEnd::Idle,
                Ok(true) => {}
                Err(error) => return failed(error.code()),
            }
            held = match file.drain_lock() {
                Ok(Some(lock)) => Some(lock),
                // Another drain took over in that moment and will see the job.
                Ok(None) => return DrainEnd::Idle,
                Err(error) => return unusable(error),
            };
            idle = 0;
            continue;
        }
        pause(poll, shutdown);
    }
}

/// Tells the job that is being run that its runner has stopped, in its progress line that the
/// queue and the tools show, so a run whose runner is gone does not look like a run in progress.
/// The run itself goes on on the compute host. Best effort: when the queue cannot be written there
/// is nowhere to say it.
fn say_gave_up<H: Host>(drain: &Drain<H>, code: &str) {
    let Ok(queue) = drain.queue_file().read() else {
        return;
    };
    let Some(job) = active(&queue) else {
        return;
    };
    let line = Line::JobRunning {
        job: job.id.clone(),
        line: format!(
            "the queue runner stopped ({code}) and the run goes on on the compute host; start the queue runner again to pick it up"
        ),
    };
    let progress = Progress {
        summary: Some(drain.text().render(&line)),
        updated: now(),
        ..job.progress.clone().unwrap_or_default()
    };
    let _ = drain.persist(Action::Progress {
        id: job.id.clone(),
        progress,
    });
}

fn failed(code: &str) -> DrainEnd {
    DrainEnd::Failed {
        code: code.to_owned(),
    }
}

fn unusable(error: QueueFileError) -> DrainEnd {
    failed(DrainError::from(error).code())
}

/// Whether a job is active or waiting.
fn more_to_do(file: &QueueFile) -> Result<bool, DrainError> {
    let queue = file.read()?;
    Ok(active(&queue).is_some() || next_to_start(&queue).is_some())
}

/// Sleeps for `total` in slices, leaving early when `shutdown` is set. A zero `total` does not
/// sleep at all.
fn pause(total: Duration, shutdown: &AtomicBool) {
    let mut left = total;
    while !left.is_zero() && !shutdown.load(Ordering::SeqCst) {
        let slice = left.min(SLICE);
        std::thread::sleep(slice);
        left = left.saturating_sub(slice);
    }
}

/// Starts `exe` with `args` as a process of its own, with no input or output, that goes on after
/// this program and its terminal are gone. Returns its process number. The arguments are not
/// printed or logged.
// The child is meant to outlive this call; the system reaps it when it ends.
#[allow(clippy::zombie_processes)]
pub fn spawn_detached(exe: &Path, args: &[String]) -> std::io::Result<u32> {
    let mut command = Command::new(exe);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS (0x8) and CREATE_NEW_PROCESS_GROUP (0x200), as the system documents
        // them, written as numbers so no new dependency is needed.
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    let child = command.spawn()?;
    Ok(child.id())
}
