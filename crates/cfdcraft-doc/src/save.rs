//! Saving and opening a study file on disk.
//!
//! Every write goes through the shared crash-safe save. This module has no staging, renaming or
//! flushing code of its own; the only thing it adds is removing staging files that a crashed save
//! left behind.

use std::fs::{self, File};
use std::io::{self, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use craft_atomic::{AtomicFs, RenameRetry};

use crate::container::{DocError, read_document, to_bytes};
use crate::model::{Document, Opened};

/// A staging file younger than this may belong to another process saving the same file.
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// Write `doc` to `path`, replacing any file already there through the shared crash-safe save.
///
/// A save that fails or is interrupted leaves the previous file as it was. After a successful save,
/// staging files older than an hour that earlier crashed saves left beside the file are removed.
pub fn save(doc: &Document, path: &Path) -> Result<(), DocError> {
    let bytes = to_bytes(doc)?;
    craft_atomic::atomic_write(path, &bytes)?;
    sweep_after_save(path);
    Ok(())
}

/// Write `doc` to `path` only if nothing is there: the shared exclusive create gives the file its
/// name in one step that fails, rather than replaces, when a file has appeared in the meantime, so
/// "never replaced" holds under concurrent callers too. A file already there is refused with
/// [`DocError::TargetExists`] and stays as it was.
pub fn save_new(doc: &Document, path: &Path) -> Result<(), DocError> {
    let bytes = to_bytes(doc)?;
    match craft_atomic::write_new(path, &bytes) {
        Ok(()) => {
            sweep_after_save(path);
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            Err(DocError::TargetExists(path.to_owned()))
        }
        Err(e) => Err(e.into()),
    }
}

/// A fingerprint of the bytes a study file had when it was read, to be handed back to
/// [`save_if_unchanged`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp(blake3::Hash);

fn stamp_of(file: &mut File) -> io::Result<Stamp> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(&mut *file)?;
    Ok(Stamp(hasher.finalize()))
}

/// [`open`], and the stamp of the exact bytes that were read.
pub fn open_stamped(path: &Path) -> Result<(Opened, Stamp), DocError> {
    let mut file = File::open(path)?;
    let stamp = stamp_of(&mut file)?;
    // A save replaces the file under its name and never writes into the one that is open, so the
    // bytes read now are the bytes that were stamped.
    file.seek(SeekFrom::Start(0))?;
    Ok((read_document(BufReader::new(file))?, stamp))
}

/// [`save`], but only if the file at `path` still has the bytes `stamp` was taken from; another
/// save in between is refused with [`DocError::Changed`] and the file stays as it is.
///
/// This closes the lost update of a read, a long piece of work and a save: the check is made after
/// the new file is built, so what is left between it and the replacement is a few steps and not
/// the work. It is not a lock, and two callers that pass the check in the same instant can still
/// both write.
pub fn save_if_unchanged(doc: &Document, path: &Path, stamp: Stamp) -> Result<(), DocError> {
    let bytes = to_bytes(doc)?;
    let now = stamp_of(&mut File::open(path)?)?;
    if now != stamp {
        return Err(DocError::Changed(path.to_owned()));
    }
    craft_atomic::atomic_write(path, &bytes)?;
    sweep_after_save(path);
    Ok(())
}

/// [`save`] through an injected file system.
///
/// This exists for tests that inject failures or stalls at each step of the shared save. Nothing
/// else should call it.
pub fn save_with(doc: &Document, path: &Path, file_system: &dyn AtomicFs) -> Result<(), DocError> {
    let bytes = to_bytes(doc)?;
    craft_atomic::atomic_write_custom(file_system, RenameRetry::platform(), path, &bytes)?;
    sweep_after_save(path);
    Ok(())
}

/// Read the study file at `path`. The file is read through a buffer, never whole into memory.
pub fn open(path: &Path) -> Result<Opened, DocError> {
    read_document(BufReader::new(File::open(path)?))
}

/// The folder that holds the stored results of the study file at `path`: beside it, named after
/// its stem, with the extension `cfdstore`.
pub fn store_dir(doc_path: &Path) -> PathBuf {
    doc_path.with_extension("cfdstore")
}

fn sweep_after_save(path: &Path) {
    // The save has already succeeded; a failed cleanup must not turn it into an error.
    let _ = sweep_stale_staging(path, STALE_AFTER);
}

/// Remove staging files that crashed saves of the file at `doc_path` left beside it, when they are
/// older than `older_than` (a zero duration removes them all). Returns the files removed.
///
/// Only files named the way the shared save names its staging files for this document are
/// touched: the document's own file and every other file stay.
///
/// A document reached through a symbolic link is not covered: the shared save stages beside the
/// file the link points to, not beside the link.
pub fn sweep_stale_staging(doc_path: &Path, older_than: Duration) -> io::Result<Vec<PathBuf>> {
    let Some(name) = doc_path.file_name() else {
        return Ok(Vec::new());
    };
    let Some((prefix, suffix)) = staging_shape(&name.to_string_lossy()) else {
        return Ok(Vec::new());
    };
    let dir = match doc_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut removed = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !is_staging_name(&entry.file_name().to_string_lossy(), &prefix, &suffix) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let old = older_than.is_zero()
            || meta
                .modified()
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age >= older_than);
        if old && fs::remove_file(entry.path()).is_ok() {
            removed.push(entry.path());
        }
    }
    removed.sort();
    Ok(removed)
}

/// What the shared save puts before and after the process-specific middle of a staging file name
/// for the document called `name`. The ending is read from the shared crate's own naming so the
/// two cannot drift apart; when the shape is not what is expected, nothing is recognised.
fn staging_shape(name: &str) -> Option<(String, String)> {
    let stem: String = name.chars().take(64).collect();
    let prefix = format!(".{stem}.");
    let sample = craft_atomic::temp_name(name);
    let (_, ending) = sample.strip_prefix(&prefix)?.rsplit_once('.')?;
    Some((prefix, format!(".{ending}")))
}

/// A staging name is the prefix, a process number, a dash, a counter, then the ending.
fn is_staging_name(file_name: &str, prefix: &str, suffix: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    file_name
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .and_then(|middle| middle.split_once('-'))
        .is_some_and(|(process, counter)| digits(process) && digits(counter))
}
