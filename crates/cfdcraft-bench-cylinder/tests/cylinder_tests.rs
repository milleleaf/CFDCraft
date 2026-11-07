mod support;

use cfdcraft_bench_cylinder::fixture::{Run, write_good_results, write_wrong_axis_results};
use cfdcraft_bench_cylinder::{CylinderBenchmark, Tolerances};
use cfdcraft_gate::{Benchmark, GateReport, Mismatch, ResultFiles};
use cfdcraft_model::{InletProfile, ResolvedShape, ResolvedStudy, TurbulenceModel};
use support::{Tree, close, study};

fn benchmark() -> CylinderBenchmark {
    CylinderBenchmark::shipped().unwrap()
}

fn compare(run: &Run) -> GateReport {
    let s = study();
    benchmark().compare(&Tree::of(&s, run), &s)
}

fn mismatches(edit: impl FnOnce(&mut ResolvedStudy)) -> Vec<Mismatch> {
    let mut s = study();
    edit(&mut s);
    benchmark().matches(&s).unwrap_err()
}

/// The edit makes the study differ, and the list names `what`.
fn check(what: &str, edit: impl FnOnce(&mut ResolvedStudy)) {
    let found = mismatches(edit);
    assert!(found.iter().any(|m| m.what == what), "{what}: {found:?}");
}

#[test]
fn the_template_shaped_study_matches() {
    benchmark().matches(&study()).unwrap();
    assert_eq!(benchmark().id(), "cylinder");
}

#[test]
fn each_difference_is_named() {
    check("domain shape", |s| s.domain.shape = None);
    check("cylinder diameter", |s| {
        s.domain.shape = Some(ResolvedShape::CylinderChannel {
            diameter: 0.08,
            centre_x: 0.2,
            centre_y: 0.2,
        });
    });
    check("cylinder centre y", |s| {
        s.domain.shape = Some(ResolvedShape::CylinderChannel {
            diameter: 0.1,
            centre_x: 0.2,
            centre_y: 0.22,
        });
    });
    check("kinematic viscosity", |s| {
        s.fluid.kinematic_viscosity = 2e-3
    });
    check("inlet profile", |s| {
        if let Some(b) = s.boundary_conditions.get_mut("inlet") {
            b.profile = Some(InletProfile::Uniform);
        }
    });
    check("mean inflow speed", |s| {
        if let Some(b) = s.boundary_conditions.get_mut("inlet") {
            b.velocity = Some(2.0);
        }
    });
    check("time", |s| s.physics = None);
    check("force monitor", |s| s.monitors.forces.clear());
    check("force reference length", |s| {
        s.monitors.forces[0].reference_length = 0.2;
    });
    check("pressure probe rear", |s| {
        s.monitors.probes.retain(|p| p.label != "rear");
    });
    check("pressure probe front", |s| s.monitors.probes[0].x = 0.16);

    // A turbulence model is the user's choice; the run is then the model against the benchmark.
    let mut s = study();
    if let Some(p) = s.physics.as_mut() {
        p.turbulence.model = TurbulenceModel::KOmegaSst;
    }
    benchmark().matches(&s).unwrap();
}

#[test]
fn a_good_synthetic_run_passes() {
    let report = compare(&Run::good());
    assert_eq!(report.name, "cylinder");
    assert!(report.passed, "{}", report.render_text());
    assert_eq!(report.metrics.len(), 4);
    assert!(report.reason.is_none() && report.notes.is_empty());

    let high = compare(&Run {
        lift_amplitude: 1.3,
        ..Run::good()
    });
    assert!(!high.passed);
    let failing: Vec<&str> = high
        .metrics
        .iter()
        .filter(|m| !m.passed)
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(failing, ["cLmax"]);
    let lift = &high.metrics[1];
    assert!(close(lift.rows[0].computed, 1.3, 1e-3));
    let text = high.render_text();
    assert!(text.contains("cLmax"), "{text}");
    assert!(text.contains("0.9900 to 1.0100"), "{text}");
    assert!(text.contains("0.0300"), "{text}");
    assert!(text.contains("FAIL"), "{text}");
}

#[test]
fn refusals_become_failed_reports_with_codes() {
    let s = study();
    let wrong_axis = Run {
        lift_dir: Some("(0 0 1)"),
        ..Run::good()
    };
    let short = Run {
        steps: 2500,
        ..Run::good()
    };
    let mut no_force = Tree::of(&s, &Run::good());
    no_force.remove_prefix("postProcessing/force_1/");
    let cases = [
        (Tree::of(&s, &wrong_axis), "CFD-BENCH-003"),
        (Tree::of(&s, &short), "CFD-BENCH-004"),
        (no_force, "CFD-BENCH-002"),
    ];
    for (tree, code) in cases {
        let report = benchmark().compare(&tree, &s);
        assert!(!report.passed);
        assert!(report.metrics.is_empty());
        let reason = report.reason.clone().unwrap_or_default();
        assert!(reason.starts_with(code), "{code}: {reason}");
        assert!(report.render_text().contains(code));
        assert!(benchmark().quantities(&tree, &s).is_empty());
    }
}

#[test]
fn the_quantities_are_the_measured_numbers() {
    let s = study();
    let found = benchmark().quantities(&Tree::of(&s, &Run::good()), &s);
    let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "bench.cylinder.st",
            "bench.cylinder.cd_max",
            "bench.cylinder.cl_max",
            "bench.cylinder.delta_p",
            "bench.cylinder.cycles"
        ]
    );
    assert!(close(found[0].1, 0.3, 2e-3));
    assert!(close(found[1].1, 3.24, 2e-3));
    assert!(close(found[2].1, 1.0, 2e-3));
    assert!(close(found[3].1, 2.48, 2e-3));
    assert!(found[4].1 >= 23.0);
}

#[test]
fn an_override_is_named_and_bounded() {
    let wide = "[tolerance]\ncd_max = 0.05\njustification = \"a coarse first mesh\"\n";
    let overridden = CylinderBenchmark::new(Tolerances::from_toml(wide).unwrap()).unwrap();
    let s = study();
    let tree = Tree::of(&s, &Run::good());
    let report = overridden.compare(&tree, &s);
    assert!(report.passed);
    assert_eq!(report.notes.len(), 1);
    assert!(report.notes[0].starts_with("tolerance override used:"));
    assert!(report.notes[0].contains("a coarse first mesh"));
    assert!(report.render_text().contains("tolerance override used:"));
    // The shipped allowances carry no note.
    assert!(benchmark().compare(&tree, &s).notes.is_empty());
}

#[test]
fn the_fixture_writers_make_the_same_tree_on_disk() {
    let s = study();
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/cylinder-tests")
        .join(std::process::id().to_string());
    let good = root.join("good");
    let wrong = root.join("wrong");
    write_good_results(&good, &s).unwrap();
    write_wrong_axis_results(&wrong, &s).unwrap();
    for name in ["force_1/0/coefficient.dat", "probe_1/0/p", "probe_2/0/p"] {
        assert!(good.join("postProcessing").join(name).is_file(), "{name}");
    }
    let coefficients =
        std::fs::read_to_string(wrong.join("postProcessing/force_1/0/coefficient.dat"));
    assert!(coefficients.unwrap().contains("(0 0 1)"));
    let tree = Tree::of(&s, &Run::good());
    let on_disk = std::fs::read_to_string(good.join("postProcessing/probe_1/0/p")).unwrap();
    assert_eq!(tree.read("postProcessing/probe_1/0/p"), Some(on_disk));
    std::fs::remove_dir_all(root).unwrap();
}
