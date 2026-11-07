//! Scenarios, the chain of parents, and the effective study of a variant.

use std::collections::{HashMap, HashSet};

use cfdcraft_model::Study;
use cfdcraft_runs::is_plain_name;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::overrides::{ApplyError, Override, apply_overrides, parse_setting};

/// The id of the document's own study. A first-level variant names it as its parent.
pub const BASE: &str = "base";

/// The most scenarios one file may hold.
pub const MAX_SCENARIOS: usize = 1000;
/// The most levels of parent a variant may have.
pub const MAX_DEPTH: usize = 16;
/// The most overrides one scenario may hold.
pub const MAX_OVERRIDES: usize = 200;
/// The most characters in a scenario name.
pub const MAX_NAME: usize = 80;
/// The most characters in the notes of a scenario.
pub const MAX_NOTES: usize = 2000;

/// The most parent links walked from a variant before the walk gives up.
const WALK_STEPS: usize = 17;

/// Which sweep made a variant, and which point of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub sweep: String,
    pub index: u32,
}

/// A variant of a study: what it changes, and the label a person gave it.
///
/// It stores only its own overrides. Its effective study is the study it descends from with the
/// overrides of every ancestor applied first and its own after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<Override>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Why a scenario could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScenarioError {
    /// No scenario has this id, or its chain of parents does not end at the base study.
    #[error("there is no variant \"{scenario}\"")]
    Unknown { scenario: String },
    /// An override of this scenario could not be applied.
    #[error("variant \"{scenario}\": {source}")]
    Apply {
        scenario: String,
        source: ApplyError,
    },
    /// An id is not a plain name (1 to 40 lower-case letters, digits or hyphens, the first not a
    /// hyphen).
    #[error("variant \"{scenario}\": a name used as an id is not a plain name")]
    BadId { scenario: String },
    /// Two records have the same id.
    #[error("variant \"{scenario}\" is listed more than once")]
    Duplicate { scenario: String },
    /// The record for the base study holds something only a variant may hold.
    #[error("the record \"base\" {reason}")]
    BaseMisused { reason: String },
    /// A parent is listed after its child, or is the child itself.
    #[error("variant \"{scenario}\": its parent \"{parent}\" is not listed before it")]
    ParentNotEarlier { scenario: String, parent: String },
    /// A parent is not in the list.
    #[error("variant \"{scenario}\": its parent \"{parent}\" does not exist")]
    UnknownParent { scenario: String, parent: String },
    /// A chain of parents is longer than the limit.
    #[error("variant \"{scenario}\": it descends through more than 16 levels")]
    TooDeep { scenario: String },
    /// The list is longer than the limit.
    #[error("{count} variants are listed, and at most 1000 are allowed")]
    TooMany { count: usize },
    /// A scenario holds more overrides than the limit.
    #[error("variant \"{scenario}\": it holds more than 200 overrides")]
    TooManyOverrides { scenario: String },
    /// A name is empty, too long or holds a control character.
    #[error(
        "variant \"{scenario}\": its name must be 1 to 80 characters with no control character"
    )]
    BadName { scenario: String },
    /// Notes are too long or hold a control character other than a newline.
    #[error(
        "variant \"{scenario}\": its notes must be at most 2000 characters with no control character but a newline"
    )]
    BadNotes { scenario: String },
    /// A stored override names a setting that is not a well-formed pointer.
    #[error("variant \"{scenario}\": setting \"{setting}\": {reason}")]
    BadSetting {
        scenario: String,
        setting: String,
        reason: String,
    },
}

/// The scenarios a variant descends from, root first, ending with the variant itself.
///
/// A record with the id `base` is never part of the chain, and the id `base` has an empty one.
pub fn chain<'a>(scenarios: &'a [Scenario], id: &str) -> Result<Vec<&'a Scenario>, ScenarioError> {
    let mut line = Vec::new();
    if id == BASE {
        return Ok(line);
    }
    let mut at = id;
    for _ in 0..WALK_STEPS {
        let found =
            scenarios
                .iter()
                .find(|s| s.id == at)
                .ok_or_else(|| ScenarioError::Unknown {
                    scenario: at.to_string(),
                })?;
        line.push(found);
        match found.parent.as_deref() {
            None | Some(BASE) => {
                line.reverse();
                return Ok(line);
            }
            Some(parent) => at = parent,
        }
    }
    Err(ScenarioError::Unknown {
        scenario: at.to_string(),
    })
}

