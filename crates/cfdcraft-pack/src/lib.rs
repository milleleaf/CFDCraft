//! Packing a study with its stored results into one file, and unpacking it elsewhere.
//!
//! # Layout
//!
//! A pack is itself a valid study file. It holds the four ordinary members of a study file (see
//! the document crate) and then, for each distinct result the artifact index lists, one entry
//! named `blobs/` followed by the 64 lower-case hexadecimal characters of the result's hash. Result
//! entries are stored without compression, in index order, so the same inputs always give the same
//! bytes. An ordinary open reads the document and ignores the result entries.
//!
//! # Pruned results
//!
//! A result the index lists as pruned was removed from the store on purpose. A pack leaves it out
//! and unpacking does not look for it; the index still lists it as pruned, so the unpacked study
//! reads the same as the packed one. Only a result that is listed, not pruned and not in the store
//! is an error.
//!
//! # Run records
//!
//! The run records of the artifact index travel with the pack, each with the label of the host it
//! ran on. [`PackOptions::redact_host`] clears those labels in the pack's copy; the study file the
//! pack is made from keeps them.
//!
//! # Order
//!
//! Unpacking checks the index against the archive first, then stores every result (each is hashed
//! while it is written), and saves the document last. A damaged or incomplete pack therefore never
//! leaves a study that points at missing results. An existing study file of the same name is never
//! replaced.
//!
//! # Untrusted input
//!
//! Entry names are never used as paths. Only entries called `blobs/<hash>` for a hash the index
//! lists are looked up; they must be stored uncompressed, their declared size must equal the size
//! the index records, and each is read no further than that size plus one byte. Every other entry
//! is ignored and counted.
//!
//! # Links
//!
//! Linked input files are not carried. The reports name the links that were left out, so they can
//! be copied beside the unpacked study.

mod pack;
mod unpack;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;

use cfdcraft_doc::{ArtifactIndex, DocError};
use cfdcraft_store::{ContentHash, StoreError};
use thiserror::Error;
use zip::result::ZipError;

pub use pack::{PackOptions, PackReport, pack, pack_with};
pub use unpack::{UnpackReport, unpack, unpack_limited};

/// Most distinct stored results one pack carries. A study lists a handful; the cap only stops an
/// index crafted to be as long as the size limit of the index allows.
pub const MAX_RESULTS: usize = 10_000;

