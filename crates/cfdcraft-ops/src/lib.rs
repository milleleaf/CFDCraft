//! Typed operations over studies and cases. The command line and any other front end are thin
//! adapters over this crate, so every path that writes a case runs the same gates in the same
//! order and reports through the same [`Report`].

pub mod cases;
pub mod documents;
pub mod ghia;
pub mod prepare;
pub mod registry;
pub mod report;
pub mod run;
pub mod workspace;

pub use cases::{check_case, compile_study, export_study};
pub use documents::{
    DocOutcome, check_document_links, link_file, open_document, pack_document, read_study,
    save_document, unpack_document,
};
pub use ghia::{GateMetric, GateReport, GateRow};
pub use prepare::{Prepared, prepare};
pub use registry::{ResultOutcome, RunRegistry, StartOutcome, Status};
pub use report::{Artifact, EvidenceReport, ExitClass, HolderInfo, InfraReport, Report, RunInfo};
pub use run::{Begun, Finished, RunMode, RunRequest, begin, execute, run_study};
pub use workspace::{ServerSettings, Workspace};
