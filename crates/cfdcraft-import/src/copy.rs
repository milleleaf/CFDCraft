//! Writing the stored copies: every listed file goes into `original/`, every file that is not a
//! script goes into `work/`, and the files of `original/` are then made read-only.

use crate::inventory::{Cache, Walk, is_script, read_capped};
use crate::record::FileEntry;
use crate::{ImportError, ImportOptions};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What was stored.
pub(crate) struct Copied {
    pub files: Vec<FileEntry>,
    pub scripts: Vec<String>,
}

pub(crate) fn write_err(e: io::Error) -> ImportError {
    ImportError::Write {
        reason: e.to_string(),
    }
}

/// Creates the folders above `path`, tells the hook, and writes the new file through the crash-safe
/// writer, which refuses to replace anything.
pub(crate) fn put(
    path: &Path,
    bytes: &[u8],
    hook: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> Result<(), ImportError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(write_err)?;
    }
    hook(path).map_err(write_err)?;
    craft_atomic::write_new(path, bytes).map_err(write_err)
}

fn make_read_only(path: &Path) -> io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)
}

/// Copies every walked file. Each file is read once (or taken from the cache the inventory kept),
/// hashed, and the same bytes are written to both copies.
pub(crate) fn copy_all(
    found: &Walk,
    cache: &Cache,
    target: &Path,
    options: &ImportOptions,
    hook: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> Result<Copied, ImportError> {
    let original = target.join("original");
    let work = target.join("work");
    fs::create_dir_all(&original).map_err(write_err)?;
    fs::create_dir_all(&work).map_err(write_err)?;
    let mut files = Vec::new();
    let mut scripts = Vec::new();
    let mut stored: Vec<PathBuf> = Vec::new();
    let mut total = 0u64;
    for (rel, planned) in &found.files {
        let read;
        let bytes: &[u8] = match cache.get(rel) {
            Some(kept) => kept,
            None => {
                read = read_capped(&planned.path, rel, options.max_file_bytes)?;
                &read
            }
        };
        total = total.saturating_add(bytes.len() as u64);
        if total > options.max_total_bytes {
            return Err(ImportError::Cap {
                what: "total_bytes",
                limit: options.max_total_bytes,
                at: rel.clone(),
            });
        }
        let dest = original.join(rel);
        put(&dest, bytes, hook)?;
        stored.push(dest);
        let script = is_script(rel);
        if script {
            scripts.push(rel.clone());
        } else {
            put(&work.join(rel), bytes, hook)?;
        }
        files.push(FileEntry {
            rel_path: rel.clone(),
            bytes: bytes.len() as u64,
            blake3: blake3::hash(bytes).to_hex().to_string(),
        });
    }
    for path in &stored {
        make_read_only(path).map_err(write_err)?;
    }
    Ok(Copied { files, scripts })
}
