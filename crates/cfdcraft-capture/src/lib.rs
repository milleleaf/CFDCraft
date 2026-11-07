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

pub use bundle::{
    Bundle, entry_names, mesh_bundle, mesh_bundle_limited, mesh_bundle_limited_with,
    mesh_bundle_with, read_entry, store_bundle,
};
pub use monitors::{
    MAX_MONITOR_FILES, checkpoint_bundle, checkpoint_bundle_with, latest_time_dir, monitor_names,
    monitor_names_limited,
};
pub use numbers::{key_numbers, key_numbers_with};
pub use result::{
    find_time_dir, result_bundle, result_bundle_limited, result_bundle_limited_with,
    result_bundle_with,
};

/// The five files of an OpenFOAM mesh, relative to the results folder, in bundle order.
pub const POLY_MESH_FILES: [&str; 5] = [
    "constant/polyMesh/boundary",
    "constant/polyMesh/faces",
    "constant/polyMesh/neighbour",
    "constant/polyMesh/owner",
    "constant/polyMesh/points",
];

/// What a solver run leaves that is worth keeping: the name of its log, the fields it ends with,
/// whether it samples the cavity centrelines, and the fields whose last initial residual becomes a
/// key number. The caller builds it from the solver it ran. An OpenFOAM run keeps its fields in
/// the end-time folder ([`CapturePlan::openfoam`]); any other solver names plain files relative to
/// the results folder ([`CapturePlan::files`]).
#[derive(Debug, Clone, PartialEq)]
pub struct CapturePlan {
    /// The solver log, relative to the results folder.
    pub log: String,
    /// The fields kept, in bundle order: relative to the end-time folder when `time_folder` is
    /// true, to the results folder otherwise.
    pub fields: Vec<String>,
    /// Whether the two centreline sample files are required and read.
    pub centreline: bool,
    /// The names that follow `Solving for ` in the log, one residual each.
    pub residual_fields: Vec<String>,
    /// Whether the fields are in the end-time folder (the OpenFOAM layout) and the run leaves the
    /// monitor files, `phi` and a potential-flow log beside them.
    pub time_folder: bool,
    /// The other logs kept with the result beside `log`.
    pub logs: Vec<String>,
    /// The files of the mesh bundle, relative to the results folder.
    pub mesh: Vec<String>,
}

impl CapturePlan {
    /// The lid-driven cavity run: `log.simpleFoam`, fields `U` and `p`, centrelines, residuals
    /// of `Ux`, `Uy` and `p`.
    pub fn cavity() -> Self {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect();
        Self::openfoam(
            "log.simpleFoam".to_owned(),
            names(&["U", "p"]),
            true,
            names(&["Ux", "Uy", "p"]),
        )
    }

    /// An OpenFOAM run: fields in the end-time folder, the two mesh logs and the five `polyMesh`
    /// files kept with it.
    pub fn openfoam(
        log: String,
        fields: Vec<String>,
        centreline: bool,
        residual_fields: Vec<String>,
    ) -> Self {
        Self {
            log,
            fields,
            centreline,
            residual_fields,
            time_folder: true,
            logs: vec!["log.blockMesh".to_owned(), "log.checkMesh".to_owned()],
            mesh: POLY_MESH_FILES.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    /// A run of any other solver: the files it names, relative to the results folder. It has no
    /// end-time folder, no centreline samples and no log-derived numbers.
    pub fn files(log: String, logs: Vec<String>, fields: Vec<String>, mesh: Vec<String>) -> Self {
        Self {
            log,
            fields,
            centreline: false,
            residual_fields: Vec::new(),
            time_folder: false,
            logs,
            mesh,
        }
    }
}

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
