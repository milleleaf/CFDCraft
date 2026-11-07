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

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use cfdcraft_doc::{ArtifactIndex, DocError};
use cfdcraft_store::{ContentHash, StoreError};
use thiserror::Error;
use zip::result::ZipError;

pub use pack::{PackReport, pack};
pub use unpack::{UnpackReport, unpack};

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

/// Each distinct result of the index once, in index order, with its recorded size. A hash listed
/// twice with two sizes cannot both be right and is refused.
pub(crate) fn distinct_blobs(index: &ArtifactIndex) -> Result<Vec<(ContentHash, u64)>, PackError> {
    let mut seen = HashSet::new();
    let mut out: Vec<(ContentHash, u64)> = Vec::new();
    for blob in &index.blobs {
        if seen.insert(blob.hash) {
            out.push((blob.hash, blob.size));
        } else if out.iter().any(|&(h, s)| h == blob.hash && s != blob.size) {
            return Err(PackError::SizeMismatch(blob.hash));
        }
    }
    Ok(out)
}
