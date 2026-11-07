//! The record of one run, and the lookups over a log of them.

use cfdcraft_compare::{Comparison, KeyNumbers};
use cfdcraft_keys::{MeshRecipe, ResultRecipe, Stage, StageKey, ToolIdentity};
use cfdcraft_model::Study;
use cfdcraft_store::ContentHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One stage output a run recorded: its key, the recipe the key comes from, and the stored blob.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Output<R> {
    /// The key of `recipe`.
    pub key: StageKey,
    /// What made the output.
    pub recipe: R,
    /// The hash of the stored blob.
    pub blob: ContentHash,
    /// The size of the stored blob in bytes.
    pub size: u64,
}

/// One named check a run was held to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// What was checked.
    pub name: String,
    /// Whether it held.
    pub passed: bool,
    /// A short statement of what was found.
    pub detail: String,
}

/// What a run stood on, so it can be repeated without anything else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// The hash of the geometry recipe.
    pub geometry_hash: ContentHash,
    /// The key of the mesh the run stands on.
    pub mesh_key: StageKey,
    /// The hash of the mesh blob the run stands on.
    pub mesh_hash: ContentHash,
    /// The key of the result the run recorded, if it recorded one.
    pub result_key: Option<StageKey>,
    /// The authored study as it was when the case ran.
    pub settings: Study,
    /// The tools that made the outputs.
    pub tool: ToolIdentity,
    /// A label for the machine it ran on; sharing may clear it.
    pub host: Option<String>,
}

/// One run. A record is never edited after it is appended to the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    /// `run-<n>`, counted from the number of records before it.
    pub id: String,
    /// The run this one repeats, if it is a rerun.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rerun_of: Option<String>,
    /// The scenario this run was made for; absent means the study itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    /// The mesh this run recorded; absent when it did not record one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<Output<MeshRecipe>>,
    /// The result this run recorded; absent when it did not record one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Output<ResultRecipe>>,
    /// What the run stood on.
    pub provenance: Provenance,
    /// The checks the run was held to.
    #[serde(default)]
    pub checks: Vec<Check>,
    /// The numbers a rerun is compared on.
    #[serde(default)]
    pub key_numbers: KeyNumbers,
    /// For a rerun, how it compares with the run it repeats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<Comparison>,
}

/// One stored blob a record names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobUse {
    /// The hash of the blob.
    pub hash: ContentHash,
    /// Its size in bytes.
    pub size: u64,
    /// The stage whose output it is.
    pub stage: Stage,
}

impl RunRecord {
    /// Every blob this record names: the mesh first, then the result.
    pub fn blob_uses(&self) -> Vec<BlobUse> {
        let mesh = self.mesh.as_ref().map(|o| BlobUse {
            hash: o.blob,
            size: o.size,
            stage: Stage::Mesh,
        });
        let result = self.result.as_ref().map(|o| BlobUse {
            hash: o.blob,
            size: o.size,
            stage: Stage::Result,
        });
        mesh.into_iter().chain(result).collect()
    }
}

/// The id the next appended record gets: `run-` and the number of records plus one.
pub fn next_run_id(runs: &[RunRecord]) -> String {
    format!("run-{}", runs.len() + 1)
}

/// The record with this id.
pub fn find_run<'a>(runs: &'a [RunRecord], id: &str) -> Option<&'a RunRecord> {
    runs.iter().find(|r| r.id == id)
}

/// The newest record that holds a mesh with this key, and that mesh; a rerun never owns a key.
pub fn latest_mesh(runs: &[RunRecord], key: StageKey) -> Option<(&RunRecord, &Output<MeshRecipe>)> {
    runs.iter()
        .rev()
        .filter(|r| r.rerun_of.is_none())
        .find_map(|r| r.mesh.as_ref().filter(|o| o.key == key).map(|o| (r, o)))
}

/// The newest record that holds a result with this key; a rerun never owns a key.
pub fn latest_result(
    runs: &[RunRecord],
    key: StageKey,
) -> Option<(&RunRecord, &Output<ResultRecipe>)> {
    runs.iter()
        .rev()
        .filter(|r| r.rerun_of.is_none())
        .find_map(|r| r.result.as_ref().filter(|o| o.key == key).map(|o| (r, o)))
}

/// The last mesh output in the log, leaving reruns aside.
pub fn newest_mesh_output(runs: &[RunRecord]) -> Option<&Output<MeshRecipe>> {
    runs.iter()
        .rev()
        .filter(|r| r.rerun_of.is_none())
        .find_map(|r| r.mesh.as_ref())
}

/// The last result output in the log, leaving reruns aside.
pub fn newest_result_output(runs: &[RunRecord]) -> Option<&Output<ResultRecipe>> {
    runs.iter()
        .rev()
        .filter(|r| r.rerun_of.is_none())
        .find_map(|r| r.result.as_ref())
}
