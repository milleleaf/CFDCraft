//! The step comparer over the sampled lines a real solver wrote.
//!
//! The lines come from sixty iterations of the turbulent step at the coarse mesh: far from a
//! solution, so only what the files show is asserted (their layout and what the readers make of
//! it), never whether the benchmark passes. See `PROVENANCE.md` in the monitors fixtures.

mod support;

use cfdcraft_bench_step::StepBenchmark;
use cfdcraft_gate::Benchmark;
use cfdcraft_model::ResolvedStudy;
use support::{Ds, MapFiles, line_path};

const FLOOR: &str =
    include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-step-line_1-U.xy");
const X1: &str = include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-step-line_2-U.xy");
const X10: &str =
    include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-step-line_3-U.xy");
const X4: &str = include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-step-line_4-U.xy");
const X6: &str = include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-step-line_5-U.xy");

/// The five lines under the folder of the last write, 60. The solver numbers its line monitors in
/// label order: `floor`, `profile_x1`, `profile_x10`, `profile_x4`, `profile_x6`.
fn real_tree(r: &ResolvedStudy) -> MapFiles {
    let mut files = MapFiles::default();
    for (label, text) in [
        ("floor", FLOOR),
        ("profile_x1", X1),
        ("profile_x10", X10),
        ("profile_x4", X4),
        ("profile_x6", X6),
    ] {
        files.0.insert(line_path(r, label, "60"), text.to_owned());
    }
    files
}

/// Where the last sign change of Ux along the floor line lies, in step heights from the step,
/// worked out from the file with the comparer's rule (linear between the two samples).
const REATTACHMENT_OVER_H: f64 = 9.844544413785696;

#[test]
fn the_study_asks_for_the_line_numbers_the_solver_wrote() {
    let r = Ds::default().resolved();
    for (label, number) in [
        ("floor", 1),
        ("profile_x1", 2),
        ("profile_x10", 3),
        ("profile_x4", 4),
        ("profile_x6", 5),
    ] {
        assert_eq!(
            line_path(&r, label, "60"),
            format!("postProcessing/line_{number}/60/line_U.xy"),
            "{label}"
        );
    }
}

#[test]
fn the_real_six_column_floor_line_gives_a_reattachment_length() {
    let r = Ds::default().resolved();
    let quantities = StepBenchmark::new().unwrap().quantities(&real_tree(&r), &r);
    assert_eq!(quantities.len(), 1);
    assert_eq!(quantities[0].0, "bench.step.reattachment");
    assert!(
        (quantities[0].1 - REATTACHMENT_OVER_H).abs() < 1e-6,
        "{}",
        quantities[0].1
    );
}

#[test]
fn the_real_lines_are_compared_and_not_refused() {
    let r = Ds::default().resolved();
    let report = StepBenchmark::new().unwrap().compare(&real_tree(&r), &r);
    assert_eq!(report.reason, None);
    let names: Vec<&str> = report.metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "x_R/H",
            "u/Uref x/H=1",
            "u/Uref x/H=4",
            "u/Uref x/H=6",
            "u/Uref x/H=10"
        ]
    );
    assert!((report.metrics[0].rows[0].computed - REATTACHMENT_OVER_H).abs() < 1e-6);
}
