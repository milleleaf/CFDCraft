//! The local copy of what a running job writes: its logs and monitor files, under
//! `live/<job>/` in the document's store folder.
//!
//! Files grow only by appending, and a file is only ever appended with whole lines, so the length
//! of a mirror file is the offset to resume from: no byte is read twice or skipped, whatever the
//! size of the pieces. A job's mirror holds at most [`MAX_BYTES`] in all.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use cfdcraft_remote::valid_rel;

/// The most bytes the mirrored files of one job may hold together.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// The folder under the live folder that holds the monitor files.
const POST: &str = "postProcessing";

/// Why a mirror file could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MirrorError {
    /// The name is not a plain relative path.
    #[error("the file name \"{rel}\" is not allowed in the live view")]
    BadName { rel: String },
    /// The mirror of the job is at its limit.
    #[error("the live view of this job is full")]
    Full,
    /// A file could not be read or written.
    #[error("the live view could not be used: {reason}")]
    Io { reason: String },
}

fn io_error(error: &std::io::Error) -> MirrorError {
    MirrorError::Io {
        reason: error.to_string(),
    }
}

/// The mirror files of one job.
#[derive(Debug, Clone)]
pub struct Mirror {
    dir: PathBuf,
}

impl Mirror {
    /// The mirror in `dir`. Nothing is made until a file is appended.
    pub fn open(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
        }
    }

    /// The folder of the mirror.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, rel: &str) -> Result<PathBuf, MirrorError> {
        if valid_rel(rel) {
            Ok(self.dir.join(rel))
        } else {
            Err(MirrorError::BadName {
                rel: rel.chars().take(40).collect(),
            })
        }
    }

    /// The length of a mirror file in bytes, which is the offset to resume from; 0 for a file that
    /// is not there, is not a plain file or has a name that is not allowed.
    pub fn len(&self, rel: &str) -> u64 {
        let Ok(path) = self.path(rel) else {
            return 0;
        };
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_file() => meta.len(),
            _ => 0,
        }
    }

    /// Adds `bytes` to the end of a mirror file, making it and its folders when needed. Refused
    /// when the mirror would pass its limit.
    pub fn append(&self, rel: &str, bytes: &[u8]) -> Result<(), MirrorError> {
        let path = self.path(rel)?;
        if self.total().saturating_add(bytes.len() as u64) > MAX_BYTES {
            return Err(MirrorError::Full);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(&e))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| io_error(&e))?;
        file.write_all(bytes).map_err(|e| io_error(&e))
    }

    /// Empties a mirror file so it can be filled again from the start. A file that is not there is
    /// left alone.
    pub fn reset(&self, rel: &str) -> Result<(), MirrorError> {
        let path = self.path(rel)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_error(&e)),
        }
    }

    /// The text of a mirror file; empty for a file that is not there. Bytes that are not text
    /// are replaced.
    pub fn read_to_string(&self, rel: &str) -> Result<String, MirrorError> {
        let path = self.path(rel)?;
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
            Err(e) => return Err(io_error(&e)),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| io_error(&e))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// The names of the mirror files, sorted: the logs at the top of the folder and every file
    /// below `postProcessing`. Nothing is followed through a link.
    pub fn files(&self) -> Vec<String> {
        let mut found = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
                if is_file && name.starts_with("log.") && valid_rel(&name) {
                    found.push(name);
                }
            }
        }
        walk(&self.dir.join(POST), POST, &mut found);
        found.sort();
        found
    }

    /// The bytes the mirror files hold together.
    pub fn total(&self) -> u64 {
        self.files().iter().map(|rel| self.len(rel)).sum()
    }
}

/// Adds every plain file below `dir`, named from `prefix`, to `found`.
fn walk(dir: &Path, prefix: &str, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}/{name}");
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_file() && valid_rel(&rel) {
            found.push(rel);
        } else if kind.is_dir() {
            walk(&entry.path(), &rel, found);
        }
    }
}
