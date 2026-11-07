mod support;

use cfdcraft_bench_cylinder::fixture::{Run, coefficient_file};
use cfdcraft_bench_cylinder::{Measured, Refusal, Tolerances, measure};
use support::{Tree, close, study};

fn tolerances() -> Tolerances {
    Tolerances::default_embedded().unwrap()
}

fn measured(tree: &Tree) -> Result<Measured, Refusal> {
    measure(tree, &study(), &tolerances())
}

#[test]
fn a_synthetic_shedding_run_gives_the_known_numbers() {
    let m = measured(&Tree::of(&study(), &Run::good())).unwrap();
    assert!(close(m.st, 0.3, 2e-3), "{m:?}");
    assert!(close(m.cl_max, 1.0, 2e-3), "{m:?}");
    assert!(close(m.cd_max, 3.24, 2e-3), "{m:?}");
    assert!(close(m.delta_p, 2.48, 2e-3), "{m:?}");
    assert!(m.cycles >= 23, "{m:?}");
}

#[test]
fn restart_folders_are_joined() {
    let run = Run::good();
    let single = measured(&Tree::of(&study(), &run)).unwrap();
    let mut split = Tree::of(&study(), &run);
    // The first folder runs on 0.1 s past the start of the second.
    split.put(
        "postProcessing/force_1/0/coefficient.dat",
        coefficient_file(&run, 0..=4100),
    );
    split.put(
        "postProcessing/force_1/4/coefficient.dat",
        coefficient_file(&run, 4000..=8000),
    );
    // A folder that is not a start time is not read.
    split.put(
        "postProcessing/force_1/notes/coefficient.dat",
        "not a coefficient file".to_owned(),
    );
    assert_eq!(measured(&split).unwrap(), single);
}

#[test]
fn a_wrong_lift_axis_is_refused() {
    for direction in [Some("(0 0 1)"), Some("(1 0 0)"), None] {
        let run = Run {
            lift_dir: direction,
            ..Run::good()
        };
        let refusal = measured(&Tree::of(&study(), &run)).unwrap_err();
        assert_eq!(refusal, Refusal::LiftDirection, "{direction:?}");
        assert!(refusal.to_string().starts_with("CFD-BENCH-003"));
    }
}

#[test]
fn too_few_cycles_is_refused() {
    let run = Run {
        steps: 2500,
        ..Run::good()
    };
    let refusal = measured(&Tree::of(&study(), &run)).unwrap_err();
    assert_eq!(
        refusal,
        Refusal::TooFewCycles {
            found: 7,
            needed: 10
        }
    );
    assert!(refusal.to_string().starts_with("CFD-BENCH-004"));
}

#[test]
fn a_lift_that_has_not_started_to_shed_is_refused() {
    // A flat lift has no cycle; it must not be judged on looking settled.
    let run = Run {
        lift_amplitude: 0.0,
        ..Run::good()
    };
    let refusal = measured(&Tree::of(&study(), &run)).unwrap_err();
    assert!(
        matches!(refusal, Refusal::TooFewCycles { needed: 10, .. }),
        "{refusal:?}"
    );
}

#[test]
fn a_drifting_lift_is_refused() {
    let run = Run {
        growth: 0.05,
        ..Run::good()
    };
    let refusal = measured(&Tree::of(&study(), &run)).unwrap_err();
    assert_eq!(refusal, Refusal::Drifting);
    assert!(refusal.to_string().starts_with("CFD-BENCH-004"));
}

#[test]
fn missing_or_bad_files_are_named() {
    let good = Tree::of(&study(), &Run::good());

    let mut no_force = Tree::of(&study(), &Run::good());
    no_force.remove_prefix("postProcessing/force_1/");
    assert_eq!(
        measured(&no_force).unwrap_err(),
        Refusal::Missing {
            file: "postProcessing/force_1".to_owned()
        }
    );

    let mut no_rear = Tree::of(&study(), &Run::good());
    no_rear.remove_prefix("postProcessing/probe_2/");
    assert_eq!(
        measured(&no_rear).unwrap_err(),
        Refusal::Missing {
            file: "postProcessing/probe_2".to_owned()
        }
    );

    let mut no_lift = Tree::of(&study(), &Run::good());
    no_lift.put(
        "postProcessing/force_1/0/coefficient.dat",
        "# liftDir : (0 1 0)\n# Time\tCd\n0.000000\t3.23\n".to_owned(),
    );
    let refusal = measured(&no_lift).unwrap_err();
    assert_eq!(
        refusal,
        Refusal::Unreadable {
            file: "postProcessing/force_1/0/coefficient.dat".to_owned()
        }
    );
    assert!(refusal.to_string().starts_with("CFD-BENCH-002"));
    assert!(refusal.to_string().contains("postProcessing/force_1"));

    let mut nan = Tree::of(&study(), &Run::good());
    nan.put(
        "postProcessing/probe_1/0/p",
        "# Probe 0 (0.15 0.2 0.005)\n#       Probe             0\n#        Time\n0.000000   nan\n"
            .to_owned(),
    );
    assert_eq!(
        measured(&nan).unwrap_err(),
        Refusal::Unreadable {
            file: "postProcessing/probe_1/0/p".to_owned()
        }
    );

    // The untouched tree still measures.
    assert!(measured(&good).is_ok());
}

#[test]
fn the_pressure_difference_uses_the_front_and_rear_probes_by_label() {
    let s = study();
    let tree = Tree::of(&s, &Run::good());
    let normal = measure(&tree, &s, &tolerances()).unwrap();
    assert!(normal.delta_p > 2.0);

    // The labels swap, so the probe numbered first is now the rear one.
    let mut swapped = s.clone();
    swapped.monitors.probes[0].label = "rear".to_owned();
    swapped.monitors.probes[1].label = "front".to_owned();
    let flipped = measure(&tree, &swapped, &tolerances()).unwrap();
    assert!(close(flipped.delta_p, -normal.delta_p, 1e-9), "{flipped:?}");

    // A study without a probe of that label is named.
    let mut renamed = s.clone();
    renamed.monitors.probes[1].label = "wake".to_owned();
    assert_eq!(
        measure(&tree, &renamed, &tolerances()).unwrap_err(),
        Refusal::Missing {
            file: "the monitor labelled rear".to_owned()
        }
    );
}
