use std::collections::{BTreeMap, BTreeSet};

use cfdcraft_keys::Stage;
use cfdcraft_store::ContentHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The name of the study itself, which is a scenario too.
const BASE: &str = "base";

/// One blob that one run names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BlobHold {
    /// The run that names the blob.
    pub run: String,
    /// The scenario the run was made for (`base` for the study itself).
    pub scenario: String,
    pub hash: ContentHash,
    pub size: u64,
    pub stage: Stage,
}

/// The bytes one scenario holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioUse {
    pub scenario: String,
    pub pinned: bool,
    /// Distinct runs made for the scenario.
    pub runs: usize,
    /// Stored bytes the scenario holds: `mesh_bytes` plus `result_bytes`.
    pub bytes: u64,
    pub mesh_bytes: u64,
    pub result_bytes: u64,
    /// Of `bytes`, those that another scenario also holds.
    pub shared_bytes: u64,
    /// Bytes of blobs the scenario names that have been pruned.
    pub pruned_bytes: u64,
}

/// The bytes held by every scenario, each distinct blob counted once in the totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DiskUse {
    /// `base` first, then the others by name.
    pub scenarios: Vec<ScenarioUse>,
    pub stored_bytes: u64,
    pub pruned_bytes: u64,
}

/// What each scenario holds. A hash is one blob: its size and stage are those of the first
/// hold that names it.
pub fn disk_use(
    holds: &[BlobHold],
    pinned: &BTreeSet<String>,
    pruned: &BTreeSet<ContentHash>,
) -> DiskUse {
    let mut blobs: BTreeMap<ContentHash, (u64, Stage)> = BTreeMap::new();
    let mut owners: BTreeMap<ContentHash, BTreeSet<&str>> = BTreeMap::new();
    let mut per: BTreeMap<&str, (BTreeSet<&str>, BTreeSet<ContentHash>)> = BTreeMap::new();
    for hold in holds {
        blobs.entry(hold.hash).or_insert((hold.size, hold.stage));
        owners.entry(hold.hash).or_default().insert(&hold.scenario);
        let entry = per.entry(&hold.scenario).or_default();
        entry.0.insert(&hold.run);
        entry.1.insert(hold.hash);
    }

    let mut scenarios: Vec<ScenarioUse> = per
        .into_iter()
        .map(|(scenario, (runs, hashes))| {
            let mut used = ScenarioUse {
                scenario: scenario.to_owned(),
                pinned: pinned.contains(scenario),
                runs: runs.len(),
                bytes: 0,
                mesh_bytes: 0,
                result_bytes: 0,
                shared_bytes: 0,
                pruned_bytes: 0,
            };
            for hash in hashes {
                let (size, stage) = blobs[&hash];
                if pruned.contains(&hash) {
                    used.pruned_bytes = used.pruned_bytes.saturating_add(size);
                    continue;
                }
                used.bytes = used.bytes.saturating_add(size);
                match stage {
                    Stage::Mesh => used.mesh_bytes = used.mesh_bytes.saturating_add(size),
                    Stage::Result => used.result_bytes = used.result_bytes.saturating_add(size),
                }
                if owners[&hash].len() > 1 {
                    used.shared_bytes = used.shared_bytes.saturating_add(size);
                }
            }
            used
        })
        .collect();
    scenarios
        .sort_by(|a, b| (a.scenario != BASE, &a.scenario).cmp(&(b.scenario != BASE, &b.scenario)));

    let (mut stored_bytes, mut pruned_bytes) = (0u64, 0u64);
    for (hash, (size, _)) in &blobs {
        let total = if pruned.contains(hash) {
            &mut pruned_bytes
        } else {
            &mut stored_bytes
        };
        *total = total.saturating_add(*size);
    }
    DiskUse {
        scenarios,
        stored_bytes,
        pruned_bytes,
    }
}
