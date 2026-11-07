//! Disk use per variant, and pruning: the dry run changes nothing, an applied prune removes only
//! what no pinned variant needs, and every run record stays. No test needs a compute host or a
//! container engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_diskuse::DiskUse;
use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::disk_ops::{disk_use, prune};
use cfdcraft_ops::scenario_ops::{LabelUpdate, scenario_label, scenario_set};
use cfdcraft_ops::{ExitClass, PruneData, Report, Repro, solve_document, solve_scenario};
use cfdcraft_scenarios::Override;
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_text::Line;
use serde_json::{Value, json};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";

fn set(setting: &str, value: Value) -> Override {
    Override::Set {
        setting: setting.to_owned(),
        value,
    }
}

/// The five stored results of the solved document, each named by one run.
struct Blobs {
    /// The study's mesh, which `fast-lid` stands on too.
    mesh: ContentHash,
    /// The study's result.
    result: ContentHash,
    /// The result of `fast-lid`.
    fast: ContentHash,
    /// The mesh of `fine`.
    fine_mesh: ContentHash,
    /// The result of `fine`.
    fine: ContentHash,
}

impl Blobs {
    fn all(&self) -> [ContentHash; 5] {
        [self.mesh, self.result, self.fast, self.fine_mesh, self.fine]
    }
}

fn blobs(path: &Path) -> Blobs {
    let runs = open(path).unwrap().document.artifacts.runs;
    let mesh = |n: usize| runs[n].mesh.as_ref().unwrap().blob;
    let result = |n: usize| runs[n].result.as_ref().unwrap().blob;
    let found = Blobs {
        mesh: mesh(0),
        result: result(0),
        fast: result(1),
        fine_mesh: mesh(2),
        fine: result(2),
    };
    let distinct: std::collections::BTreeSet<ContentHash> = found.all().into_iter().collect();
    assert_eq!(distinct.len(), 5, "the five stored results all differ");
    found
}

/// A document of the cavity solved three times: the study itself (`run-1`, mesh and result),
/// `fast-lid` (`run-2`, a result on the study's mesh) and `fine` (`run-3`, its own mesh and result).
fn solved(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = vec![0.0, 0.05, 0.1];
    passed(&solve_document(&path, &mut runner).report);
    let made = [
        ("fast-lid", set(LID, json!(0.2))),
        ("fine", set(CELLS_X, json!(256))),
    ];
    for (id, change) in made {
        passed(&scenario_set(&path, id, Some("base"), &[change], None).report);
        passed(&solve_scenario(&path, Some(id), &mut runner).report);
    }
    path
}

fn pin(path: &Path, id: &str) {
    let update = LabelUpdate {
        pinned: Some(true),
        ..LabelUpdate::default()
    };
    passed(&scenario_label(path, id, &update).report);
}

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn usage(report: &Report) -> &DiskUse {
    match &report.repro {
        Some(Repro::Disk(data)) => data.as_ref(),
        other => panic!("expected the disk use, found {other:?}"),
    }
}

fn pruning(report: &Report) -> &PruneData {
    match &report.repro {
        Some(Repro::Prune(data)) => data.as_ref(),
        other => panic!("expected the data of a prune, found {other:?}"),
    }
}

fn stored(path: &Path, hash: &ContentHash) -> bool {
    Store::open(store_dir(path)).unwrap().contains(hash)
}

fn sorted(mut hashes: Vec<ContentHash>) -> Vec<ContentHash> {
    hashes.sort();
    hashes
}

