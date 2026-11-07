use std::collections::BTreeSet;

use cfdcraft_diskuse::{BlobHold, plan_prune};
use cfdcraft_keys::Stage;
use cfdcraft_store::ContentHash;

fn hold(run: &str, scenario: &str, blob: &str, size: u64, stage: Stage) -> BlobHold {
    BlobHold {
        run: run.to_owned(),
        scenario: scenario.to_owned(),
        hash: ContentHash::of(blob.as_bytes()),
        size,
        stage,
    }
}

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn none() -> BTreeSet<ContentHash> {
    BTreeSet::new()
}

#[test]
fn nothing_is_removed_when_every_scenario_is_pinned() {
    let holds = [
        hold("r1", "base", "mesh", 100, Stage::Mesh),
        hold("r1", "base", "res-a", 40, Stage::Result),
        hold("r2", "fast-lid", "res-b", 30, Stage::Result),
    ];
    let plan = plan_prune(&holds, &names(&["base", "fast-lid"]), &none(), None);
    assert!(plan.remove.is_empty());
    assert_eq!(
        (plan.freed_bytes, plan.kept_pinned, plan.kept_shared),
        (0, 3, 0)
    );
}

#[test]
fn an_unpinned_scenario_loses_its_blobs() {
    let holds = [
        hold("r1", "base", "mesh", 100, Stage::Mesh),
        hold("r1", "base", "res-a", 40, Stage::Result),
        hold("r2", "fast-lid", "mesh", 100, Stage::Mesh),
        hold("r2", "fast-lid", "res-b", 30, Stage::Result),
    ];
    let plan = plan_prune(&holds, &names(&["base"]), &none(), None);
    assert_eq!(plan.remove.len(), 1);
    assert_eq!(plan.remove[0].hash, ContentHash::of(b"res-b"));
    assert_eq!(plan.remove[0].stage, Stage::Result);
    assert_eq!(plan.freed_bytes, 30);
    // The shared mesh and base's own result.
    assert_eq!(plan.kept_pinned, 2);
}

#[test]
fn a_scope_limits_the_plan_to_the_named_scenarios() {
    let holds = [
        hold("r1", "a", "res-a", 10, Stage::Result),
        hold("r2", "fast-lid", "res-b", 20, Stage::Result),
    ];
    let scope = names(&["fast-lid"]);
    let scoped = plan_prune(&holds, &BTreeSet::new(), &none(), Some(&scope));
    assert_eq!(scoped.remove.len(), 1);
    assert_eq!(scoped.remove[0].scenarios, ["fast-lid"]);
    assert_eq!(scoped.kept_shared, 1);

    let all = plan_prune(&holds, &BTreeSet::new(), &none(), None);
    assert_eq!((all.remove.len(), all.freed_bytes), (2, 30));
}

#[test]
fn a_blob_used_by_an_out_of_scope_scenario_is_kept_and_counted() {
    let holds = [
        hold("r1", "a", "mesh", 100, Stage::Mesh),
        hold("r1", "a", "res-a", 10, Stage::Result),
        hold("r2", "b", "mesh", 100, Stage::Mesh),
    ];
    let scope = names(&["a"]);
    let plan = plan_prune(&holds, &BTreeSet::new(), &none(), Some(&scope));
    assert_eq!(plan.remove.len(), 1);
    assert_eq!(plan.remove[0].hash, ContentHash::of(b"res-a"));
    assert_eq!((plan.kept_shared, plan.kept_pinned), (1, 0));
}

#[test]
fn an_already_pruned_blob_is_not_planned_again() {
    let holds = [
        hold("r1", "a", "res-a", 10, Stage::Result),
        hold("r1", "a", "res-b", 20, Stage::Result),
    ];
    let pruned: BTreeSet<ContentHash> = [ContentHash::of(b"res-a")].into();
    let plan = plan_prune(&holds, &BTreeSet::new(), &pruned, None);
    assert_eq!(plan.remove.len(), 1);
    assert_eq!(plan.remove[0].hash, ContentHash::of(b"res-b"));
    assert_eq!(plan.freed_bytes, 20);
}

#[test]
fn the_plan_is_sorted_by_size_then_hash_and_names_its_scenarios() {
    let holds = [
        hold("r1", "zeta", "small", 5, Stage::Result),
        hold("r2", "alpha", "big", 50, Stage::Mesh),
        hold("r3", "zeta", "big", 50, Stage::Mesh),
        hold("r4", "alpha", "tie-a", 7, Stage::Result),
        hold("r4", "alpha", "tie-b", 7, Stage::Result),
    ];
    let plan = plan_prune(&holds, &BTreeSet::new(), &none(), None);
    let sizes: Vec<u64> = plan.remove.iter().map(|r| r.size).collect();
    assert_eq!(sizes, [50, 7, 7, 5]);
    assert_eq!(plan.remove[0].scenarios, ["alpha", "zeta"]);
    assert!(plan.remove[1].hash < plan.remove[2].hash);
    assert_eq!(plan.freed_bytes, 69);
}

#[test]
fn a_run_of_a_pinned_scenario_protects_all_its_blobs_even_when_shared_with_an_unpinned_one() {
    let holds = [
        hold("r1", "keep", "mesh", 100, Stage::Mesh),
        hold("r1", "keep", "res", 40, Stage::Result),
        hold("r2", "other", "mesh", 100, Stage::Mesh),
        hold("r2", "other", "res", 40, Stage::Result),
    ];
    let scope = names(&["other"]);
    let plan = plan_prune(&holds, &names(&["keep"]), &none(), Some(&scope));
    assert!(plan.remove.is_empty());
    assert_eq!((plan.kept_pinned, plan.kept_shared), (2, 0));
}

#[test]
fn planning_is_deterministic() {
    let holds = [
        hold("r1", "a", "mesh", 100, Stage::Mesh),
        hold("r1", "a", "res-a", 10, Stage::Result),
        hold("r2", "b", "mesh", 100, Stage::Mesh),
        hold("r2", "b", "res-b", 20, Stage::Result),
    ];
    let mut reversed = holds.clone();
    reversed.reverse();
    let pinned = names(&["b"]);
    assert_eq!(
        plan_prune(&holds, &pinned, &none(), None),
        plan_prune(&reversed, &pinned, &none(), None)
    );
}

#[test]
fn sizes_add_without_overflow() {
    let holds = [
        hold("r1", "a", "x", u64::MAX, Stage::Mesh),
        hold("r1", "a", "y", 9, Stage::Result),
    ];
    let plan = plan_prune(&holds, &BTreeSet::new(), &none(), None);
    assert_eq!(plan.freed_bytes, u64::MAX);
}
