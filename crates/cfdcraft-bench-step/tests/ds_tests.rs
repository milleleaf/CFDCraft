mod support;

use cfdcraft_bench_step::{DsReference, StepBenchmark, Tolerances};
use cfdcraft_gate::Benchmark;
use cfdcraft_gate::Mismatch;
use support::{Ds, MapFiles, floor_text, floor_tree, full_tree, line_path};

const H: f64 = 0.0127;
const UREF: f64 = 44.2;
const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const REFERENCE: &str = include_str!("../data/ds_reference.toml");

fn bench() -> StepBenchmark {
    StepBenchmark::new().unwrap()
}

#[test]
fn a_four_column_and_a_six_column_line_file_are_read() {
    let r = Ds::default().resolved();
    let four = floor_tree(&r, H, 6.25);
    let q4 = bench().quantities(&four, &r);
    assert_eq!(q4.len(), 1);
    assert_eq!(q4[0].0, "bench.step.reattachment");
    assert!((q4[0].1 - 6.25).abs() < 1e-6);

    // The same line as x y z Ux Uy Uz rows.
    let line = r
        .monitors
        .lines
        .iter()
        .find(|l| l.label == "floor")
        .unwrap();
    let six: String = floor_text(&r, "floor", H, 6.25)
        .lines()
        .map(|row| {
            let mut words = row.split_whitespace();
            let d: f64 = words.next().unwrap().parse().unwrap();
            let ux = words.next().unwrap();
            format!("{:.12} {:.12} 0.0 {ux} 0.0 0.0\n", line.x0 + d, line.y0)
        })
        .collect();
    let mut files = MapFiles::default();
    files.0.insert(line_path(&r, "floor", "2000"), six);
    let q6 = bench().quantities(&files, &r);
    assert_eq!(q6.len(), 1);
    assert!((q6[0].1 - 6.25).abs() < 1e-6);
}

#[test]
fn the_turbulent_step_passes_inside_the_tolerance() {
    let r = Ds::default().resolved();
    for (crossing, passes) in [(6.30, true), (6.9, false), (5.7, false)] {
        let report = bench().compare(&full_tree(&r, H, UREF, crossing, 0.0), &r);
        assert_eq!(report.name, "step");
        assert!(report.reason.is_none(), "{:?}", report.reason);
        let m = report
            .metrics
            .iter()
            .find(|m| m.name == "x_R/H")
            .expect("the reattachment metric");
        assert!((m.rows[0].computed - crossing).abs() < 1e-6);
        let range = m.reference_range.unwrap();
        assert!((range[0] - 6.16).abs() < 1e-9 && (range[1] - 6.36).abs() < 1e-9);
        assert!((m.tolerance - 0.5).abs() < 1e-12);
        assert_eq!(m.passed, passes, "crossing {crossing}");
        assert_eq!(report.passed, passes, "crossing {crossing}");
    }
}

#[test]
fn the_latest_write_time_is_used() {
    let r = Ds::default().resolved();
    let mut files = MapFiles::default();
    // 99 is the largest name as text and 100 the largest as a number; `backup` is no time.
    for (folder, crossing) in [("99", 9.0), ("100", 6.30), ("backup", 1.0)] {
        files.0.insert(
            line_path(&r, "floor", folder),
            floor_text(&r, "floor", H, crossing),
        );
    }
    let q = bench().quantities(&files, &r);
    assert_eq!(q.len(), 1);
    assert!((q[0].1 - 6.30).abs() < 1e-6);
}

#[test]
fn missing_or_unreadable_files_fail_with_a_reason() {
    let r = Ds::default().resolved();
    let bench = bench();

    let none = bench.compare(&MapFiles::default(), &r);
    assert!(!none.passed && none.metrics.is_empty());
    assert!(
        none.reason
            .as_deref()
            .is_some_and(|t| t.contains("CFD-BENCH-002"))
    );

    // The study has no floor line.
    let no_line = Ds {
        lines: vec!["profile_x1"],
        ..Ds::default()
    }
    .resolved();
    let report = bench.compare(&floor_tree(&r, H, 6.26), &no_line);
    assert!(!report.passed && report.metrics.is_empty() && report.reason.is_some());

    // A file with a number that is not finite.
    let mut bad = MapFiles::default();
    bad.0.insert(
        line_path(&r, "floor", "2000"),
        "0.0 -1.0 0.0 0.0\n1.0 nan 0.0 0.0\n".to_owned(),
    );
    let report = bench.compare(&bad, &r);
    assert!(!report.passed && report.metrics.is_empty() && report.reason.is_some());
    assert!(bench.quantities(&bad, &r).is_empty());

    // The velocity never turns positive.
    let mut backward = MapFiles::default();
    backward.0.insert(
        line_path(&r, "floor", "2000"),
        "0.0 -1.0 0.0 0.0\n1.0 -0.5 0.0 0.0\n".to_owned(),
    );
    let report = bench.compare(&backward, &r);
    assert!(!report.passed && report.reason.is_some());
}

