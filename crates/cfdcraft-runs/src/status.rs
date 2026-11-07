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

/// The state of each stage for a log of runs and the recipes the study makes now, for the study
/// itself. It is `status_for` with no scenario.
pub fn status(runs: &[RunRecord], current: &StageRecipes) -> Status {
    status_for(runs, current, None)
}

/// The state of each stage for the scenario `scenario` (`None` is the study itself). A stage is
/// fresh when any record, of any scenario, holds its current key, so a setting-only variant finds
/// the mesh its parent made. A stage that is not fresh is judged against the newest output of the
/// same scenario only, so one scenario is never compared with another one's run. A rerun never
/// makes a stage fresh and is never the newest output a stage is compared with. Blobs are not
/// inspected: the caller checks that a recorded blob is still in the store.
pub fn status_for(runs: &[RunRecord], current: &StageRecipes, scenario: Option<&str>) -> Status {
    let mesh = match latest_mesh(runs, current.mesh.key()) {
        Some((run, _)) => StageState::Fresh {
            run: run.id.clone(),
        },
        None => match newest_of(runs, scenario, |r| r.mesh.as_ref().map(|o| &o.recipe)) {
            Some((run, old)) => StageState::Stale {
                since: run.id.clone(),
                changes: diff_mesh(old, &current.mesh),
            },
            None => StageState::NeverMade,
        },
    };
    let result = match latest_result(runs, current.result.key()) {
        Some((run, _)) => StageState::Fresh {
            run: run.id.clone(),
        },
        None => match newest_of(runs, scenario, |r| r.result.as_ref().map(|o| &o.recipe)) {
            Some((run, old)) => StageState::Stale {
                since: run.id.clone(),
                changes: diff_result(old, &current.result),
            },
            None => StageState::NeverMade,
        },
    };
    Status { mesh, result }
}

/// The newest non-rerun record of this scenario that holds the stage, with the recipe it holds.
fn newest_of<'a, R>(
    runs: &'a [RunRecord],
    scenario: Option<&str>,
    recipe: impl Fn(&'a RunRecord) -> Option<&'a R>,
) -> Option<(&'a RunRecord, &'a R)> {
    runs.iter()
        .rev()
        .filter(|r| r.rerun_of.is_none() && r.scenario.as_deref() == scenario)
        .find_map(|r| recipe(r).map(|old| (r, old)))
}
