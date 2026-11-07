//! Disk use and pruning: how much stored result each variant of a document holds, and the
//! explicit removal of stored results, planned first and carried out only when asked. Every
//! function takes the path of the document and returns a `DocOutcome`.
//!
//! Nothing here runs by itself and no other operation removes a stored result. [`prune`] is a dry
//! run unless `apply` is set. An applied prune removes only results that no pinned variant's run
//! names or its current stage keys resolve to and, when it is given variants, that no variant
//! outside them names. It records the removed hashes in the pruned list of the artifact index and
//! changes no run record, so the key numbers of every run stay.
//!
//! Finding codes of this family: 001 a variant named for pruning does not exist (class 2); 002 a
//! stored result could not be removed (class 4). A report names variants by id and never the path
//! of the store.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use cfdcraft_diskuse::{BlobHold, plan_prune};
use cfdcraft_doc::{Document, Opened, Stamp, open_stamped, save_if_unchanged, store_dir};
use cfdcraft_keys::Stage;
use cfdcraft_runs::{latest_mesh, latest_result};
use cfdcraft_scenarios::BASE;
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_text::Line;

use crate::documents::{
    DocOutcome, is_pack_name, migration_with, not_written, pack_kept, problem, refused, study_hash,
};
use crate::jobs::open_refusal;
use crate::report::{ExitClass, Report};
use crate::repro::{PruneData, Repro};
use crate::scenario_ops::keys_of;

const DISK: &str = "disk";
const PRUNE: &str = "prune";

/// Open the document at `path`. A pack is refused: its results are inside it, not in a store.
fn load(operation: &str, path: &Path) -> Result<(Opened, Stamp), Box<DocOutcome>> {
    let kept = || Box::new(refused(operation, problem(pack_kept(path))));
    if is_pack_name(path) {
        return Err(kept());
    }
    let (opened, stamp) =
        open_stamped(path).map_err(|e| Box::new(open_refusal(operation, &e, path)))?;
    if opened.packed_entries > 0 {
        return Err(kept());
    }
    Ok((opened, stamp))
}

/// Every stored result each run names, tagged with the variant the run was made for. A run that
/// made no mesh stands on one an earlier run made, and holds that mesh too: pinning the variant
/// keeps the mesh it stands on, not only what it made.
fn holds(document: &Document) -> Vec<BlobHold> {
    let sizes: HashMap<ContentHash, u64> = document
        .artifacts
        .blobs
        .iter()
        .map(|blob| (blob.hash, blob.size))
        .collect();
    let mut holds = Vec::new();
    for run in &document.artifacts.runs {
        let scenario = run.scenario.clone().unwrap_or_else(|| BASE.to_owned());
        let mut named: Vec<(ContentHash, u64, Stage)> = run
            .blob_uses()
            .into_iter()
            .map(|used| (used.hash, used.size, used.stage))
            .collect();
        if run.mesh.is_none()
            && let Some(&size) = sizes.get(&run.provenance.mesh_hash)
        {
            named.push((run.provenance.mesh_hash, size, Stage::Mesh));
        }
        for (hash, size, stage) in named {
            holds.push(BlobHold {
                run: run.id.clone(),
                scenario: scenario.clone(),
                hash,
                size,
                stage,
            });
        }
    }
    holds
}

/// The stored results the current stage keys of each pinned variant resolve to, found the way a
/// status finds them: a stage is fresh when any run holds its key, so a pinned variant whose keys
/// equal another variant's stands on that variant's results although none of its own runs names
/// them. Pinning the variant keeps those results too.
fn pinned_holds(document: &Document) -> Vec<BlobHold> {
    let runs = &document.artifacts.runs;
    let mut holds = Vec::new();
    for id in pinned(document) {
        let Some(keys) = keys_of(document, &id) else {
            continue;
        };
        let found = [
            latest_mesh(runs, keys.mesh).map(|(run, out)| (run, out.blob, out.size, Stage::Mesh)),
            latest_result(runs, keys.result)
                .map(|(run, out)| (run, out.blob, out.size, Stage::Result)),
        ];
        for (run, hash, size, stage) in found.into_iter().flatten() {
            holds.push(BlobHold {
                run: run.id.clone(),
                scenario: id.clone(),
                hash,
                size,
                stage,
            });
        }
    }
    holds
}

/// The ids of the variants (and `base`) whose record says they are pinned.
fn pinned(document: &Document) -> BTreeSet<String> {
    document
        .artifacts
        .scenarios
        .iter()
        .filter(|scenario| scenario.pinned)
        .map(|scenario| scenario.id.clone())
        .collect()
}

fn pruned(document: &Document) -> BTreeSet<ContentHash> {
    document.artifacts.pruned.iter().copied().collect()
}

/// What each variant of the document at `path` holds in the store, from the blobs its run records
/// name: bytes of mesh and result, how much of that another variant also holds, and what was
/// pruned. The file and the store are not read beyond the document, and nothing is changed.
pub fn disk_use(path: &Path) -> DocOutcome {
    let (opened, _) = match load(DISK, path) {
        Ok(loaded) => loaded,
        Err(outcome) => return *outcome,
    };
    let document = &opened.document;
    let usage = cfdcraft_diskuse::disk_use(&holds(document), &pinned(document), &pruned(document));
    let line = Line::DiskTotal {
        bytes: usage.stored_bytes.to_string(),
        scenarios: usage.scenarios.len().to_string(),
    };
    let report = Report::from_diagnostics(DISK, Vec::new(), study_hash(&document.study))
        .with_migration(migration_with(&opened.notes, false))
        .with_repro(Repro::Disk(Box::new(usage)));
    DocOutcome {
        report,
        line: Some(line),
    }
}