#[test]
fn the_reference_loads_and_is_pinned() {
    let r = DsReference::load().unwrap();
    assert_eq!(r.reattachment.x_over_h, 6.26);
    assert_eq!(r.reattachment.uncertainty, 0.10);
    assert_eq!(r.source.doi, "10.2514/3.8890");
    assert_eq!(
        r.source.page_url,
        "https://tmbwg.github.io/turbmodels/backstep_val.html"
    );
    assert!(r.source.terms.starts_with("no terms stated"));
    assert!(!r.tolerance.justification.is_empty());
    let t = Tolerances::default_embedded().unwrap();
    assert_eq!((t.x_over_h, t.profile_u_max_abs), (0.5, 0.10));

    // A missing justification, or a tolerance that is not above zero, is refused.
    assert!(DsReference::parse(&REFERENCE.replace("justification =", "note =")).is_err());
    assert!(
        DsReference::parse(&REFERENCE.replace("x_over_h = 0.5\n", "x_over_h = 0.0\n")).is_err()
    );
    assert!(
        DsReference::parse(&REFERENCE.replace("x_over_h = 0.5\n", "x_over_h = -0.5\n")).is_err()
    );
    assert!(
        DsReference::parse(&REFERENCE.replace("doi = \"10.2514/3.8890\"", "doi = \"\"")).is_err()
    );
}

#[test]
fn a_tolerance_override_is_bounded_and_needs_a_reason() {
    let default = Tolerances::default_embedded().unwrap();
    let file = |value: &str, why: &str| format!("[tolerance]\nx_over_h = {value}\n{why}\n");
    let why = "justification = \"coarse mesh study\"";
    // Up to three times the shipped value is accepted, more is not.
    let ok = Tolerances::from_toml(&file("1.5", why), default).unwrap();
    assert_eq!(
        (ok.x_over_h, ok.profile_u_max_abs),
        (1.5, default.profile_u_max_abs)
    );
    assert!(Tolerances::from_toml(&file("1.6", why), default).is_err());
    assert!(Tolerances::from_toml(&file("0.0", why), default).is_err());
    assert!(Tolerances::from_toml(&file("0.8", ""), default).is_err());
    assert!(Tolerances::from_toml(&file("0.8", "justification = \"  \""), default).is_err());
    assert!(
        Tolerances::from_toml(
            "[tolerance]\nunknown = 1.0\njustification = \"x\"\n",
            default
        )
        .is_err()
    );

    // The override and its reason are named in the report.
    let r = Ds::default().resolved();
    let report = StepBenchmark::new()
        .unwrap()
        .with_tolerances(ok, "coarse mesh study")
        .compare(&full_tree(&r, H, UREF, 7.2, 0.0), &r);
    assert!(report.passed);
    assert!(report.notes.iter().any(|n| n.contains("coarse mesh study")));
}

fn has(list: &[Mismatch], what: &str) -> bool {
    list.iter().any(|m| m.what == what)
}

fn refused(study: &cfdcraft_model::ResolvedStudy) -> Vec<Mismatch> {
    bench().matches(study).unwrap_err()
}

#[test]
fn profiles_are_compared_by_interpolation() {
    let r = Ds::default().resolved();
    let b = bench();

    let exact = b.compare(&full_tree(&r, H, UREF, 6.26, 0.0), &r);
    assert!(exact.passed, "{:?}", exact.reason);
    for m in exact
        .metrics
        .iter()
        .filter(|m| m.name.starts_with("u/Uref"))
    {
        assert!(m.max_abs_dev < 1e-6, "{} {}", m.name, m.max_abs_dev);
        assert!(m.passed);
    }

    // Every velocity 0.2 reference velocities too high: the profiles fail, the reattachment does not.
    let shifted = b.compare(&full_tree(&r, H, UREF, 6.26, 0.2), &r);
    assert!(!shifted.passed);
    for m in &shifted.metrics {
        if m.name.starts_with("u/Uref") {
            assert!(
                (m.max_abs_dev - 0.2).abs() < 1e-6 && !m.passed,
                "{}",
                m.name
            );
        } else {
            assert!(m.passed);
        }
    }

    // A profile line with a single sample cannot be compared.
    let mut files = full_tree(&r, H, UREF, 6.26, 0.0);
    files.0.insert(
        line_path(&r, "profile_x4", "2000"),
        "0.05 1.0 0.0 0.0\n".to_owned(),
    );
    let report = b.compare(&files, &r);
    assert!(!report.passed && report.metrics.is_empty());
    assert!(
        report
            .reason
            .as_deref()
            .is_some_and(|t| t.contains("profile_x4") || t.contains("line_"))
    );
}

