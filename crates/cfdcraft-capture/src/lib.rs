//! Turn the folder a finished run left behind into the blobs and key numbers a run record names.
//!
//! Two bundles come out of a results folder: the mesh (the five `polyMesh` files) and the result
//! (final fields, centreline samples, the three logs and the monitor files). A bundle is a ZIP with sorted entries,
//! stored without compression, with a fixed time and fixed permissions, so equal files always give
//! equal bytes and an equal hash. The key numbers (iteration count, last initial residuals and
//! every centreline sample) are read from the same files for comparison.
//!
//! The folder came from a compute host, so nothing in it is trusted: every file is read without
//! following links, a link or a folder where a file belongs is refused, and each file is capped in
//! size. A refusal names the file and nothing is stored. The transfer archives a run leaves beside
//! its output are never read.
//!
//! Ceilings: a bundle is built in memory, so a file may hold at most [`FILE_CAP`] bytes and a bundle
//! at most [`BUNDLE_CAP`] bytes in all. The check that a path is not a link and the read that
//! follows are two steps; a writer that swaps a file in between is not defended against, because
//! the folder belongs to the process that reads it.

mod bundle;
#[doc(hidden)]
pub mod fixture;
mod monitors;
mod numbers;
mod result;

pub use bundle::{Bundle, entry_names, mesh_bundle, mesh_bundle_limited, read_entry, store_bundle};
pub use monitors::{
    MAX_MONITOR_FILES, checkpoint_bundle, latest_time_dir, monitor_names, monitor_names_limited,
};
pub use numbers::key_numbers;
pub use result::{find_time_dir, result_bundle, result_bundle_limited};

/// The most bytes one file of a results folder may hold.
pub const FILE_CAP: u64 = 512 * 1024 * 1024;
/// The most bytes all the files of one bundle may hold together.
pub const BUNDLE_CAP: u64 = 1024 * 1024 * 1024;

/// Why a results folder could not be captured. Messages name the file relative to the folder.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("{0} is missing from the results")]
    Missing(String),
    #[error("{0} is a link or is not the plain file or folder it should be, so it was not read")]
    NotAFile(String),
    #[error("{0} is larger than the limit, so nothing was stored")]
    TooLarge(String),
    #[error("{0}")]
    Invalid(String),
    #[error("the bundle could not be written or read: {0}")]
    Zip(String),
    #[error("cannot read the results: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot store the bundle: {0}")]
    Store(#[from] cfdcraft_store::StoreError),
}
