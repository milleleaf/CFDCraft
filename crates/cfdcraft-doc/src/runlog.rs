//! The run history inside the artifact index: adding a run, clearing host labels, and checking
//! that the history and the list of stored results agree.

use std::collections::HashMap;

use cfdcraft_runs::{RunRecord, RunsError, Stage, next_run_id, redact_hosts, verify_runs};
use cfdcraft_store::ContentHash;
use thiserror::Error;

use crate::model::{ArtifactIndex, BlobKind, BlobRef, Document};

/// Why a run could not be added to a document. The document is left as it was.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RunLogError {
    /// The record, with the runs before it, is not consistent.
    #[error(transparent)]
    Runs(#[from] RunsError),
    /// The record is not called what the next run must be called.
    #[error("the next run must be called {expected}, but this one is called {found}")]
    WrongId {
        /// The id the next run gets.
        expected: String,
        /// The id the record carries.
        found: String,
    },
    /// The record names a stored result that the file already lists with another size or kind.
    #[error("the stored data {0} is already listed in the file with another size or kind")]
    BlobConflict(ContentHash),
}

impl Document {
    /// Append `record` to the run history and list the stored results it names.
    ///
    /// The record must be the next run, must be consistent with the runs before it, and must
    /// agree with what the index already says about each result it names. Nothing is changed
    /// unless all of that holds.
    pub fn add_run(&mut self, record: RunRecord) -> Result<(), RunLogError> {
        let expected = next_run_id(&self.artifacts.runs);
        if record.id != expected {
            return Err(RunLogError::WrongId {
                expected,
                found: record.id,
            });
        }
        let uses = record.blob_uses();
        let mut runs = self.artifacts.runs.clone();
        runs.push(record);
        verify_runs(&runs)?;

        let mut added: Vec<BlobRef> = Vec::new();
        for used in uses {
            let kind = match used.stage {
                Stage::Mesh => BlobKind::Mesh,
                Stage::Result => BlobKind::Result,
            };
            let listed = self
                .artifacts
                .blobs
                .iter()
                .chain(added.iter())
                .find(|b| b.hash == used.hash);
            match listed {
                Some(b) if b.size == used.size && b.kind == kind => {}
                Some(_) => return Err(RunLogError::BlobConflict(used.hash)),
                None => added.push(BlobRef {
                    hash: used.hash,
                    size: used.size,
                    kind,
                }),
            }
        }
        self.artifacts.runs = runs;
        self.artifacts.blobs.extend(added);
        Ok(())
    }
}

impl ArtifactIndex {
    /// Clear the host label of every run, and change nothing else.
    pub fn redact_hosts(&mut self) {
        redact_hosts(&mut self.runs);
    }
}

/// Whether the run history holds together and every stored result it names is listed with the
/// size it was recorded with. The reason is a plain sentence and never names a path.
pub fn check_runs(index: &ArtifactIndex) -> Result<(), String> {
    verify_runs(&index.runs).map_err(|e| e.to_string())?;
    let sizes: HashMap<ContentHash, u64> = index.blobs.iter().map(|b| (b.hash, b.size)).collect();
    for run in &index.runs {
        for used in run.blob_uses() {
            match sizes.get(&used.hash) {
                None => {
                    return Err(format!(
                        "run {}: the stored data {} is not listed in the file",
                        run.id, used.hash
                    ));
                }
                Some(&size) if size != used.size => {
                    return Err(format!(
                        "run {}: the stored data {} is listed with a different size",
                        run.id, used.hash
                    ));
                }
                Some(_) => {}
            }
        }
    }
    Ok(())
}
