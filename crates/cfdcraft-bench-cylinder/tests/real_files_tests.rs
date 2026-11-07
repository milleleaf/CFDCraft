//! The cylinder comparer over the force coefficient and pressure probe files a real solver wrote.
//!
//! The files come from sixty time steps of 1 ms at the coarse mesh: the flow has not started to
//! shed, so the comparer must read them and then refuse to judge. Only that is asserted.

mod support;

use cfdcraft_bench_cylinder::CylinderBenchmark;
use cfdcraft_bench_cylinder::fixture::{Run, tree};
use cfdcraft_gate::Benchmark;
use support::{Tree, study};

const COEFFICIENTS: &str =
    include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-cylinder-coefficient.dat");
const FRONT: &str =
    include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-cylinder-probe_1-p.dat");
const REAR: &str =
    include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-cylinder-probe_2-p.dat");

fn real_tree() -> Tree {
    let mut files = Tree::default();
    files.put(
        "postProcessing/force_1/0/coefficient.dat",
        COEFFICIENTS.to_owned(),
    );
    files.put("postProcessing/probe_1/0/p", FRONT.to_owned());
    files.put("postProcessing/probe_2/0/p", REAR.to_owned());
    files
}

#[test]
fn the_study_asks_for_the_files_the_solver_wrote() {
    let paths: Vec<String> = tree(&study(), &Run::good())
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert_eq!(
        paths,
        [
            "postProcessing/force_1/0/coefficient.dat",
            "postProcessing/probe_1/0/p",
            "postProcessing/probe_2/0/p"
        ]
    );
}

#[test]
fn the_real_coefficient_file_is_read() {
    let report = CylinderBenchmark::shipped()
        .unwrap()
        .compare(&real_tree(), &study());
    let reason = report.reason.expect("a run that has not shed is refused");
    // The files were found, the header states a lift along y and the series were read: the only
    // thing missing is whole shedding cycles.
    assert!(reason.contains("CFD-BENCH-004"), "{reason}");
    assert!(reason.contains("whole shedding cycles"), "{reason}");
    assert!(!reason.contains("CFD-BENCH-002"), "{reason}");
    assert!(!reason.contains("CFD-BENCH-003"), "{reason}");
    assert!(report.metrics.is_empty());
}
