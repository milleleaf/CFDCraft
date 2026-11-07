//! Typed operations over studies and cases. The command line and any other front end are thin
//! adapters over this crate, so every path that writes a case runs the same gates in the same
//! order and reports through the same [`Report`].

pub mod cases;
pub mod documents;
pub mod ghia;
pub mod host_runner;
pub mod inspect;
pub mod jobs;
pub mod keys;
pub mod prepare;
pub mod reasons;
pub mod registry;
pub mod report;
pub mod repro;
pub mod run;
pub mod update;
pub mod workspace;

pub use cases::{check_case, compile_study, export_study};
pub use cfdcraft_doc::default_link_id;
pub use cfdcraft_keys::Stage;
pub use documents::{
    DocOutcome, check_document_links, link_file, open_document, pack_document, read_study,
    save_document, unpack_document,
};
pub use ghia::{GateMetric, GateReport, GateRow};
pub use host_runner::HostRunner;
pub use inspect::{compare_runs, document_status, list_runs, run_provenance, run_result};
pub use jobs::{CaseRun, CaseRunner, DocJob, JobAction, plan_job, solve_document};
pub use keys::{stage_keys, tool_identity};
pub use prepare::{Prepared, prepare};
pub use reasons::{change_line, reason_parts, reason_text, stale_line, stale_sentence};
pub use registry::{ResultOutcome, RunRegistry, StartOutcome, Status};
pub use report::{Artifact, EvidenceReport, ExitClass, HolderInfo, InfraReport, Report, RunInfo};
pub use repro::{
    ComparisonData, PlanData, ProvenanceData, Repro, ResultData, RunData, RunSummary, RunsData,
    StageStatus, StatusData, UpdateData, UpdatedItem,
};
pub use run::{Begun, Finished, RunMode, RunRequest, begin, execute, run_study};
pub use update::{plan_update_report, update_documents};
pub use workspace::{DocJobKind, ServerSettings, Workspace};
