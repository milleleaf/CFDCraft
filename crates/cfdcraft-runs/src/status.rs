//! Which stages of a document are fresh, stale or never made, and why.

use cfdcraft_keys::{Change, StageRecipes, diff_mesh, diff_result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::record::{RunRecord, latest_mesh, latest_result};

/// Where one stage stands against the recipes the study makes now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StageState {
    /// No record holds an output of this stage.
    NeverMade,
    /// A record holds an output with the current key.
    Fresh {
        /// The newest record holding it.
        run: String,
    },
    /// The newest recorded output has another key; `changes` go from its recipe to the current one.
    Stale {
        /// The record holding the newest output of this stage.
        since: String,
        /// What differs, old to new.
        changes: Vec<Change>,
    },
}

/// The state of both stages of one document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Status {
    pub mesh: StageState,
    pub result: StageState,
}

/// The state of each stage for a log of runs and the recipes the study makes now. A rerun never
/// makes a stage fresh and is never the newest output a stage is compared with. Blobs are not
/// inspected: the caller checks that a recorded blob is still in the store.
pub fn status(runs: &[RunRecord], current: &StageRecipes) -> Status {
    let mesh = match latest_mesh(runs, current.mesh.key()) {
        Some((run, _)) => StageState::Fresh {
            run: run.id.clone(),
        },
        None => match runs
            .iter()
            .rev()
            .find_map(|r| r.mesh.as_ref().map(|o| (r, o)))
        {
            Some((run, old)) => StageState::Stale {
                since: run.id.clone(),
                changes: diff_mesh(&old.recipe, &current.mesh),
            },
            None => StageState::NeverMade,
        },
    };
    let result = match latest_result(runs, current.result.key()) {
        Some((run, _)) => StageState::Fresh {
            run: run.id.clone(),
        },
        None => match runs
            .iter()
            .rev()
            .filter(|r| r.rerun_of.is_none())
            .find_map(|r| r.result.as_ref().map(|o| (r, o)))
        {
            Some((run, old)) => StageState::Stale {
                since: run.id.clone(),
                changes: diff_result(&old.recipe, &current.result),
            },
            None => StageState::NeverMade,
        },
    };
    Status { mesh, result }
}
