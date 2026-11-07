mod support;

use cfdcraft_bench_step::fixture::{good_files, write_good_results};
use cfdcraft_bench_step::{
    ArmalyBenchmark, ArmalyCurve, ArmalyReference, StepBenchmark, reynolds_number,
};
use cfdcraft_gate::{Benchmark, Mismatch};
use support::{Ar, Ds, MapFiles, floor_text, floor_tree, line_path};

const S: f64 = 0.0049;
const ARMALY: &str = include_str!("../data/armaly.toml");

fn bench() -> ArmalyBenchmark {
    ArmalyBenchmark::new().unwrap()
}

fn reference_at(re: f64) -> f64 {
    ArmalyCurve::load().unwrap().reference_at(re).unwrap()
}

fn has(list: &[Mismatch], what: &str) -> bool {
    list.iter().any(|m| m.what == what)
}

fn refused(ar: &Ar) -> Vec<Mismatch> {
    bench().matches(&ar.resolved()).unwrap_err()
}

#[test]
fn the_study_has_the_reynolds_number_it_was_built_for() {
    for re in [100.0, 200.0, 400.0] {
        let got = reynolds_number(
            &Ar {
                re,
                ..Ar::default()
            }
            .resolved(),
        )
        .unwrap();
        assert!((got - re).abs() < 1e-6 * re, "{re} {got}");
    }
}

#[test]
fn the_geometry_and_method_are_recorded() {
    let r = ArmalyReference::load().unwrap();
    assert_eq!(r.geometry.step_height_mm, 4.9);
    assert_eq!(r.geometry.inlet_height_mm, 5.2);
    assert_eq!(r.geometry.expansion_ratio, 1.94);
    assert_eq!(r.geometry.aspect_ratio, 18.0);
    assert_eq!(r.geometry.reynolds_definition, "U 2h / nu");
    assert_eq!(r.tolerance.reading_uncertainty_s, 0.5);
    assert_eq!(r.tolerance.relative_allowance, 0.10);
    assert_eq!(r.source.doi, "10.1017/S0022112083002839");
    assert_eq!(
        (r.source.figure.as_str(), r.source.page.as_str()),
        ("4", "479")
    );
    assert!(r.source.method.contains("0.5 S"));
    assert!(!r.tolerance.justification.is_empty());
    // The geometry is consistent: the step is the difference of the two channel heights.
    let g = &r.geometry;
    assert!((g.outlet_height_mm - g.inlet_height_mm - g.step_height_mm).abs() < 1e-9);
    assert!((g.outlet_height_mm / g.inlet_height_mm - g.expansion_ratio).abs() < 0.01);

    // A missing field and a tolerance that is not above zero are refused.
    assert!(ArmalyReference::parse(&ARMALY.replace("reading_uncertainty_s", "reading")).is_err());
    assert!(
        ArmalyReference::parse(
            &ARMALY.replace("relative_allowance = 0.10", "relative_allowance = 0.0")
        )
        .is_err()
    );
    assert!(
        ArmalyReference::parse(&ARMALY.replace(
            "reading_uncertainty_s = 0.5",
            "reading_uncertainty_s = -0.5"
        ))
        .is_err()
    );
    assert!(ArmalyReference::parse(&ARMALY.replace("method = ", "how = ")).is_err());
}

#[test]
fn the_reference_at_a_reynolds_number_is_interpolated() {
    let c = ArmalyCurve::load().unwrap();
    let mid = (c.x1_over_s[1] + c.x1_over_s[2]) / 2.0;
    assert!((c.reference_at(175.0).unwrap() - mid).abs() < 1e-12);
    assert_eq!(c.reference_at(100.0), Some(c.x1_over_s[0]));
    assert_eq!(c.reference_at(400.0), Some(*c.x1_over_s.last().unwrap()));
    // Rounding in a computed Reynolds number does not push it out of the table.
    assert_eq!(c.reference_at(100.0 - 1e-9), Some(c.x1_over_s[0]));
    assert_eq!(
        c.reference_at(400.0 + 1e-9),
        Some(*c.x1_over_s.last().unwrap())
    );
    for outside in [99.9, 400.1, f64::NAN, f64::INFINITY] {
        assert_eq!(c.reference_at(outside), None, "{outside}");
    }
}

#[test]
fn the_laminar_step_passes_inside_the_allowance() {
    for re in [100.0, 200.0, 400.0] {
        let r = Ar {
            re,
            ..Ar::default()
        }
        .resolved();
        let published = reference_at(re);
        let allowance = 0.5 + 0.10 * published;

        let report = bench().compare(&floor_tree(&r, S, published * 1.05), &r);
        assert!(report.reason.is_none(), "{:?}", report.reason);
        assert_eq!(report.name, "armaly");
        let m = &report.metrics[0];
        assert_eq!(m.name, "x1/S");
        assert!((m.rows[0].computed - published * 1.05).abs() < 1e-6);
        let range = m.reference_range.unwrap();
        assert!((range[0] - (published - 0.5)).abs() < 1e-9);
        assert!((range[1] - (published + 0.5)).abs() < 1e-9);
        assert!((m.tolerance - allowance).abs() < 1e-9);
        assert!(m.passed && report.passed, "re {re}");

        let far = bench().compare(&floor_tree(&r, S, published * 2.0), &r);
        assert!(!far.passed && !far.metrics[0].passed, "re {re}");
        // The limit is a distance from the published value, not from its uncertainty band.
        let edge = bench().compare(&floor_tree(&r, S, published + allowance + 0.05), &r);
        assert!(!edge.passed, "re {re}");
    }
}

