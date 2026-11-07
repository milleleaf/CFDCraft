//! The data a reproducibility report carries beside the usual findings.

use cfdcraft_compare::{Comparison, KeyNumbers};
use cfdcraft_diskuse::{DiskUse, PrunePlan};
use cfdcraft_keys::Change;
use cfdcraft_runs::{Check, Provenance};
use cfdcraft_scenarios::Override;
use cfdcraft_settle::Verdict;
use cfdcraft_sweep::Point;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What an operation on the stored mesh and result of a document did. The stage words used in
/// the lists are `mesh` and `result`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Repro {
    /// A solve: which stages it recorded, which it reused, and the keys they stand on.
    Run(RunData),
    /// Whether the mesh and the result of a document are up to date, and if not why.
    Status(Box<StatusData>),
    /// The runs a document holds.
    Runs(RunsData),
    /// The checks and key numbers of one run.
    #[serde(rename = "result")]
    RunResult(Box<ResultData>),
    /// What one run was made from.
    Provenance(Box<ProvenanceData>),
    /// Two runs compared on their key numbers.
    Comparison(Box<ComparisonData>),
    /// What an update would do, in the order it would do it.
    Plan(PlanData),
    /// What an update did.
    Update(UpdateData),
    /// The variants of a document.
    Scenarios(Box<ScenariosData>),
    /// One variant, with the changes it holds.
    Scenario(Box<ScenarioData>),
    /// How two variants differ.
    ScenarioDiff(Box<DiffData>),
    /// The points a sweep makes.
    Sweep(Box<SweepData>),
    /// What checking every point of a sweep found, before anything is solved.
    Preflight(Box<PreflightData>),
    /// The disk the stored results of each variant take.
    Disk(Box<DiskUse>),
    /// What pruning stored results would remove, or did.
    Prune(Box<PruneData>),
    /// The job queue of a document.
    Queue(Box<QueueData>),
}

/// The outcome of one solve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunData {
    /// The run that was recorded; absent when nothing ran.
    pub run: Option<String>,
    /// The stages this solve made and stored.
    pub recorded: Vec<String>,
    /// The stages that were already up to date and stayed as they were.
    pub reused: Vec<String>,
    /// The key of the mesh the study makes.
    pub mesh_key: String,
    /// The key of the result the study makes.
    pub result_key: String,
}

/// Where the mesh and the result of a document stand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StatusData {
    pub mesh: StageStatus,
    pub result: StageStatus,
}

/// Where one stage stands against the study as it is now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StageStatus {
    /// The key the study makes for this stage now.
    pub key: String,
    /// `fresh`, `stale`, `never_made`, `blob_missing` or `pruned`. `pruned` means the stored
    /// output of the stage was removed on purpose; the key numbers of its runs are kept.
    pub state: String,
    /// The run whose stored output is current; present when the stage is fresh or its blob is
    /// missing.
    pub run: Option<String>,
    /// The run that holds the newest output of a stale stage.
    pub since: Option<String>,
    /// The sentence for a person.
    pub sentence: String,
    /// One rendered reason per change.
    pub reasons: Vec<String>,
    /// The changes the reasons are made from, old value to new.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<Change>,
}

/// The runs of a document, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunsData {
    pub runs: Vec<RunSummary>,
}

/// One run in the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunSummary {
    pub id: String,
    /// The run this one repeats, if it is a rerun.
    pub rerun_of: Option<String>,
    /// The variant the run was made for; null for a run of the study itself.
    pub scenario: Option<String>,
    /// The stages this run recorded: `mesh`, `result`.
    pub recorded: Vec<String>,
    pub mesh_key: Option<String>,
    pub result_key: Option<String>,
    pub host: Option<String>,
    pub checks: Vec<Check>,
    /// The verdict word of a rerun's comparison; absent for an ordinary run.
    pub verdict: Option<String>,
}

/// What one run produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResultData {
    pub run: String,
    pub result_key: Option<String>,
    pub checks: Vec<Check>,
    pub key_numbers: KeyNumbers,
    /// For a rerun, how it compares with the run it repeats.
    pub comparison: Option<Comparison>,
    /// The convergence verdict made when the run was recorded; absent for a run recorded without
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convergence: Option<Verdict>,
}

/// What one run was made from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProvenanceData {
    pub run: String,
    pub provenance: Provenance,
}

/// Two runs and how their key numbers compare.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ComparisonData {
    pub original: String,
    pub other: String,
    pub comparison: Comparison,
}