/// Most bytes of stored results a pack carries and [`unpack`] accepts. The sizes come from the
/// index of the file, which cannot be trusted; [`unpack_limited`] takes another limit.
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// Why a pack could not be written or unpacked.
#[derive(Debug, Error)]
pub enum PackError {
    /// The study file itself could not be read or written.
    #[error(transparent)]
    Doc(#[from] DocError),
    /// The result store refused something.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A result the study lists is not in the store.
    #[error("the result {0} is listed by the study but is not in the store")]
    Missing(ContentHash),
    /// A stored result no longer matches its hash.
    #[error("the stored result {0} does not match its hash; it is damaged")]
    Corrupt(ContentHash),
    /// A result is not the size the study records.
    #[error("the result {0} is not the size the study records")]
    SizeMismatch(ContentHash),
    /// The study file to create already exists.
    #[error("{} already exists; nothing was written", .0.display())]
    TargetExists(PathBuf),
    /// The pack lacks a result its index lists.
    #[error("the pack does not contain the result {0}")]
    Incomplete(ContentHash),
    /// A result in the pack does not match its hash.
    #[error("the result {0} in the pack does not match its hash; the pack is damaged")]
    Damaged(ContentHash),
    /// A result in the pack is compressed.
    #[error("the result {0} in the pack is not stored as plain bytes")]
    EntryNotStored(ContentHash),
    /// The study lists more stored results, or more bytes of them, than a pack carries.
    #[error("the study lists more stored results than a pack carries: {0}")]
    OverLimit(String),
    /// The disk refused a read or a write.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The ZIP layer reported a problem.
    #[error("the pack could not be read or written: {0}")]
    Zip(String),
}

pub(crate) fn zip_error(error: ZipError) -> PackError {
    match error {
        ZipError::Io(error) => PackError::Io(error),
        other => PackError::Zip(other.to_string()),
    }
}

/// The entry that holds the result `hash`.
pub(crate) fn blob_entry(hash: &ContentHash) -> String {
    format!("blobs/{hash}")
}

/// Each distinct result of the index that is still stored once, in index order, with its recorded
/// size; a result the index lists as pruned is left out. A hash listed twice with two sizes cannot
/// both be right and is refused, and so is an index with more than [`MAX_RESULTS`] distinct
/// results. One pass over the index, whatever it repeats.
pub(crate) fn distinct_blobs(index: &ArtifactIndex) -> Result<Vec<(ContentHash, u64)>, PackError> {
    let pruned: HashSet<ContentHash> = index.pruned.iter().copied().collect();
    let mut first_size: HashMap<ContentHash, u64> = HashMap::new();
    let mut out: Vec<(ContentHash, u64)> = Vec::new();
    for blob in index.blobs.iter().filter(|b| !pruned.contains(&b.hash)) {
        match first_size.get(&blob.hash) {
            Some(&size) if size != blob.size => return Err(PackError::SizeMismatch(blob.hash)),
            Some(_) => {}
            None => {
                if out.len() >= MAX_RESULTS {
                    return Err(PackError::OverLimit(format!(
                        "more than {MAX_RESULTS} distinct results"
                    )));
                }
                first_size.insert(blob.hash, blob.size);
                out.push((blob.hash, blob.size));
            }
        }
    }
    Ok(out)
}

/// Refuse results that add up to more than `max` bytes.
pub(crate) fn within_bytes(wanted: &[(ContentHash, u64)], max: u64) -> Result<(), PackError> {
    let total = wanted
        .iter()
        .fold(0u64, |total, &(_, size)| total.saturating_add(size));
    if total > max {
        return Err(PackError::OverLimit(format!(
            "the results add up to more than {max} bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use cfdcraft_doc::{BlobKind, BlobRef};

    use super::*;

    fn blob(n: u32, size: u64) -> BlobRef {
        BlobRef {
            hash: ContentHash::of(&n.to_le_bytes()),
            size,
            kind: BlobKind::Result,
        }
    }

    fn index(blobs: Vec<BlobRef>) -> ArtifactIndex {
        ArtifactIndex {
            blobs,
            ..ArtifactIndex::default()
        }
    }

    #[test]
    fn a_long_index_of_repeats_is_read_in_one_pass() {
        // The old scan cost about 10 000 comparisons per repeat: tens of billions here.
        let mut blobs: Vec<BlobRef> = (0..9_900).map(|n| blob(n, 1)).collect();
        blobs.extend((0..500_000).map(|_| blob(9_899, 1)));
        let started = Instant::now();
        let distinct = distinct_blobs(&index(blobs)).unwrap();
        assert_eq!(distinct.len(), 9_900);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_number_of_distinct_results_is_bounded_and_repeats_are_not_counted() {
        let at_limit: Vec<BlobRef> = (0..MAX_RESULTS as u32).map(|n| blob(n, 1)).collect();
        let mut with_repeat = at_limit.clone();
        with_repeat.push(blob(0, 1));
        assert_eq!(
            distinct_blobs(&index(with_repeat)).unwrap().len(),
            MAX_RESULTS
        );

        let mut over = at_limit;
        over.push(blob(MAX_RESULTS as u32, 1));
        assert!(matches!(
            distinct_blobs(&index(over)),
            Err(PackError::OverLimit(_))
        ));
    }

    #[test]
    fn one_hash_with_two_sizes_is_still_refused() {
        let blobs = vec![blob(1, 5), blob(2, 5), blob(1, 6)];
        assert!(matches!(
            distinct_blobs(&index(blobs)),
            Err(PackError::SizeMismatch(h)) if h == ContentHash::of(&1u32.to_le_bytes())
        ));
    }

    #[test]
    fn the_byte_limit_is_on_the_sum_and_cannot_overflow() {
        let wanted = vec![blob(1, u64::MAX), blob(2, u64::MAX)]
            .into_iter()
            .map(|b| (b.hash, b.size))
            .collect::<Vec<_>>();
        assert!(within_bytes(&wanted, u64::MAX).is_ok());
        assert!(matches!(
            within_bytes(&wanted, u64::MAX - 1),
            Err(PackError::OverLimit(_))
        ));
    }
}
