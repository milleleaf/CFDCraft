//! The run history inside the artifact index: adding a run, clearing host labels, and checking
//! that the history and the list of stored results agree.

use std::collections::HashMap;

use cfdcraft_runs::{RunRecord, RunsError, Stage, next_run_id, redact_hosts, verify_runs};
use cfdcraft_store::ContentHash;
use thiserror::Error;

use crate::model::{ARTIFACTS_LIMIT, ArtifactIndex, BlobKind, BlobRef, Document};

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
    /// The list of runs has no room for another record: the index would pass its size limit.
    #[error(
        "the run history already holds {runs} records, and the file has no room for another: its list of results and runs would pass the {limit} byte limit"
    )]
    HistoryFull {
        /// How many records the history holds.
        runs: usize,
        /// The size limit of the index in bytes.
        limit: u64,
    },
    /// The record names a stored result that the file already lists with another size or kind.
    #[error("the stored data {0} is already listed in the file with another size or kind")]
    BlobConflict(ContentHash),
    /// The record was made for a variant the file does not list.
    #[error("the run was made for the variant {scenario}, which the file does not list")]
    UnknownScenario {
        /// The variant the record names.
        scenario: String,
    },
}

/// Room kept for the blob entries a new record brings with it, on top of the record itself.
const BLOB_ENTRIES_ROOM: usize = 4 * 1024;

impl Document {
    /// Whether one more record fits in the file, judged before a case is run: the index as it is
    /// now, plus a record as big as the biggest one it holds, must stay within the size limit
    /// the file is written and read under. A history is stopped by that limit long before it
    /// reaches the most records it may hold.
    pub fn check_room_for_run(&self) -> Result<(), RunLogError> {
        let size = |bytes: serde_json::Result<Vec<u8>>| bytes.map_or(0, |bytes| bytes.len());
        let runs = &self.artifacts.runs;
        let biggest = runs
            .iter()
            .map(|run| size(serde_json::to_vec_pretty(run)))
            .max()
            .unwrap_or(0);
        let now = size(serde_json::to_vec_pretty(&self.artifacts));
        if (now + biggest + BLOB_ENTRIES_ROOM) as u64 > ARTIFACTS_LIMIT {
            return Err(RunLogError::HistoryFull {
                runs: runs.len(),
                limit: ARTIFACTS_LIMIT,
            });
        }
        Ok(())
    }

    /// Append `record` to the run history and list the stored results it names.
    ///
    /// The record must be the next run, must be consistent with the runs before it, must name a
    /// listed variant when it names one, and must agree with what the index already says about
    /// each result it names. Nothing is changed unless all of that holds. A result the record
    /// stores again is taken off the list of pruned results.
    pub fn add_run(&mut self, record: RunRecord) -> Result<(), RunLogError> {
        let expected = next_run_id(&self.artifacts.runs);
        if record.id != expected {
            return Err(RunLogError::WrongId {
                expected,
                found: record.id,
            });
        }
        let uses = record.blob_uses();
        let named: Vec<ContentHash> = uses.iter().map(|used| used.hash).collect();
        let mut runs = self.artifacts.runs.clone();
        runs.push(record);
        verify_runs(&runs)?;
        if let Some(scenario) = runs.last().and_then(|r| r.scenario.as_deref())
            && !self.artifacts.scenarios.iter().any(|s| s.id == scenario)
        {
            return Err(RunLogError::UnknownScenario {
                scenario: scenario.to_owned(),
            });
        }

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
        // A result made again is no longer gone; the rest of the list keeps its order.
        self.artifacts.pruned.retain(|hash| !named.contains(hash));
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