#[test]
fn the_report_has_a_scalar_and_four_profile_metrics() {
    let r = Ds::default().resolved();
    let b = bench();
    let report = b.compare(&full_tree(&r, H, UREF, 6.26, 0.0), &r);
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
    for m in &report.metrics[1..] {
        assert_eq!(m.rows.len(), 28);
        assert!((m.tolerance - 0.10).abs() < 1e-12);
        assert_eq!(m.unit.as_deref(), Some("Uref"));
        // station is y/H, then the computed and published values and the deviation.
        let row = &m.rows[0];
        assert!((row.station - 0.1).abs() < 1e-12);
        assert!((row.abs_dev - (row.computed - row.reference).abs()).abs() < 1e-12);
    }

    // The verdict is the AND of the five.
    assert!(report.passed);
    assert!(!b.compare(&full_tree(&r, H, UREF, 6.26, 0.2), &r).passed);
    assert!(!b.compare(&full_tree(&r, H, UREF, 7.5, 0.0), &r).passed);
}

#[test]
fn matches_names_what_differs() {
    assert!(bench().matches(&Ds::default().resolved()).is_ok());

    let big = refused(
        &Ds {
            h: 0.025,
            ..Ds::default()
        }
        .resolved(),
    );
    assert!(has(&big, "step height"));
    let slow = refused(
        &Ds {
            speed: 22.1,
            ..Ds::default()
        }
        .resolved(),
    );
    assert!(has(&slow, "Reynolds number on the step height"));
    let wide = refused(
        &Ds {
            height_h: 3.0,
            ..Ds::default()
        }
        .resolved(),
    );
    assert!(has(&wide, "expansion ratio"));
    let parabolic = refused(
        &Ds {
            profile: Some("parabolic"),
            ..Ds::default()
        }
        .resolved(),
    );
    assert!(has(&parabolic, "inlet profile") && parabolic.len() == 1);
    let found = &parabolic[0];
    assert_eq!(
        (found.found.as_str(), found.expected.as_str()),
        ("parabolic", "uniform")
    );

    let no_floor = refused(
        &Ds {
            lines: vec!["profile_x1", "profile_x4", "profile_x6", "profile_x10"],
            ..Ds::default()
        }
        .resolved(),
    );
    assert!(has(&no_floor, "line floor") && no_floor.len() == 1);
    let no_profile = refused(
        &Ds {
            lines: vec!["floor", "profile_x1", "profile_x6", "profile_x10"],
            ..Ds::default()
        }
        .resolved(),
    );
    assert!(has(&no_profile, "line profile_x4") && no_profile.len() == 1);

    // A floor line far above the floor, and a profile line off its station, are named too.
    let high = refused(
        &Ds {
            floor_height_h: 1.0,
            ..Ds::default()
        }
        .resolved(),
    );
    assert!(has(&high, "floor line height") && high.len() == 1);
    let off = refused(
        &Ds {
            station_offset_h: 0.5,
            ..Ds::default()
        }
        .resolved(),
    );
    assert!(has(&off, "line profile_x1 position") && off.len() == 4);

    // Another shape.
    let cavity =
        cfdcraft_model::resolve(&cfdcraft_model::Study::from_json(CAVITY).unwrap()).unwrap();
    assert!(has(&refused(&cavity), "shape"));

    // The turbulence model does not matter.
    assert!(
        bench()
            .matches(
                &Ds {
                    turbulent: false,
                    ..Ds::default()
                }
                .resolved()
            )
            .is_ok()
    );
}

#[test]
fn the_quantity_is_the_reattachment_in_step_heights() {
    let r = Ds::default().resolved();
    let q = bench().quantities(&floor_tree(&r, H, 6.25), &r);
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].0, "bench.step.reattachment");
    assert!((q[0].1 - 6.25).abs() < 1e-6);
    // No sign change in the 50 step heights the line covers.
    assert!(bench().quantities(&floor_tree(&r, H, 100.0), &r).is_empty());
    assert!(bench().quantities(&MapFiles::default(), &r).is_empty());
}
