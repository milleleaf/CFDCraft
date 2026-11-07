use std::collections::{BTreeMap, BTreeSet};

use cfdcraft_keys::Stage;
use cfdcraft_store::ContentHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::BlobHold;

/// One stored blob that pruning would remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Removal {
    pub hash: ContentHash,
    pub size: u64,
    pub stage: Stage,
    /// The scenarios whose runs name the blob, sorted.
    pub scenarios: Vec<String>,
}

/// What pruning would remove and what it keeps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PrunePlan {
    /// Largest first, then by hash.
    pub remove: Vec<Removal>,
    pub freed_bytes: u64,
    /// Distinct blobs kept because a pinned scenario names them.
    pub kept_pinned: usize,
    /// Distinct blobs kept because a scenario outside the scope names them.
    pub kept_shared: usize,
}

/// Plans a prune. It only reports: nothing is removed here, and the caller must show the
/// plan before it acts on it.
///
/// A blob is removable when it is not already pruned, every scenario that names it is in
/// `scope` (every scenario when `scope` is `None`), and none of them is pinned. A pinned
/// scenario protects a blob before any other rule is looked at.
pub fn plan_prune(
    holds: &[BlobHold],
    pinned: &BTreeSet<String>,
    pruned: &BTreeSet<ContentHash>,
    scope: Option<&BTreeSet<String>>,
) -> PrunePlan {
    let mut blobs: BTreeMap<ContentHash, (u64, Stage, BTreeSet<&str>)> = BTreeMap::new();
    for hold in holds.iter().filter(|h| !pruned.contains(&h.hash)) {
        blobs
            .entry(hold.hash)
            .or_insert_with(|| (hold.size, hold.stage, BTreeSet::new()))
            .2
            .insert(&hold.scenario);
    }

    let (mut kept_pinned, mut kept_shared) = (0, 0);
    let mut remove = Vec::new();
    for (hash, (size, stage, scenarios)) in blobs {
        if scenarios.iter().any(|s| pinned.contains(*s)) {
            kept_pinned += 1;
        } else if scope.is_some_and(|scope| scenarios.iter().any(|s| !scope.contains(*s))) {
            kept_shared += 1;
        } else {
            remove.push(Removal {
                hash,
                size,
                stage,
                scenarios: scenarios.into_iter().map(str::to_owned).collect(),
            });
        }
    }
    remove.sort_by(|a, b| b.size.cmp(&a.size).then(a.hash.cmp(&b.hash)));
    let freed_bytes = remove
        .iter()
        .fold(0u64, |sum, r| sum.saturating_add(r.size));
    PrunePlan {
        remove,
        freed_bytes,
        kept_pinned,
        kept_shared,
    }
}
