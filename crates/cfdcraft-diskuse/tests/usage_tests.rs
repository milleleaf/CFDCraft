use std::collections::BTreeSet;

use cfdcraft_diskuse::{BlobHold, disk_use};
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

#[test]
fn one_scenario_with_one_run_counts_its_two_blobs() {
    let holds = [
        hold("run-1", "base", "mesh-a", 100, Stage::Mesh),
        hold("run-1", "base", "result-a", 40, Stage::Result),
    ];
    let report = disk_use(&holds, &BTreeSet::new(), &BTreeSet::new());
    assert_eq!(report.scenarios.len(), 1);
    let base = &report.scenarios[0];
    assert_eq!(base.scenario, "base");
    assert_eq!(
        (base.bytes, base.mesh_bytes, base.result_bytes),
        (140, 100, 40)
    );
    assert_eq!((base.runs, base.shared_bytes, base.pruned_bytes), (1, 0, 0));
    assert_eq!((report.stored_bytes, report.pruned_bytes), (140, 0));
}

#[test]
fn a_blob_named_twice_by_one_scenario_counts_once() {
    let holds = [
        hold("run-1", "base", "mesh-a", 100, Stage::Mesh),
        hold("run-2", "base", "mesh-a", 100, Stage::Mesh),
    ];
    let report = disk_use(&holds, &BTreeSet::new(), &BTreeSet::new());
    let base = &report.scenarios[0];
    assert_eq!((base.bytes, base.runs), (100, 2));
    assert_eq!(report.stored_bytes, 100);
}

#[test]
fn a_blob_shared_by_two_scenarios_counts_for_each_and_once_in_the_total() {
    let holds = [
        hold("run-1", "base", "mesh-a", 100, Stage::Mesh),
        hold("run-2", "fast-lid", "mesh-a", 100, Stage::Mesh),
    ];
    let report = disk_use(&holds, &BTreeSet::new(), &BTreeSet::new());
    for used in &report.scenarios {
        assert_eq!((used.bytes, used.shared_bytes), (100, 100), "{used:?}");
    }
    assert_eq!(report.stored_bytes, 100);
}

#[test]
fn a_pruned_blob_is_not_stored_and_is_shown_as_pruned() {
    let holds = [
        hold("run-1", "base", "mesh-a", 100, Stage::Mesh),
        hold("run-1", "base", "result-a", 40, Stage::Result),
    ];
    let pruned: BTreeSet<ContentHash> = [ContentHash::of(b"result-a")].into();
    let report = disk_use(&holds, &BTreeSet::new(), &pruned);
    let base = &report.scenarios[0];
    assert_eq!(
        (base.bytes, base.result_bytes, base.pruned_bytes),
        (100, 0, 40)
    );
    assert_eq!((report.stored_bytes, report.pruned_bytes), (100, 40));
}

#[test]
fn the_scenarios_come_out_in_a_fixed_order_with_base_first() {
    let holds = [
        hold("r1", "zeta", "z", 1, Stage::Result),
        hold("r2", "alpha", "a", 1, Stage::Result),
        hold("r3", "base", "b", 1, Stage::Result),
    ];
    let report = disk_use(&holds, &names(&["alpha"]), &BTreeSet::new());
    let order: Vec<(&str, bool)> = report
        .scenarios
        .iter()
        .map(|s| (s.scenario.as_str(), s.pinned))
        .collect();
    assert_eq!(order, [("base", false), ("alpha", true), ("zeta", false)]);
}

#[test]
fn no_holds_give_an_empty_report() {
    let report = disk_use(&[], &BTreeSet::new(), &BTreeSet::new());
    assert!(report.scenarios.is_empty());
    assert_eq!((report.stored_bytes, report.pruned_bytes), (0, 0));
}

#[test]
fn sizes_add_without_overflow() {
    let holds = [
        hold("r1", "base", "a", u64::MAX, Stage::Mesh),
        hold("r1", "base", "b", 5, Stage::Result),
    ];
    let report = disk_use(&holds, &BTreeSet::new(), &BTreeSet::new());
    assert_eq!(report.scenarios[0].bytes, u64::MAX);
    assert_eq!(report.stored_bytes, u64::MAX);
}

#[test]
fn the_report_is_json_with_stable_names() {
    let holds = [hold("run-1", "base", "mesh-a", 100, Stage::Mesh)];
    let value = serde_json::to_value(disk_use(&holds, &BTreeSet::new(), &BTreeSet::new()))
        .expect("serialises");
    let object = value.as_object().expect("an object");
    assert_eq!(
        object.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["scenarios", "stored_bytes", "pruned_bytes"])
    );
    let first = value["scenarios"][0].as_object().expect("an object");
    assert_eq!(
        first.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "scenario",
            "pinned",
            "runs",
            "bytes",
            "mesh_bytes",
            "result_bytes",
            "shared_bytes",
            "pruned_bytes"
        ])
    );
}
