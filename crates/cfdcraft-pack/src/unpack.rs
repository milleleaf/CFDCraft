use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use cfdcraft_doc::{
    ARTIFACTS_MEMBER, Document, Link, MANIFEST_MEMBER, STUDY_MEMBER, THUMBNAIL_MEMBER,
    read_document, save_new, store_dir,
};
use cfdcraft_store::{ContentHash, Store, StoreError};
use zip::{CompressionMethod, ZipArchive, read::ZipFile, result::ZipError};

use crate::{DEFAULT_MAX_BYTES, PackError, blob_entry, distinct_blobs, within_bytes, zip_error};

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
/// is written to the store. The study file is created last and exclusively, so a file that appears
/// at the target meanwhile is never replaced (the error is `DocError::TargetExists`), and a failed
/// unpack never leaves a study that points at missing results. Results stored before the failure
/// stay in the store; they are verified and harmless. A store folder that this call created and
/// that holds nothing is removed again; a folder that was there before, or that holds anything,
/// is never touched.
///
/// The results may add up to [`DEFAULT_MAX_BYTES`] at most; [`unpack_limited`] takes another limit.
pub fn unpack(packed: &Path, dir: &Path) -> Result<UnpackReport, PackError> {
    unpack_limited(packed, dir, DEFAULT_MAX_BYTES)
}

/// [`unpack`] with a limit on the bytes of results the pack may carry. The sizes the pack's index
/// records are checked against `max_bytes` before anything is written, so a pack that claims
/// terabytes is refused (`PackError::OverLimit`) instead of being streamed to disk until it is full.
pub fn unpack_limited(
    packed: &Path,
    dir: &Path,
    max_bytes: u64,
) -> Result<UnpackReport, PackError> {
    let target = dir.join(format!("{}.cfdcraft", stem_of(packed)?));
    if fs::symlink_metadata(&target).is_ok() {
        return Err(PackError::TargetExists(target));
    }

    let mut reader = BufReader::new(File::open(packed)?);
    let document = read_document(&mut reader)?.document;
    let wanted = distinct_blobs(&document.artifacts)?;
    within_bytes(&wanted, max_bytes)?;
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

    let store_existed = fs::symlink_metadata(store_dir(&target)).is_ok();
    let written = store_and_save(&mut archive, &document, &wanted, &target);
    if written.is_err() && !store_existed {
        // Only empty folders go: a result already stored, by this call or by an unpack running at
        // the same time, keeps its folder. Failures here are ignored on purpose.
        let store = store_dir(&target);
        let _ = fs::remove_dir(store.join("blobs"));
        let _ = fs::remove_dir(&store);
    }
    written?;
    Ok(UnpackReport {
        blobs: wanted.len(),
        ignored_entries,
        target,
        links: document.artifacts.links,
    })
}

/// The second pass and the save: every result goes through the store's verifying write, read no
/// further than its recorded size plus one byte, and the study file is created last.
fn store_and_save(
    archive: &mut Archive,
    document: &Document,
    wanted: &[(ContentHash, u64)],
    target: &Path,
) -> Result<(), PackError> {
    if !wanted.is_empty() {
        let store = Store::create(store_dir(target))?;
        for &(hash, size) in wanted {
            let entry = find(archive, &hash)?;
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
    save_new(document, target)?;
    Ok(())
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
