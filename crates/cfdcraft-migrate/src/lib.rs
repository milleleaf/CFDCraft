//! Brings an older study document forward one schema version at a time, and refuses a newer one.
//!
//! The crate is pure: no file, process, network, thread, clock or environment access, so the same
//! migration can run anywhere, including in a browser.
//!
//! A document carries one schema number for the whole file. To add the next schema version, append
//! one [`Step`] to [`STEPS`] and raise [`CURRENT_SCHEMA`]; the runner does not change.

use cfdcraft_model::{
    MigrationNote,
    migrate::{StudyV0, migrate_v0_to_v1},
};
use serde_json::Value;
use thiserror::Error;

/// The schema version this build writes and fully understands.
pub const CURRENT_SCHEMA: u32 = 5;

/// The members of a document that a migration step may change.
#[derive(Debug, Clone, PartialEq)]
pub struct Parts {
    /// The study body.
    pub study: Value,
    /// The artifact index, or `None` for a document that has none yet (read as an empty index).
    pub artifacts: Option<Value>,
}

/// Why a document could not be brought to the current schema.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MigrateError {
    /// The document was written by a newer build than this one.
    #[error(
        "this document uses schema {found}, but this version only understands up to schema {supported}"
    )]
    NewerSchema { found: u32, supported: u32 },
    /// One step failed, or the step table has a gap.
    #[error("migrating from schema {from} to schema {to} failed: {reason}")]
    Step { from: u32, to: u32, reason: String },
}

/// One schema upgrade: schema `from` to schema `from + 1`.
///
/// A step is a pure function (no clock, no file access). It may change either member of [`Parts`]
/// and returns a plain-language note for the caller to show.
#[derive(Clone, Copy)]
pub struct Step {
    pub from: u32,
    pub apply: fn(Parts) -> Result<(Parts, MigrationNote), MigrateError>,
}

/// Every real step, in order, starting at schema 0.
pub const STEPS: &[Step] = &[
    Step {
        from: 0,
        apply: v0_to_v1,
    },
    Step {
        from: 1,
        apply: v1_to_v2,
    },
    Step {
        from: 2,
        apply: v2_to_v3,
    },
    Step {
        from: 3,
        apply: v3_to_v4,
    },
    Step {
        from: 4,
        apply: v4_to_v5,
    },
];

fn v0_to_v1(parts: Parts) -> Result<(Parts, MigrationNote), MigrateError> {
    let fail = |reason: String| MigrateError::Step {
        from: 0,
        to: 1,
        reason,
    };
    let old: StudyV0 = serde_json::from_value(parts.study).map_err(|e| fail(e.to_string()))?;
    let (study, note) = migrate_v0_to_v1(old);
    let study = serde_json::to_value(study).map_err(|e| fail(e.to_string()))?;
    Ok((
        Parts {
            study,
            artifacts: parts.artifacts,
        },
        note,
    ))
}

/// Schema 2 adds the history of runs to the artifact index. The study is untouched; an index
/// that lacks the entry gets an empty one, and anything else is returned as it came.
fn v1_to_v2(parts: Parts) -> Result<(Parts, MigrationNote), MigrateError> {
    let mut artifacts = parts.artifacts;
    if let Some(Value::Object(map)) = &mut artifacts {
        map.entry("runs")
            .or_insert_with(|| Value::Array(Vec::new()));
    }
    Ok((
        Parts {
            study: parts.study,
            artifacts,
        },
        MigrationNote {
            from: 1,
            to: 2,
            summary: "This file now keeps a history of runs: what ran, from which settings, \
                      and what it made. This file has none yet. Nothing else changed."
                .to_owned(),
        },
    ))
}

/// Schema 3 lets the artifact index hold variants of the study, parameter sweeps and a record of
/// pruned results. Each is optional and an absent one reads as empty, so nothing in the file
/// changes; the step only moves the number and says so.
fn v2_to_v3(parts: Parts) -> Result<(Parts, MigrationNote), MigrateError> {
    Ok((
        parts,
        MigrationNote {
            from: 2,
            to: 3,
            summary: "This file can now hold variants of the study, parameter sweeps and a record \
                      of pruned results. This file has none yet. Nothing else changed."
                .to_owned(),
        },
    ))
}

/// Schema 4 lets a run record say it was stopped, keep the time it can be continued from, name the
/// run it was continued from, and carry the verdict on whether its monitored quantities had
/// settled. Each is optional and an absent one reads as an ordinary complete run, so nothing in the
/// file changes; the step only moves the number and says so.
fn v3_to_v4(parts: Parts) -> Result<(Parts, MigrationNote), MigrateError> {
    Ok((
        parts,
        MigrationNote {
            from: 3,
            to: 4,
            summary:
                "This file can now keep runs that were stopped and continued, and a record of \
                      whether each result had converged. This file has none yet. Nothing else \
                      changed."
                    .to_owned(),
        },
    ))
}

/// Schema 5 lets the recipes in the artifact index carry a mesh level, a wall grading, a domain
/// shape, thermal conditions on the walls, the physics of the run, force monitors and the
/// monitors' boundaries. Each is optional and an absent one reads as a plain box, steady and
/// laminar, so nothing in the file changes; the step only moves the number and says so.
fn v4_to_v5(parts: Parts) -> Result<(Parts, MigrationNote), MigrateError> {
    Ok((
        parts,
        MigrationNote {
            from: 4,
            to: 5,
            summary: "This file can now keep results of studies with a mesh level, other shapes, \
                      turbulence models, time-dependent and buoyant physics, and the benchmark \
                      numbers recorded with them. This file has none yet. Nothing else changed."
                .to_owned(),
        },
    ))
}

/// Refuse a document from a newer build, deciding from the version number alone.
pub fn check_readable(found: u32) -> Result<(), MigrateError> {
    if found > CURRENT_SCHEMA {
        Err(MigrateError::NewerSchema {
            found,
            supported: CURRENT_SCHEMA,
        })
    } else {
        Ok(())
    }
}

/// Bring `parts`, written at schema `found`, up to [`CURRENT_SCHEMA`].
pub fn migrate(parts: Parts, found: u32) -> Result<(Parts, Vec<MigrationNote>), MigrateError> {
    check_readable(found)?;
    migrate_with(STEPS, CURRENT_SCHEMA, parts, found)
}

/// The chain runner behind [`migrate`], over any step table.
///
/// A newer `found` is refused before `parts` is looked at. The table must have a step for every
/// version from `found` up to `current` (a missing one is a [`MigrateError::Step`] naming it,
/// reported before anything runs). Steps run once each, in order; the first failure stops the chain
/// and no notes are returned.
pub fn migrate_with(
    steps: &[Step],
    current: u32,
    parts: Parts,
    found: u32,
) -> Result<(Parts, Vec<MigrationNote>), MigrateError> {
    if found > current {
        return Err(MigrateError::NewerSchema {
            found,
            supported: current,
        });
    }
    let mut chain = Vec::new();
    for version in found..current {
        let step = steps
            .iter()
            .find(|s| s.from == version)
            .ok_or_else(|| MigrateError::Step {
                from: version,
                to: version + 1,
                reason: format!("no migration step from schema {version}"),
            })?;
        chain.push(step);
    }
    let mut parts = parts;
    let mut notes = Vec::with_capacity(chain.len());
    for step in chain {
        let (next, note) = (step.apply)(parts)?;
        parts = next;
        notes.push(note);
    }
    Ok((parts, notes))
}
