//! Checks that a log of run records is consistent, and clearing host labels for sharing.

use std::collections::HashMap;

use cfdcraft_keys::Stage;
use thiserror::Error;

use crate::record::RunRecord;

/// The most records a log may hold. This is a sanity bound for the reader. It is not the limit a
/// history meets in practice: a record with its two recipes and its key numbers runs to many
/// kilobytes, so the size limit of the file's artifact index stops a history first (the document
/// crate checks the room before a case is run).
pub const MAX_RUNS: usize = 10_000;

/// The first thing wrong with a log of run records.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RunsError {
    /// The log holds more records than the limit.
    #[error("the file holds {0} run records, but at most {max} are allowed", max = MAX_RUNS)]
    TooMany(usize),
    /// A run id is not 1 to 40 lower-case letters, digits or hyphens starting with a letter or digit.
    #[error(
        "the run id \"{0}\" is not a plain name of 1 to 40 lower-case letters, digits or hyphens"
    )]
    BadId(String),
    /// Two records share an id.
    #[error("the run id \"{0}\" is used by more than one run")]
    DuplicateId(String),
    /// A record is not called what its place in the log makes it: `run-1`, `run-2` and so on.
    #[error(
        "the run id \"{found}\" is out of order: run ids count up from run-1, so the record in this place must be \"{expected}\""
    )]
    OutOfOrder { found: String, expected: String },
    /// A recorded key is not the key of the recipe next to it.
    #[error("run {run}: the {} key does not match the recipe it was recorded with", .stage.as_str())]
    KeyMismatch { run: String, stage: Stage },
    /// A result stands on another mesh than its provenance names.
    #[error("run {run}: its result was made from a different mesh than the one it records")]
    MeshKeyMismatch { run: String },
    /// The provenance names another result than the run holds.
    #[error("run {run}: the result named in its provenance is not the result it holds")]
    ResultKeyMismatch { run: String },
    /// A rerun names a run that does not come before it.
    #[error("run {run}: it repeats \"{of}\", which is not an earlier run")]
    BadRerun { run: String, of: String },
    /// A run holds a stored output although one of its checks did not pass.
    #[error("run {run}: it holds a stored output although the check \"{check}\" did not pass")]
    FailedCheck { run: String, check: String },
    /// A rerun holds a mesh; a rerun records a result only.
    #[error("run {run}: it is a rerun, which records a result only, but it holds a mesh")]
    RerunHoldsMesh { run: String },
    /// A rerun repeats a run that recorded no result.
    #[error("run {run}: it repeats \"{of}\", which recorded no result")]
    RerunOfNoResult { run: String, of: String },
    /// A run carries a scenario tag that is not a plain name, or is the reserved word `base`.
    #[error(
        "run {run}: its scenario tag \"{scenario}\" is not a plain name, or it is the reserved word \"base\""
    )]
    BadScenario { run: String, scenario: String },
    /// A rerun carries another scenario tag than the run it repeats.
    #[error("run {run}: it repeats \"{of}\", which was made for another scenario")]
    RerunScenario { run: String, of: String },
}

/// The one rule for a plain name, which every run id, link id and document name follows: 1 to 40
/// lower-case letters, digits or hyphens, the first not a hyphen. The schema carries the same rule
/// as a pattern (`^[a-z0-9][a-z0-9-]{0,39}$`).
pub fn is_plain_name(id: &str) -> bool {
    let bytes = id.as_bytes();
    (1..=40).contains(&bytes.len())
        && bytes
            .iter()
            .enumerate()
            .all(|(i, &c)| c.is_ascii_lowercase() || c.is_ascii_digit() || (i > 0 && c == b'-'))
}

/// Walk the log once, in order, and return the first problem found.
pub fn verify_runs(runs: &[RunRecord]) -> Result<(), RunsError> {
    if runs.len() > MAX_RUNS {
        return Err(RunsError::TooMany(runs.len()));
    }
    // Every earlier id, and whether that run recorded a result.
    let mut seen: HashMap<&str, bool> = HashMap::with_capacity(runs.len());
    // The scenario tag of every earlier run, by id.
    let mut tags: HashMap<&str, Option<&str>> = HashMap::with_capacity(runs.len());
    for (at, run) in runs.iter().enumerate() {
        let id = run.id.as_str();
        if !is_plain_name(id) {
            return Err(RunsError::BadId(run.id.clone()));
        }
        if seen.contains_key(id) {
            return Err(RunsError::DuplicateId(run.id.clone()));
        }
        // The next run is called by the length of the log, so a log that does not count up from
        // run-1 can never take another record.
        let expected = format!("run-{}", at + 1);
        if id != expected {
            return Err(RunsError::OutOfOrder {
                found: run.id.clone(),
                expected,
            });
        }
        if run.mesh.as_ref().is_some_and(|o| o.recipe.key() != o.key) {
            return Err(RunsError::KeyMismatch {
                run: run.id.clone(),
                stage: Stage::Mesh,
            });
        }
        if run.result.as_ref().is_some_and(|o| o.recipe.key() != o.key) {
            return Err(RunsError::KeyMismatch {
                run: run.id.clone(),
                stage: Stage::Result,
            });
        }
        let stands_elsewhere = run
            .result
            .as_ref()
            .is_some_and(|o| o.recipe.mesh_key != run.provenance.mesh_key);
        if stands_elsewhere {
            return Err(RunsError::MeshKeyMismatch {
                run: run.id.clone(),
            });
        }
        let held = run.result.as_ref().map(|o| o.key);
        if run.provenance.result_key != held {
            return Err(RunsError::ResultKeyMismatch {
                run: run.id.clone(),
            });
        }
        let unknown = run
            .rerun_of
            .as_ref()
            .filter(|of| !seen.contains_key(of.as_str()));
        if let Some(of) = unknown {
            return Err(RunsError::BadRerun {
                run: run.id.clone(),
                of: of.clone(),
            });
        }
        // What the producer guarantees, held against a file that came from elsewhere: nothing
        // is stored for a run whose checks did not all pass, and a rerun is a result on the mesh
        // of the run it repeats.
        let failed = run
            .checks
            .iter()
            .find(|check| !check.passed)
            .filter(|_| run.mesh.is_some() || run.result.is_some());
        if let Some(check) = failed {
            return Err(RunsError::FailedCheck {
                run: run.id.clone(),
                check: check.name.clone(),
            });
        }
        if let Some(of) = &run.rerun_of {
            if run.mesh.is_some() {
                return Err(RunsError::RerunHoldsMesh {
                    run: run.id.clone(),
                });
            }
            if seen.get(of.as_str()) == Some(&false) {
                return Err(RunsError::RerunOfNoResult {
                    run: run.id.clone(),
                    of: of.clone(),
                });
            }
        }
        // The scenario tag comes last, so every earlier check keeps its place in the order.
        let tag = run.scenario.as_deref();
        if let Some(tag) = tag.filter(|t| !is_plain_name(t) || *t == "base") {
            return Err(RunsError::BadScenario {
                run: run.id.clone(),
                scenario: tag.to_owned(),
            });
        }
        if let Some(of) = &run.rerun_of
            && tags.get(of.as_str()) != Some(&tag)
        {
            return Err(RunsError::RerunScenario {
                run: run.id.clone(),
                of: of.clone(),
            });
        }
        seen.insert(id, run.result.is_some());
        tags.insert(id, tag);
    }
    Ok(())
}

/// Clear the host label of every record, and change nothing else.
pub fn redact_hosts(runs: &mut [RunRecord]) {
    for run in runs {
        run.provenance.host = None;
    }
}
