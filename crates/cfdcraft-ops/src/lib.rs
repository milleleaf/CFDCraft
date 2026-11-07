//! Typed operations over studies and cases. The command line and any other front end are thin
//! adapters over this crate, so every path that writes a case runs the same gates in the same
//! order and reports through the same [`Report`].

pub mod benchmark_ops;
pub mod benchmarks;
pub mod cases;
pub mod convergence;
pub mod disk_ops;
pub mod documents;
pub mod gci_ops;
pub mod ghia;
pub mod host_runner;
pub mod inspect;
pub mod jobs;
pub mod keys;
pub mod prepare;
pub mod queue_file;
pub mod queue_ops;
pub mod reasons;
pub mod registry;
pub mod report;
pub mod repro;
pub mod run;
pub mod scenario_ops;
pub mod sweep_ops;
pub mod update;
pub mod workspace;

pub use benchmark_ops::{
    BenchmarkOptions, benchmark_check, benchmark_create, benchmark_list, created_line,
};
pub use benchmarks::{BundleFiles, DirFiles, build, ids, quantities_for, select};
pub use cases::{check_case, compile_study, export_study};
pub use cfdcraft_doc::default_link_id;
pub use cfdcraft_keys::Stage;
pub use documents::{
    DocOutcome, check_document_links, link_file, open_document, pack_document, read_study,
    save_document, unpack_document,
};
pub use gci_ops::{GciData, GciLevelRow, GciRequest, gci_warning_lines};
pub use ghia::{GateMetric, GateReport, GateRow};
pub use host_runner::HostRunner;
pub use inspect::{
    compare_runs, document_status, list_runs, run_provenance, run_result, scenario_status,
};
pub use jobs::{
    CaseRun, CaseRunner, DocJob, JobAction, JobOptions, plan_job, plan_scenario_job,
    plan_scenario_job_with, solve_document, solve_scenario,
};
pub use keys::{stage_keys, tool_identity};
pub use prepare::{Prepared, prepare, prepare_with_options};
pub use queue_ops::{AddRequest, CONTINUE_REFUSED};
pub use reasons::{change_line, reason_parts, reason_text, stale_line, stale_sentence};
pub use registry::{ResultOutcome, RunRegistry, StartOutcome, Status};
pub use report::{Artifact, EvidenceReport, ExitClass, HolderInfo, InfraReport, Report, RunInfo};
pub use repro::{
    BenchmarkEntry, BenchmarksData, ComparisonData, DiffData, JobView, PlanData, PointCheck,
    PreflightData, ProvenanceData, PruneData, QueueData, RawView, Repro, ResultData, RunData,
    RunSummary, RunsData, ScenarioData, ScenarioSummary, ScenariosData, StageStatus, StatusData,
    SweepData, UpdateData, UpdatedItem,
};
pub use run::{Begun, Finished, RunMode, RunRequest, begin, execute, run_study};
pub use update::{plan_update_report, update_documents};
pub use workspace::{DocJobKind, ServerSettings, Workspace};