/// The study a scenario stands for: `base` as JSON with the overrides of the chain applied root to
/// leaf, read back as a study. The id `base` gives the base study itself.
pub fn effective_study(
    base: &Study,
    scenarios: &[Scenario],
    id: &str,
) -> Result<Study, ScenarioError> {
    if id == BASE {
        return Ok(base.clone());
    }
    let line = chain(scenarios, id)?;
    let not_a_study = |reason: String| ScenarioError::Apply {
        scenario: id.to_string(),
        source: ApplyError::NotAStudy { reason },
    };
    let mut value = serde_json::to_value(base).map_err(|e| not_a_study(e.to_string()))?;
    for scenario in line {
        value = apply_overrides(&value, &scenario.overrides).map_err(|source| {
            ScenarioError::Apply {
                scenario: scenario.id.clone(),
                source,
            }
        })?;
    }
    serde_json::from_value::<Study>(value).map_err(|e| not_a_study(e.to_string()))
}

/// Check the rules that keep a list of scenarios sound, and return the first problem found.
///
/// A parent must be `base` or a record listed earlier, so a list cannot hold a cycle; the depth,
/// the count, the overrides, the names and the notes are bounded.
pub fn check_scenarios(scenarios: &[Scenario]) -> Result<(), ScenarioError> {
    if scenarios.len() > MAX_SCENARIOS {
        return Err(ScenarioError::TooMany {
            count: scenarios.len(),
        });
    }
    let listed: HashSet<&str> = scenarios.iter().map(|s| s.id.as_str()).collect();
    // The depth of every record seen so far, so the walk stays linear.
    let mut depths: HashMap<&str, usize> = HashMap::with_capacity(scenarios.len());
    for scenario in scenarios {
        let id = scenario.id.as_str();
        if !is_plain_name(id) {
            return Err(ScenarioError::BadId {
                scenario: id.chars().take(40).collect(),
            });
        }
        if depths.contains_key(id) {
            return Err(ScenarioError::Duplicate {
                scenario: id.to_string(),
            });
        }
        let depth = if id == BASE {
            let misuse = if scenario.parent.is_some() {
                Some("cannot have a parent")
            } else if !scenario.overrides.is_empty() {
                Some("cannot hold overrides")
            } else if scenario.origin.is_some() {
                Some("cannot have an origin")
            } else {
                None
            };
            if let Some(reason) = misuse {
                return Err(ScenarioError::BaseMisused {
                    reason: reason.to_string(),
                });
            }
            0
        } else {
            let parent = scenario.parent.as_deref().unwrap_or("");
            let parent_depth = if parent == BASE {
                0
            } else if let Some(depth) = depths.get(parent) {
                *depth
            } else if listed.contains(parent) {
                return Err(ScenarioError::ParentNotEarlier {
                    scenario: id.to_string(),
                    parent: parent.chars().take(40).collect(),
                });
            } else {
                return Err(ScenarioError::UnknownParent {
                    scenario: id.to_string(),
                    parent: parent.chars().take(40).collect(),
                });
            };
            if parent_depth + 1 > MAX_DEPTH {
                return Err(ScenarioError::TooDeep {
                    scenario: id.to_string(),
                });
            }
            parent_depth + 1
        };
        depths.insert(id, depth);
        if scenario.overrides.len() > MAX_OVERRIDES {
            return Err(ScenarioError::TooManyOverrides {
                scenario: id.to_string(),
            });
        }
        for item in &scenario.overrides {
            let (Override::Set { setting, .. } | Override::Remove { setting }) = item;
            if let Err(ApplyError::BadSetting { setting, reason }) = parse_setting(setting) {
                return Err(ScenarioError::BadSetting {
                    scenario: id.to_string(),
                    setting,
                    reason,
                });
            }
        }
        if let Some(name) = &scenario.name {
            let length = name.chars().count();
            if length == 0 || length > MAX_NAME || name.chars().any(char::is_control) {
                return Err(ScenarioError::BadName {
                    scenario: id.to_string(),
                });
            }
        }
        if let Some(notes) = &scenario.notes
            && (notes.chars().count() > MAX_NOTES
                || notes.chars().any(|c| c.is_control() && c != '\n'))
        {
            return Err(ScenarioError::BadNotes {
                scenario: id.to_string(),
            });
        }
        if let Some(origin) = &scenario.origin
            && !is_plain_name(&origin.sweep)
        {
            return Err(ScenarioError::BadId {
                scenario: id.to_string(),
            });
        }
    }
    Ok(())
}
