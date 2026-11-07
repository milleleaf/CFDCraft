use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use cfdcraft_doc::{Document, Link, open, store_dir, write_members};
use cfdcraft_store::{ContentHash, Store};
use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

use crate::{PackError, blob_entry, distinct_blobs, zip_error};

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

/// Write the study at `doc_path` and the results its index lists to `out` as one file.
///
/// Everything is checked before anything is written: each listed result must be in the store with
/// the recorded size. The bytes are hashed again as they are copied. The file is written through
/// the shared crash-safe save, so a failure leaves no output file (and an earlier file at `out`
/// stays as it was).
pub fn pack(doc_path: &Path, out: &Path) -> Result<PackReport, PackError> {
    if out == doc_path {
        return Err(PackError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the pack must be written to a different file than the study",
        )));
    }
    let document = open(doc_path)?.document;
    let wanted = distinct_blobs(&document.artifacts)?;
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
    let written = craft_atomic::atomic_write_with(out, |file| {
        write_pack(&document, store.as_ref(), &wanted, file).map_err(|error| {
            failure = Some(error);
            io::Error::other("the pack could not be written")
        })
    });
    if let Err(error) = written {
        return Err(failure.unwrap_or(PackError::Io(error)));
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
