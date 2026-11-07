//! The data a reproducibility report carries beside the usual findings.

use cfdcraft_compare::{Comparison, KeyNumbers};
use cfdcraft_keys::Change;
use cfdcraft_runs::{Check, Provenance};
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
    /// `fresh`, `stale`, `never_made` or `blob_missing`.
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
