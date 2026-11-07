use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use cfdcraft_doc::{Document, Link, open, store_dir, write_members};
use cfdcraft_store::{ContentHash, Store};
use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

use crate::{DEFAULT_MAX_BYTES, PackError, blob_entry, distinct_blobs, within_bytes, zip_error};

/// What a pack holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackReport {
    /// Distinct results written into the pack.
    pub blobs: usize,
    /// Total size of those results in bytes.
    pub bytes: u64,
    /// Linked files the study lists. The pack does not carry them.
    pub links_not_included: Vec<Link>,
}

/// What a pack leaves out of the copy it writes. The default leaves nothing out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PackOptions {
    /// Clear the host label of every run record in the pack. The study file the pack is made from
    /// is never touched.
    pub redact_host: bool,
}

/// Write the study at `doc_path` and the results its index lists to `out` as one file, with every
/// run record and its host label. [`pack_with`] takes options.
///
/// Everything is checked before anything is written: each listed result must be in the store with
/// the recorded size. The bytes are hashed again as they are copied. The file is written through
/// the shared exclusive create, so a failure leaves no output file, and a file already at `out`,
/// even one that appears while the pack is being written, is never replaced
/// ([`PackError::TargetExists`]).
pub fn pack(doc_path: &Path, out: &Path) -> Result<PackReport, PackError> {
    pack_with(doc_path, out, &PackOptions::default())
}

/// [`pack`] with options. With `redact_host` set, the copy of the run records that goes into the
/// pack has no host label; the study file on disk keeps its labels.
pub fn pack_with(
    doc_path: &Path,
    out: &Path,
    options: &PackOptions,
) -> Result<PackReport, PackError> {
    if out == doc_path {
        return Err(PackError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the pack must be written to a different file than the study",
        )));
    }
    let mut document = open(doc_path)?.document;
    if options.redact_host {
        document.artifacts.redact_hosts();
    }
    let wanted = distinct_blobs(&document.artifacts)?;
    // Never write a pack that unpacking would refuse.
    within_bytes(&wanted, DEFAULT_MAX_BYTES)?;
    let store = if wanted.is_empty() {
        None
    } else {
        Some(Store::open(store_dir(doc_path))?)
    };
    if let Some(store) = &store {
        for &(hash, size) in &wanted {
            if !store.contains(&hash) {
                return Err(PackError::Missing(hash));
            }
            if fs::metadata(store.blob_path(&hash))?.len() != size {
                return Err(PackError::SizeMismatch(hash));
            }
        }
    }

    // The closure can only return an io error; the typed one travels out in this slot.
    let mut failure = None;
    let written = craft_atomic::write_new_with(out, |file| {
        write_pack(&document, store.as_ref(), &wanted, file).map_err(|error| {
            failure = Some(error);
            io::Error::other("the pack could not be written")
        })
    });
    if let Err(error) = written {
        return Err(failure.unwrap_or_else(|| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                PackError::TargetExists(out.to_owned())
            } else {
                PackError::Io(error)
            }
        }));
    }

    Ok(PackReport {
        blobs: wanted.len(),
        bytes: wanted
            .iter()
            .fold(0u64, |total, &(_, size)| total.saturating_add(size)),
        links_not_included: document.artifacts.links,
    })
}

fn write_pack(
    document: &Document,
    store: Option<&Store>,
    wanted: &[(ContentHash, u64)],
    file: &mut File,
) -> Result<(), PackError> {
    let mut zip = ZipWriter::new(file);
    write_members(document, &mut zip)?;
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    if let Some(store) = store {
        for &(hash, size) in wanted {
            zip.start_file(
                blob_entry(&hash),
                stored.large_file(size >= u64::from(u32::MAX)),
            )
            .map_err(zip_error)?;
            copy_checked(store.reader(hash)?, &mut zip, hash, size)?;
        }
    }
    zip.finish().map_err(zip_error)?;
    Ok(())
}

/// Copy a stored result into the archive, hashing it on the way. Reads stop one byte past the
/// recorded size, so a result that grew cannot be copied whole.
fn copy_checked(
    from: impl Read,
    to: &mut impl Write,
    hash: ContentHash,
    size: u64,
) -> Result<(), PackError> {
    let mut from = from.take(size.saturating_add(1));
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut len = 0u64;
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        hasher.update(&buf[..n]);
        to.write_all(&buf[..n])?;
        len += n as u64;
    }
    if len != size {
        return Err(PackError::SizeMismatch(hash));
    }
    if hash.to_string() != hasher.finalize().to_hex().as_str() {
        return Err(PackError::Corrupt(hash));
    }
    Ok(())
}
