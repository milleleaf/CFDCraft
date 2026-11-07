//! The queue of a document on disk: `queue.json` inside the document's store folder, replaced
//! whole under an advisory lock, so the command line, the tool server and the background runner
//! can all change it without corrupting it or losing each other's moves.
//!
//! Three files sit in the store folder. `queue.json` is the queue itself. `queue.lock` is held,
//! exclusively, for the whole of every read-change-write, and for no longer. `drain.lock` is held
//! by the one background runner of the document for as long as it runs; a second runner finds it
//! taken and says so. All locks are advisory, belong to the open file and go with it when the
//! process ends, so a crashed process never leaves one behind. `live/<job>/` holds what a running
//! job mirrors for people to read.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use cfdcraft_doc::store_dir;
use cfdcraft_queue::{Action, MAX_BYTES, Queue, QueueError, apply};
use thiserror::Error;

const QUEUE: &str = "queue.json";
const QUEUE_LOCK: &str = "queue.lock";
const DRAIN_LOCK: &str = "drain.lock";
const LIVE: &str = "live";

/// How long a change waits between two tries for the lock, and how many tries it makes: 5 seconds.
const POLL: Duration = Duration::from_millis(20);
const POLLS: u32 = 250;

/// Why the queue file could not be read or changed.
#[derive(Debug, Error)]
pub enum QueueFileError {
    /// The file breaks a rule of the queue, or a change asked for is not allowed.
    #[error("{source}")]
    Queue {
        #[from]
        source: QueueError,
    },
    /// Another process held the lock for the whole wait.
    #[error("the queue is being changed by another process; try again in a moment")]
    Busy,
    /// A file could not be read or written.
    #[error("the queue file could not be used: {reason}")]
    Io { reason: String },
}

fn io_error(error: &io::Error) -> QueueFileError {
    QueueFileError::Io {
        reason: error.to_string(),
    }
}

/// The queue files of one document.
#[derive(Debug, Clone)]
pub struct QueueFile {
    dir: PathBuf,
}

/// The right to run the queue of a document. The runner holds it for as long as it lives; the
/// lock is released when this value is dropped or the process ends.
#[derive(Debug)]
pub struct DrainLock {
    _held: File,
}

impl QueueFile {
    /// The queue files of the document at `doc`, which are in its store folder.
    pub fn new(doc: &Path) -> Self {
        Self {
            dir: store_dir(doc),
        }
    }

    /// The queue as stored. A missing file is an empty queue; a file that is damaged, too large,
    /// newer than this build or against the rules is refused by name.
    pub fn read(&self) -> Result<Queue, QueueFileError> {
        let file = match File::open(self.dir.join(QUEUE)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Queue::new()),
            Err(e) => return Err(io_error(&e)),
        };
        // One byte over the limit is enough for the queue to refuse the file by its size.
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| io_error(&e))?;
        let text = String::from_utf8(bytes).map_err(|_| QueueError::Damaged {
            reason: "the file is not text".to_owned(),
        })?;
        Ok(Queue::from_json(&text)?)
    }

    /// Changes the queue under `queue.lock`: reads it, asks `f` for the moves to make, applies
    /// them in order and replaces `queue.json` with the result in one crash-safe step. Nothing is
    /// written when `f` or a move fails, or when `f` asks for no move; the file is then exactly as
    /// it was. Waits up to 5 seconds for another process to finish its change.
    pub fn update(
        &self,
        f: impl FnOnce(&Queue) -> Result<Vec<Action>, QueueError>,
    ) -> Result<Queue, QueueFileError> {
        fs::create_dir_all(&self.dir).map_err(|e| io_error(&e))?;
        let lock = open_lock(&self.dir.join(QUEUE_LOCK))?;
        wait_for(&lock)?;
        let current = self.read()?;
        let moves = f(&current)?;
        if moves.is_empty() {
            return Ok(current);
        }
        let mut next = current;
        for action in moves {
            next = apply(&next, action)?;
        }
        craft_atomic::atomic_write(&self.dir.join(QUEUE), next.to_json().as_bytes())
            .map_err(|e| io_error(&e))?;
        Ok(next)
    }

    /// Takes the right to run this queue: `Some` when it is free, `None` when another runner of
    /// this document holds it.
    pub fn drain_lock(&self) -> Result<Option<DrainLock>, QueueFileError> {
        fs::create_dir_all(&self.dir).map_err(|e| io_error(&e))?;
        let file = open_lock(&self.dir.join(DRAIN_LOCK))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(DrainLock { _held: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(io_error(&e)),
        }
    }

    /// Whether a runner holds the right to run this queue now. It makes nothing: a document that
    /// never had a runner answers no.
    pub fn drain_running(&self) -> bool {
        let Ok(file) = File::open(self.dir.join(DRAIN_LOCK)) else {
            return false;
        };
        matches!(file.try_lock(), Err(TryLockError::WouldBlock))
    }

    /// The folder a running job mirrors its files into, for a job id of the form `q-` and digits;
    /// any other text names no folder.
    pub fn live_dir(&self, job: &str) -> Option<PathBuf> {
        let digits = job.strip_prefix("q-")?;
        let plain = (1..=10).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit());
        plain.then(|| self.dir.join(LIVE).join(job))
    }
}

fn open_lock(path: &Path) -> Result<File, QueueFileError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| io_error(&e))
}

fn wait_for(file: &File) -> Result<(), QueueFileError> {
    for _ in 0..POLLS {
        match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) => thread::sleep(POLL),
            Err(TryLockError::Error(e)) => return Err(io_error(&e)),
        }
    }
    Err(QueueFileError::Busy)
}