#[test]
fn disk_use_lists_each_variant_with_mesh_and_result_bytes() {
    let path = solved("disk-use");
    let runs = open(&path).unwrap().document.artifacts.runs;
    let mesh = |n: usize| runs[n].mesh.as_ref().unwrap().size;
    let result = |n: usize| runs[n].result.as_ref().unwrap().size;

    let done = disk_use(&path);
    passed(&done.report);
    assert_eq!(done.report.operation, "disk");
    let found = usage(&done.report);
    let names: Vec<&str> = found
        .scenarios
        .iter()
        .map(|s| s.scenario.as_str())
        .collect();
    assert_eq!(names, ["base", "fast-lid", "fine"]);
    let (base, fast, fine) = (
        &found.scenarios[0],
        &found.scenarios[1],
        &found.scenarios[2],
    );

    assert_eq!(base.runs, 1);
    assert_eq!(base.mesh_bytes, mesh(0));
    assert_eq!(base.result_bytes, result(0));
    assert_eq!(base.bytes, mesh(0) + result(0));
    assert_eq!(base.shared_bytes, mesh(0));

    assert_eq!(fast.runs, 1);
    assert_eq!(fast.mesh_bytes, mesh(0));
    assert_eq!(fast.result_bytes, result(1));
    assert_eq!(fast.shared_bytes, mesh(0));

    assert_eq!(fine.runs, 1);
    assert_eq!(fine.mesh_bytes, mesh(2));
    assert_eq!(fine.result_bytes, result(2));
    assert_eq!(fine.shared_bytes, 0);

    // The shared mesh is counted once in the total.
    let total = mesh(0) + result(0) + result(1) + mesh(2) + result(2);
    assert_eq!(found.stored_bytes, total);
    assert_eq!(found.pruned_bytes, 0);
    assert_eq!(
        done.line,
        Some(Line::DiskTotal {
            bytes: total.to_string(),
            scenarios: "3".to_owned(),
        })
    );
}

#[test]
fn a_dry_run_changes_nothing_and_says_so() {
    let path = solved("prune-dry");
    let before = snapshot(&path);

    let done = prune(&path, None, false);
    passed(&done.report);
    assert_eq!(done.report.operation, "prune");
    let data = pruning(&done.report);
    assert!(!data.applied);
    assert_eq!(data.plan.remove.len(), 5);
    let freed: u64 = data.plan.remove.iter().map(|item| item.size).sum();
    assert_eq!(data.plan.freed_bytes, freed);
    assert_eq!(
        done.line,
        Some(Line::PruneDryRun {
            blobs: "5".to_owned(),
            bytes: freed.to_string(),
        })
    );
    assert_eq!(snapshot(&path), before);
}

#[test]
fn apply_removes_unpinned_results_and_keeps_pinned_ones() {
    let path = solved("prune-apply");
    pin(&path, "fast-lid");
    let found = blobs(&path);

    let done = prune(&path, None, true);
    passed(&done.report);
    let data = pruning(&done.report);
    assert!(data.applied);
    assert_eq!(data.plan.remove.len(), 3);
    assert_eq!(data.plan.kept_pinned, 2);
    assert!(matches!(done.line, Some(Line::Pruned { blobs: ref count, .. }) if count == "3"));

    for gone in [found.result, found.fine_mesh, found.fine] {
        assert!(!stored(&path, &gone));
    }
    // The pinned variant keeps its result and the mesh it stands on.
    for kept in [found.mesh, found.fast] {
        assert!(stored(&path, &kept));
        Store::open(store_dir(&path))
            .unwrap()
            .verify(&kept)
            .unwrap();
    }
}

#[test]
fn every_run_record_and_its_key_numbers_stay() {
    let path = solved("prune-records");
    pin(&path, "fast-lid");
    let found = blobs(&path);
    let before = open(&path).unwrap().document.artifacts;

    passed(&prune(&path, None, true).report);

    let after = open(&path).unwrap().document.artifacts;
    assert_eq!(after.runs, before.runs);
    assert_eq!(after.blobs, before.blobs);
    assert_eq!(
        after.pruned,
        sorted(vec![found.result, found.fine_mesh, found.fine])
    );
}

#[test]
fn a_scope_prunes_only_the_named_variants() {
    let path = solved("prune-scope-fine");
    let found = blobs(&path);
    let scope = ids(&["fine"]);
    let done = prune(&path, Some(scope.as_slice()), true);
    passed(&done.report);
    assert_eq!(
        open(&path).unwrap().document.artifacts.pruned,
        sorted(vec![found.fine_mesh, found.fine])
    );
    for kept in [found.mesh, found.result, found.fast] {
        assert!(stored(&path, &kept));
    }

    // The mesh of `fast-lid` is also the study's, which is outside the scope.
    let path = solved("prune-scope-fast");
    let found = blobs(&path);
    let scope = ids(&["fast-lid"]);
    let done = prune(&path, Some(scope.as_slice()), true);
    passed(&done.report);
    let data = pruning(&done.report);
    assert_eq!(data.plan.remove.len(), 1);
    // every blob that a scenario outside the scope names is kept: the study's mesh and result
    // (the mesh is shared with `fast-lid`) and the two blobs of `fine`
    assert_eq!(data.plan.kept_shared, 4);
    assert_eq!(
        open(&path).unwrap().document.artifacts.pruned,
        vec![found.fast]
    );
    for kept in [found.mesh, found.result, found.fine_mesh, found.fine] {
        assert!(stored(&path, &kept));
    }
}