#[test]
fn a_run_with_no_reattachment_or_no_files_fails_with_a_reason() {
    let r = Ar::default().resolved();
    let none = bench().compare(&MapFiles::default(), &r);
    assert!(!none.passed && none.metrics.is_empty() && none.reason.is_some());
    // Still separated at the end of the line.
    let report = bench().compare(&floor_tree(&r, S, 1000.0), &r);
    assert!(!report.passed && report.reason.is_some());
    // The Reynolds number is outside the measured range.
    let outside = Ar {
        re: 600.0,
        ..Ar::default()
    }
    .resolved();
    let report = bench().compare(&floor_tree(&outside, S, 9.0), &outside);
    assert!(!report.passed && report.reason.as_deref().is_some_and(|t| t.contains("600")));
}

#[test]
fn matches_requires_the_armaly_conditions() {
    assert!(bench().matches(&Ar::default().resolved()).is_ok());
    for re in [100.0, 400.0] {
        assert!(
            bench()
                .matches(
                    &Ar {
                        re,
                        ..Ar::default()
                    }
                    .resolved()
                )
                .is_ok(),
            "{re}"
        );
    }

    assert!(has(
        &refused(&Ar {
            re: 50.0,
            ..Ar::default()
        }),
        "Reynolds number"
    ));
    assert!(has(
        &refused(&Ar {
            re: 600.0,
            ..Ar::default()
        }),
        "Reynolds number"
    ));
    assert!(has(
        &refused(&Ar {
            turbulent: true,
            ..Ar::default()
        }),
        "turbulence model"
    ));
    let uniform = refused(&Ar {
        profile: None,
        ..Ar::default()
    });
    assert!(has(&uniform, "inlet profile") && uniform.len() == 1);
    let wide = refused(&Ar {
        height: 0.0147,
        ..Ar::default()
    });
    assert!(has(&wide, "expansion ratio"));
    let no_floor = refused(&Ar {
        lines: vec![],
        ..Ar::default()
    });
    assert!(has(&no_floor, "line floor"));

    // A turbulent step of the other benchmark is not this one, and the other way round.
    let ds = Ds::default().resolved();
    assert!(bench().matches(&ds).is_err());
    assert!(
        StepBenchmark::new()
            .unwrap()
            .matches(&Ar::default().resolved())
            .is_err()
    );
}

#[test]
fn the_quantity_is_in_step_heights() {
    let r = Ar::default().resolved();
    let q = bench().quantities(&floor_tree(&r, S, 4.75), &r);
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].0, "bench.armaly.reattachment");
    assert!((q[0].1 - 4.75).abs() < 1e-6);
    assert!(bench().quantities(&MapFiles::default(), &r).is_empty());
}

#[test]
fn the_fixture_files_pass_the_benchmark_they_were_made_for() {
    let mut files = MapFiles::default();
    let ds = Ds::default().resolved();
    for (rel, text) in good_files(&ds).unwrap() {
        files.0.insert(rel, text);
    }
    let report = StepBenchmark::new().unwrap().compare(&files, &ds);
    assert!(report.passed, "{:?}", report.reason);
    assert_eq!(report.metrics.len(), 5);

    for re in [100.0, 250.0, 400.0] {
        let ar = Ar {
            re,
            ..Ar::default()
        }
        .resolved();
        let mut files = MapFiles::default();
        for (rel, text) in good_files(&ar).unwrap() {
            files.0.insert(rel, text);
        }
        let report = bench().compare(&files, &ar);
        assert!(report.passed, "{re} {:?}", report.reason);
        // The files are at the path a comparer looks for them.
        assert!(files.0.contains_key(&line_path(&ar, "floor", "2000")));
    }

    // A study that is neither step gets nothing.
    let other = Ar {
        re: 600.0,
        ..Ar::default()
    }
    .resolved();
    assert!(good_files(&other).is_none());
    let dir = std::env::temp_dir().join(format!("cfdcraft-bench-step-{}", std::process::id()));
    assert!(write_good_results(&dir, &other).is_err());

    // Written to disk, the same text is found below the results folder.
    write_good_results(&dir, &ds).unwrap();
    let path = dir.join(line_path(&ds, "floor", "2000"));
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert_eq!(on_disk, floor_text(&ds, "floor", 0.0127, 6.26));
    std::fs::remove_dir_all(&dir).unwrap();
}
