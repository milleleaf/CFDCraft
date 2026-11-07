//! Overrides: a typed change to one setting of a study, addressed by a JSON Pointer.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The most segments a setting pointer may have.
const MAX_SEGMENTS: usize = 12;
/// The most characters a setting pointer may have.
const MAX_SETTING: usize = 200;

/// One change to a study: put a value at a setting, or take a member away.
///
/// A setting is a JSON Pointer (RFC 6901) into the study written as JSON, for example
/// `/mesh/cells_x`. Only object members are addressed; no list element ever is, so a list such as
/// the faces of a boundary is replaced whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Override {
    /// Replace the member at `setting`, or add it when its parent exists and it does not.
    Set { setting: String, value: Value },
    /// Delete the member at `setting`.
    Remove { setting: String },
}

/// Why overrides could not be applied.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApplyError {
    /// The setting is not a well-formed pointer.
    #[error("setting \"{setting}\": {reason}")]
    BadSetting { setting: String, reason: String },
    /// A step of the pointer before the last has no member of that name.
    #[error("setting \"{setting}\": a part of the path above it does not exist")]
    MissingParent { setting: String },
    /// A part of the path above the setting is not an object, so it has no members to name.
    #[error("setting \"{setting}\": a part of the path above it is not an object")]
    NotAnObject { setting: String },
    /// There is nothing at the setting to remove.
    #[error("setting \"{setting}\": there is nothing there to remove")]
    NothingToRemove { setting: String },
    /// The result does not read back as a valid study.
    #[error("the result is not a valid study: {reason}")]
    NotAStudy { reason: String },
}

fn bad(setting: &str, reason: &str) -> ApplyError {
    ApplyError::BadSetting {
        // The setting comes from a file, so what an error repeats of it is bounded.
        setting: setting.chars().take(MAX_SETTING).collect(),
        reason: reason.to_string(),
    }
}

/// Split a setting pointer into its decoded segments, or say what is wrong with it.
pub(crate) fn parse_setting(setting: &str) -> Result<Vec<String>, ApplyError> {
    if setting.chars().count() > MAX_SETTING {
        return Err(bad(setting, "it is longer than 200 characters"));
    }
    let Some(rest) = setting.strip_prefix('/') else {
        return Err(bad(setting, "it must start with a slash"));
    };
    let mut segments = Vec::new();
    for raw in rest.split('/') {
        if raw.is_empty() {
            return Err(bad(setting, "it has an empty part"));
        }
        let mut part = String::with_capacity(raw.len());
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c != '~' {
                part.push(c);
                continue;
            }
            match chars.next() {
                Some('0') => part.push('~'),
                Some('1') => part.push('/'),
                _ => return Err(bad(setting, "a tilde must be followed by 0 or 1")),
            }
        }
        segments.push(part);
    }
    if segments.len() > MAX_SEGMENTS {
        return Err(bad(setting, "it has more than 12 parts"));
    }
    Ok(segments)
}

/// Apply the overrides in order to a copy of `study` and return the copy; `study` is not changed.
///
/// Whether the result is a valid study is not judged here.
pub fn apply_overrides(study: &Value, overrides: &[Override]) -> Result<Value, ApplyError> {
    let mut out = study.clone();
    for item in overrides {
        let (setting, new) = match item {
            Override::Set { setting, value } => (setting, Some(value)),
            Override::Remove { setting } => (setting, None),
        };
        let segments = parse_setting(setting)?;
        let Some((last, parents)) = segments.split_last() else {
            return Err(bad(setting, "it has no parts"));
        };
        // A study with no raw overrides does not write the member, so the first one is added
        // under a new, empty map (the member is optional and empty by default).
        let adds_a_raw_override =
            new.is_some() && segments.len() >= 2 && segments[0] == "raw_overrides";
        if adds_a_raw_override && let Value::Object(root) = &mut out {
            root.entry("raw_overrides".to_owned())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
        }
        let mut at = &mut out;
        for segment in parents {
            let Value::Object(members) = at else {
                return Err(ApplyError::NotAnObject {
                    setting: setting.clone(),
                });
            };
            at = members
                .get_mut(segment)
                .ok_or_else(|| ApplyError::MissingParent {
                    setting: setting.clone(),
                })?;
        }
        let Value::Object(members) = at else {
            return Err(ApplyError::NotAnObject {
                setting: setting.clone(),
            });
        };
        match new {
            Some(value) => {
                members.insert(last.clone(), value.clone());
            }
            None => {
                if members.remove(last).is_none() {
                    return Err(ApplyError::NothingToRemove {
                        setting: setting.clone(),
                    });
                }
            }
        }
    }
    Ok(out)
}
