use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use cfdcraft_doc::{
    ARTIFACTS_MEMBER, Link, MANIFEST_MEMBER, STUDY_MEMBER, THUMBNAIL_MEMBER, read_document, save,
    store_dir,
};
use cfdcraft_store::{ContentHash, Store, StoreError};
use zip::{CompressionMethod, ZipArchive, read::ZipFile, result::ZipError};

use crate::{PackError, blob_entry, distinct_blobs, zip_error};

type Archive = ZipArchive<BufReader<File>>;

/// What an unpack wrote and what it left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpackReport {
    /// Distinct results the pack carried; each is in the store and verified.
    pub blobs: usize,
    /// Entries of the pack that were neither a study member nor a listed result. None was read.
    pub ignored_entries: usize,
    /// The study file that was written.
    pub target: PathBuf,
    /// Linked files the study lists. The pack did not carry them.
    pub links: Vec<Link>,
}

/// Unpack `packed` into `dir`: the study file `<stem>.cfdcraft` and its store `<stem>.cfdstore`,
/// where the stem is the packed file's name without a trailing `.cfdpack` or `.cfdcraft`.
///
/// An existing study file of that name is refused before anything is touched. Every listed result
/// must be in the pack, stored uncompressed and of the recorded size; each is then hashed while it
/// is written to the store. The study file is saved last, so a failed unpack never leaves a study
/// that points at missing results (results stored before the failure stay in the store; they are
/// verified and harmless).
pub fn unpack(packed: &Path, dir: &Path) -> Result<UnpackReport, PackError> {
    let target = dir.join(format!("{}.cfdcraft", stem_of(packed)?));
    if fs::symlink_metadata(&target).is_ok() {
        return Err(PackError::TargetExists(target));
    }

    let mut reader = BufReader::new(File::open(packed)?);
    let document = read_document(&mut reader)?.document;
    let wanted = distinct_blobs(&document.artifacts)?;
    let mut archive = ZipArchive::new(reader).map_err(zip_error)?;

    // First pass: the whole index against the archive, before anything is written.
    for &(hash, size) in &wanted {
        let entry = find(&mut archive, &hash)?;
        if entry.compression() != CompressionMethod::Stored {
            return Err(PackError::EntryNotStored(hash));
        }
        if entry.size() != size {
            return Err(PackError::SizeMismatch(hash));
        }
    }

    let known: HashSet<Vec<u8>> = [
        MANIFEST_MEMBER,
        STUDY_MEMBER,
        ARTIFACTS_MEMBER,
        THUMBNAIL_MEMBER,
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(wanted.iter().map(|(hash, _)| blob_entry(hash)))
    .map(String::into_bytes)
    .collect();
    let ignored_entries = archive
        .file_names_raw()
        .filter(|name| !known.contains(*name))
        .count();

    fs::create_dir_all(dir)?;

    // Second pass: every result goes through the store's verifying write, read no further than
    // its recorded size plus one byte.
    if !wanted.is_empty() {
        let store = Store::create(store_dir(&target))?;
        for &(hash, size) in &wanted {
            let entry = find(&mut archive, &hash)?;
            store
                .put_reader_verified(hash, entry.take(size.saturating_add(1)))
                .map_err(|error| match error {
                    StoreError::HashMismatch { .. } => PackError::Damaged(hash),
                    StoreError::Io(e) if e.kind() == io::ErrorKind::InvalidData => {
                        PackError::Damaged(hash)
                    }
                    other => PackError::Store(other),
                })?;
        }
    }

    save(&document, &target)?;
    Ok(UnpackReport {
        blobs: wanted.len(),
        ignored_entries,
        target,
        links: document.artifacts.links,
    })
}

/// The entry for a result, looked up by its exact name; the name is never a path.
fn find<'a>(
    archive: &'a mut Archive,
    hash: &ContentHash,
) -> Result<ZipFile<'a, BufReader<File>>, PackError> {
    match archive.by_name(&blob_entry(hash)) {
        Ok(entry) => Ok(entry),
        Err(ZipError::FileNotFound) => Err(PackError::Incomplete(*hash)),
        Err(other) => Err(zip_error(other)),
    }
}

fn stem_of(packed: &Path) -> Result<String, PackError> {
    let name = packed.file_name().map(|n| n.to_string_lossy().into_owned());
    let stem = name.as_deref().map(|name| {
        name.strip_suffix(".cfdpack")
            .or_else(|| name.strip_suffix(".cfdcraft"))
            .unwrap_or(name)
    });
    match stem {
        Some(stem) if !stem.is_empty() => Ok(stem.to_owned()),
        _ => Err(PackError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the packed file has no usable name",
        ))),
    }
}