/// Plan the removal of stored results, and with `apply` carry it out.
///
/// `scope` names the variants to prune (`base` is the study itself); `None` is every variant. A
/// result is removable when it is not already pruned and every variant whose runs name it is in
/// the scope and none of them is pinned. Without `apply` this is a dry run: the plan and the bytes
/// it would free are reported and nothing is changed.
///
/// With `apply` the hashes about to be removed are recorded first, and only when that save
/// succeeds (the document is unchanged since it was read) are the files removed, in the order of
/// the plan, stopping at the first one that cannot be removed. A file that was already gone counts
/// as pruned too. A store that cannot be opened is refused before anything is recorded. A removal
/// that fails is reported as an infrastructure failure (class 4) and the hashes that were not
/// removed are taken back out of the record, so the same prune can be run again.
pub fn prune(path: &Path, scope: Option<&[String]>, apply: bool) -> DocOutcome {
    let (opened, stamp) = match load(PRUNE, path) {
        Ok(loaded) => loaded,
        Err(outcome) => return *outcome,
    };
    let mut document = opened.document;
    let hash = study_hash(&document.study);

    let wanted: Option<BTreeSet<String>> = scope.map(|ids| ids.iter().cloned().collect());
    let known = |id: &str| {
        id == BASE
            || document
                .artifacts
                .scenarios
                .iter()
                .any(|scenario| scenario.id == id)
    };
    if let Some(id) = wanted.iter().flatten().find(|id| !known(id.as_str())) {
        let shown_id: String = id.chars().take(40).collect();
        let line = Line::PruneRefused {
            reason: format!("there is no variant \"{shown_id}\""),
        };
        return refused(PRUNE, problem(("CFD-DISK-001", line)));
    }

    let mut named = holds(&document);
    named.extend(pinned_holds(&document));
    let mut plan = plan_prune(
        &named,
        &pinned(&document),
        &pruned(&document),
        wanted.as_ref(),
    );
    let report = |plan, applied, saved, diagnostics| {
        Report::from_diagnostics(PRUNE, diagnostics, hash.clone())
            .with_migration(migration_with(&opened.notes, saved))
            .with_repro(Repro::Prune(Box::new(PruneData { applied, plan })))
    };

    if !apply {
        let line = if plan.remove.is_empty() {
            Line::PruneNothing
        } else {
            Line::PruneDryRun {
                blobs: plan.remove.len().to_string(),
                bytes: plan.freed_bytes.to_string(),
            }
        };
        return DocOutcome {
            report: report(plan, false, false, Vec::new()),
            line: Some(line),
        };
    }

    if plan.remove.is_empty() {
        return DocOutcome {
            report: report(plan, true, false, Vec::new()),
            line: Some(Line::PruneNothing),
        };
    }
    // A store that cannot be opened (a document moved without it, a share that is not mounted)
    // removes nothing, so nothing is recorded either.
    let Ok(store) = Store::open(store_dir(path)) else {
        let reason = "the result store beside the document cannot be opened".to_owned();
        let found = problem(("CFD-DISK-002", Line::PruneNotRemoved { reason }));
        let mut failed = report(plan, false, false, vec![found]);
        failed.set_class(ExitClass::Infrastructure);
        return DocOutcome {
            report: failed,
            line: None,
        };
    };

    // The record first, the files after: the save is refused when the document changed since it
    // was read, and then nothing has been removed, so a run record saved in between can never
    // name a result that was removed under it. Ceiling: a crash after the save leaves files that
    // are recorded as pruned and still on disk, and a later prune does not retry them.
    let mut merged: Vec<ContentHash> = document
        .artifacts
        .pruned
        .iter()
        .copied()
        .chain(plan.remove.iter().map(|item| item.hash))
        .collect();
    merged.sort();
    merged.dedup();
    document.artifacts.pruned = merged;
    if let Err(e) = save_if_unchanged(&document, path, stamp) {
        return refused(PRUNE, problem(not_written(path, e)));
    }

    let mut done = Vec::new();
    let mut failure: Option<String> = None;
    let mut waiting: BTreeSet<ContentHash> = BTreeSet::new();
    for item in std::mem::take(&mut plan.remove) {
        if failure.is_some() {
            waiting.insert(item.hash);
            continue;
        }
        match store.remove(&item.hash) {
            Ok(true) => done.push(item),
            Ok(false) => {}
            Err(error) => {
                failure = Some(format!("the result {}: {error}", item.hash));
                waiting.insert(item.hash);
            }
        }
    }
    plan.freed_bytes = done
        .iter()
        .fold(0u64, |sum, item| sum.saturating_add(item.size));
    plan.remove = done;

    if let Some(reason) = failure {
        // Take what was not removed back out of the record, from the file as it is now, so the
        // same prune can be run again.
        let undone = open_stamped(path)
            .map_err(|e| e.to_string())
            .and_then(|(now, fresh)| {
                let mut again = now.document;
                again
                    .artifacts
                    .pruned
                    .retain(|hash| !waiting.contains(hash));
                save_if_unchanged(&again, path, fresh).map_err(|e| e.to_string())
            });
        let mut diagnostics = vec![problem(("CFD-DISK-002", Line::PruneNotRemoved { reason }))];
        if let Err(e) = undone {
            diagnostics.push(problem(not_written(path, e)));
        }
        let mut failed = report(plan, true, true, diagnostics);
        failed.set_class(ExitClass::Infrastructure);
        return DocOutcome {
            report: failed,
            line: None,
        };
    }
    let line = if plan.remove.is_empty() {
        Line::PruneNothing
    } else {
        Line::Pruned {
            blobs: plan.remove.len().to_string(),
            bytes: plan.freed_bytes.to_string(),
        }
    };
    DocOutcome {
        report: report(plan, true, true, Vec::new()),
        line: Some(line),
    }
}