/// The items waiting for an update and the order an update would take them in. An item is
/// written `document:mesh` or `document:result`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanData {
    /// Every item that is out of date, document by document, the mesh first.
    pub pending: Vec<String>,
    /// The items an update would make, in the order it would make them.
    pub order: Vec<String>,
}

/// The items an update made, in the order it made them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateData {
    pub updated: Vec<UpdatedItem>,
}

/// One item an update made, and the run that recorded it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpdatedItem {
    pub item: String,
    pub run: String,
}

/// One variant in the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioSummary {
    /// The id of the variant; `base` is the study itself.
    pub id: String,
    /// The variant this one descends from; null for `base`.
    pub parent: Option<String>,
    /// The name a person gave it, if any.
    pub name: Option<String>,
    /// Whether pruning keeps its stored results.
    pub pinned: bool,
    /// The notes a person wrote, if any.
    pub notes: Option<String>,
    /// How many changes it holds of its own.
    pub overrides: usize,
    /// `base` for the study itself, `geometry` when its changes alter the mesh, `setting` when
    /// they leave the mesh as it was.
    pub kind: String,
    /// The sweep that made it, if one did.
    pub sweep: Option<String>,
    /// The key of the mesh it makes.
    pub mesh_key: Option<String>,
    /// The key of the result it makes.
    pub result_key: Option<String>,
    /// Another variant that makes the same mesh, if there is one.
    pub mesh_shared_with: Option<String>,
}

/// The variants of a document, the study itself first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenariosData {
    pub scenarios: Vec<ScenarioSummary>,
}

/// One variant in full.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioData {
    pub summary: ScenarioSummary,
    /// The changes the variant holds of its own, in the order they are applied.
    pub overrides: Vec<Override>,
    /// The raw dictionary entries the variant's study holds.
    pub raw_overrides: Vec<RawView>,
}

/// A raw dictionary entry as it is shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RawView {
    /// The label of the entry.
    pub id: String,
    /// The solver line it is written for.
    pub backend: String,
    /// The case file it is written into.
    pub file: String,
    /// The entry inside the file.
    pub entry: String,
    /// The value written.
    pub value: String,
}

/// How two variants differ.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DiffData {
    pub from: String,
    pub to: String,
    /// Whether the two make the same study.
    pub same: bool,
    /// The differences, first variant to second.
    pub changes: Vec<Change>,
    /// One rendered reason per change.
    pub reasons: Vec<String>,
    /// The whole line for a person.
    pub sentence: String,
}

/// The points a sweep makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SweepData {
    pub sweep: String,
    /// The variant the points descend from.
    pub base: String,
    /// Whether the sweep is stored in the document.
    pub stored: bool,
    pub points: Vec<Point>,
}

/// What checking a sweep before solving found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PreflightData {
    pub sweep: String,
    pub points: Vec<PointCheck>,
    /// The number of different meshes among the points that passed.
    pub distinct_meshes: usize,
    /// The number of points that failed.
    pub failed: usize,
}

/// What checking one point of a sweep found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PointCheck {
    pub id: String,
    pub label: String,
    pub ok: bool,
    /// Why the point fails; empty when it passes.
    pub reasons: Vec<String>,
    pub mesh_key: Option<String>,
    pub result_key: Option<String>,
    /// Another point that makes the same mesh, if there is one.
    pub mesh_shared_with: Option<String>,
}

/// What pruning stored results would remove, or removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PruneData {
    /// Whether the plan was carried out; false for a dry run.
    pub applied: bool,
    pub plan: PrunePlan,
}

/// The job queue of a document, in the order its jobs start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QueueData {
    pub jobs: Vec<JobView>,
    /// The job that is starting, running, stopping, finishing or unreachable, if there is one.
    pub active: Option<String>,
    /// Whether a runner is advancing the queue now.
    pub drain: bool,
}

/// One job of the queue as it is shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobView {
    pub id: String,
    /// The variant the job solves; null for the study itself.
    pub scenario: Option<String>,
    /// `solve` or `continue`.
    pub kind: String,
    /// The state word: `queued`, `starting`, `running`, `stopping`, `finishing`, `unreachable`,
    /// `finished`, `stopped`, `failed`, `cancelled` or `interrupted`.
    pub state: String,
    /// The sweep and point the job came from, written `<sweep> <point>`.
    pub origin: Option<String>,
    /// The run recorded for a finished or stopped job.
    pub run: Option<String>,
    /// The stable code of a failed or interrupted job.
    pub code: Option<String>,
    /// A one-line summary of the latest progress of an active job.
    pub progress_line: Option<String>,
}
