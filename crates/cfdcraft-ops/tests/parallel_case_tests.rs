//! Writing the case for a number of processes the caller chooses. One process writes exactly the
//! case written before; more add the decomposition and a parallel run script, and the case goes
//! through the same gates. No test needs a compute host or a container engine.

use cfdcraft_backend_esi::{
    CaseTree, Dialect,
    case::{CaseOptions, np_for},
};
use cfdcraft_ops::{JobAction, plan_job, prepare, prepare_with_options};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

fn names(tree: &CaseTree) -> Vec<String> {
    tree.files
        .iter()
        .map(|f| f.rel_path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn text(tree: &CaseTree, rel: &str) -> String {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path.to_string_lossy().replace('\\', "/") == rel)
        .unwrap_or_else(|| panic!("the case has {rel}"));
    String::from_utf8(file.bytes.clone()).unwrap()
}

#[test]
fn prepare_with_options_one_process_is_prepare() {
    let plain = prepare(CAVITY, Dialect::EsiV2512).unwrap();
    let one = prepare_with_options(CAVITY, Dialect::EsiV2512, &CaseOptions { np: 1 }).unwrap();
    assert_eq!(one.tree, plain.tree);
    assert_eq!(one.export_tree, plain.export_tree);
    assert_eq!(one.hash, plain.hash);
}

#[test]
fn two_processes_write_the_decomposition_and_the_parallel_allrun() {
    let two = prepare_with_options(CAVITY, Dialect::EsiV2512, &CaseOptions { np: 2 })
        .expect("the case passes the gates for two processes");
    assert!(names(&two.tree).contains(&"system/decomposeParDict".to_owned()));
    assert!(names(&two.export_tree).contains(&"system/decomposeParDict".to_owned()));
    let decompose = text(&two.export_tree, "system/decomposeParDict");
    assert!(decompose.contains("numberOfSubdomains 2;"), "{decompose}");
    assert!(text(&two.export_tree, "Allrun").contains("mpirun -np 2"));

    // One process has neither.
    let one = prepare(CAVITY, Dialect::EsiV2512).unwrap();
    assert!(!names(&one.export_tree).contains(&"system/decomposeParDict".to_owned()));
    assert!(!text(&one.export_tree, "Allrun").contains("mpirun"));

    // The study hash is the same: the process count is not part of the study.
    assert_eq!(two.hash, one.hash);

    // No processes is not a case.
    assert!(prepare_with_options(CAVITY, Dialect::EsiV2512, &CaseOptions { np: 0 }).is_err());
}

#[test]
fn the_cell_count_and_the_np_helper_are_exposed() {
    let dir = scratch("parallel-cells");
    let path = new_document(&dir, "cavity", CAVITY);
    let job = plan_job(&path, JobAction::Solve).expect("a fresh document has work to do");
    assert_eq!(job.cell_count(), 16384);

    assert_eq!(np_for(job.cell_count(), 2), 1);
    assert_eq!(np_for(100_000, 2), 2);
    assert_eq!(np_for(100_000, 8), 5);
    assert_eq!(np_for(10, 8), 1);
}
