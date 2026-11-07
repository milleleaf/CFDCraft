//! Checks that a log of run records is consistent, and clearing host labels for sharing.

use std::collections::HashSet;

use cfdcraft_keys::Stage;
use thiserror::Error;

use crate::record::RunRecord;

/// The most records a log may hold.
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
}

fn is_plain_id(id: &str) -> bool {
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
    let mut seen: HashSet<&str> = HashSet::with_capacity(runs.len());
    for run in runs {
        let id = run.id.as_str();
        if !is_plain_id(id) {
            return Err(RunsError::BadId(run.id.clone()));
        }
        if seen.contains(id) {
            return Err(RunsError::DuplicateId(run.id.clone()));
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
            .filter(|of| !seen.contains(of.as_str()));
        if let Some(of) = unknown {
            return Err(RunsError::BadRerun {
                run: run.id.clone(),
                of: of.clone(),
            });
        }
        seen.insert(id);
    }
    Ok(())
}

/// Clear the host label of every record, and change nothing else.
pub fn redact_hosts(runs: &mut [RunRecord]) {
    for run in runs {
        run.provenance.host = None;
    }
}