#[test]
fn an_unknown_variant_in_the_scope_is_cfd_disk_001() {
    let path = solved("prune-unknown");
    let before = snapshot(&path);
    let scope = ids(&["fine", "nope"]);
    for apply in [false, true] {
        let done = prune(&path, Some(scope.as_slice()), apply);
        assert_eq!(done.report.class(), ExitClass::StudyInvalid);
        assert_eq!(codes(&done.report), ["CFD-DISK-001"]);
        assert!(done.report.diagnostics[0].message.contains("nope"));
        assert_eq!(snapshot(&path), before);
    }
}

#[test]
fn pruning_twice_removes_nothing_more() {
    let path = solved("prune-twice");
    passed(&prune(&path, None, true).report);
    let after_first = snapshot(&path);

    let again = prune(&path, None, true);
    passed(&again.report);
    assert_eq!(again.line, Some(Line::PruneNothing));
    assert!(pruning(&again.report).plan.remove.is_empty());
    assert_eq!(snapshot(&path), after_first);
}

#[test]
fn a_blob_already_missing_from_the_store_is_recorded_as_pruned_without_error() {
    let path = solved("prune-absent");
    let found = blobs(&path);
    let store = Store::open(store_dir(&path)).unwrap();
    fs::remove_file(store.blob_path(&found.result)).unwrap();

    let done = prune(&path, None, true);
    passed(&done.report);
    let pruned = open(&path).unwrap().document.artifacts.pruned;
    assert!(pruned.contains(&found.result));
    assert_eq!(pruned, sorted(found.all().to_vec()));
    // Only what was really there is counted as removed.
    assert_eq!(pruning(&done.report).plan.remove.len(), 4);
}

#[test]
fn a_failed_removal_is_cfd_disk_002_and_records_only_what_went() {
    let path = solved("prune-failed");
    let all = blobs(&path).all();
    let plan = prune(&path, None, false);
    let order: Vec<ContentHash> = pruning(&plan.report)
        .plan
        .remove
        .iter()
        .map(|item| item.hash)
        .collect();
    assert_eq!(order.len(), 5);

    // The last blob of the plan becomes a folder with a file in it, which cannot be removed.
    let stuck = *order.last().unwrap();
    let blob = Store::open(store_dir(&path)).unwrap().blob_path(&stuck);
    fs::remove_file(&blob).unwrap();
    fs::create_dir(&blob).unwrap();
    fs::write(blob.join("inside"), b"x").unwrap();

    let done = prune(&path, None, true);
    assert_eq!(done.report.class(), ExitClass::Infrastructure);
    assert_eq!(codes(&done.report), ["CFD-DISK-002"]);
    let data = pruning(&done.report);
    assert!(data.applied);
    assert_eq!(data.plan.remove.len(), 4);

    let went: Vec<ContentHash> = all.into_iter().filter(|hash| *hash != stuck).collect();
    assert_eq!(
        open(&path).unwrap().document.artifacts.pruned,
        sorted(went.clone())
    );
    for hash in &went {
        assert!(!stored(&path, hash));
    }
    assert!(blob.join("inside").is_file());
}

#[test]
fn the_study_itself_is_pinned_when_its_base_record_says_so() {
    let path = solved("prune-base-pinned");
    pin(&path, "base");
    let found = blobs(&path);

    passed(&prune(&path, None, true).report);

    assert_eq!(
        open(&path).unwrap().document.artifacts.pruned,
        sorted(vec![found.fast, found.fine_mesh, found.fine])
    );
    for kept in [found.mesh, found.result] {
        assert!(stored(&path, &kept));
    }
}
