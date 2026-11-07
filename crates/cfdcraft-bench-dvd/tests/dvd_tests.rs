mod support;

use cfdcraft_bench_dvd::fixture::{Tweak, good_files};
use cfdcraft_bench_dvd::{DvdBenchmark, Row, Setup, Table};
use cfdcraft_gate::{Benchmark, GateReport};
use cfdcraft_model::ResolvedStudy;
use support::{Case, Tree, study};

const RAYLEIGH_NUMBERS: [f64; 4] = [1.0e3, 1.0e4, 1.0e5, 1.0e6];

fn bench() -> DvdBenchmark {
    DvdBenchmark::new().unwrap()
}

fn row(ra: f64) -> Row {
    Table::load().unwrap().row_for_ra(ra).unwrap().clone()
}

fn case(ra: f64) -> Case {
    Case {
        ra,
        ..Case::default()
    }
}

fn tree(r: &ResolvedStudy, tweak: &Tweak) -> Tree {
    Tree::from_pairs(good_files(r, tweak).unwrap())
}

fn failed(report: &GateReport) -> Vec<&str> {
    report
        .metrics
        .iter()
        .filter(|m| !m.passed)
        .map(|m| m.name.as_str())
        .collect()
}

fn near(a: f64, b: f64, relative: f64) -> bool {
    (a - b).abs() <= relative * b.abs()
}

/// Equal to 1e-6 of the value, or of one for a value below one, so a position is held to an
/// absolute distance.
fn agrees(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn the_template_shaped_study_matches_at_each_tabulated_rayleigh_number() {
    let bench = bench();
    for ra in RAYLEIGH_NUMBERS {
        let r = study(&case(ra));
        assert_eq!(bench.matches(&r), Ok(()), "Ra {ra}");
        let setup = Setup::from_study(&r).unwrap();
        assert!(near(setup.rayleigh, ra, 1e-9), "{} {ra}", setup.rayleigh);
        assert_eq!(row(setup.rayleigh).ra, ra);
    }
    assert_eq!(bench.id(), "dvd");
    assert_eq!(bench.reference(), "de Vahl Davis (1983), Table V");
}

#[test]
fn each_difference_is_named() {
    let bench = bench();
    let named = |c: Case, what: &str| {
        let r = study(&c);
        let found = bench.matches(&r).unwrap_err();
        assert!(
            found.iter().any(|m| m.what.contains(what)),
            "{what} is not named in {found:?}"
        );
    };
    named(
        Case {
            length_y: 2.0,
            ..Case::default()
        },
        "cavity",
    );
    named(case(5.0e4), "Rayleigh number");
    named(
        Case {
            prandtl: 0.7,
            ..Case::default()
        },
        "Prandtl number",
    );
    named(
        Case {
            swapped: true,
            ..Case::default()
        },
        "hot wall",
    );
    named(
        Case {
            heated: false,
            ..Case::default()
        },
        "buoyancy",
    );
    named(
        Case {
            hot_flux: false,
            ..Case::default()
        },
        "surface hot_flux",
    );
    named(
        Case {
            cold_flux: false,
            ..Case::default()
        },
        "surface cold_flux",
    );
    named(
        Case {
            mid_vertical: false,
            ..Case::default()
        },
        "line mid_vertical",
    );
    named(
        Case {
            mid_horizontal: false,
            ..Case::default()
        },
        "line mid_horizontal",
    );
    // Steady or transient does not matter.
    let transient = study(&Case {
        transient: true,
        ..Case::default()
    });
    assert_eq!(bench.matches(&transient), Ok(()));
}

#[test]
fn a_good_synthetic_run_passes_at_each_rayleigh_number() {
    let bench = bench();
    for ra in RAYLEIGH_NUMBERS {
        let r = study(&case(ra));
        let report = bench.compare(&tree(&r, &Tweak::default()), &r);
        assert_eq!(report.name, "dvd");
        assert!(report.passed, "Ra {ra}: {}", report.render_text());
        assert!(report.reason.is_none());
        let names: Vec<&str> = report.metrics.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Nu", "u_max", "u_max z", "w_max", "w_max x"]);
        let published = row(ra);
        let wanted = [
            published.nu_avg,
            published.u_max,
            published.u_max_z,
            published.w_max,
            published.w_max_x,
        ];
        for (metric, value) in report.metrics.iter().zip(wanted) {
            assert_eq!(
                metric.reference_range,
                Some([value, value]),
                "{}",
                metric.name
            );
            assert!(agrees(metric.rows[0].computed, value), "{}", metric.name);
            assert!(metric.tolerance > 0.0);
        }
        // The tolerance is in the table's units: a fraction of the value, or absolute.
        let nu_allowance = if ra >= 1.0e6 { 0.03 } else { 0.02 };
        assert!(near(
            report.metrics[0].tolerance,
            nu_allowance * published.nu_avg,
            1e-12
        ));
        assert!(near(report.metrics[2].tolerance, 0.03, 1e-12));
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("better than 1 per cent at the highest Rayleigh number")),
            "{:?}",
            report.notes
        );
        assert!(report.notes.iter().any(|n| n.contains("energy imbalance")));
        assert!(!report.notes.iter().any(|n| n.contains("override")));
    }
}

#[test]
fn a_wrong_run_fails_the_right_metric() {
    let bench = bench();
    let r = study(&Case::default());
    let run = |tweak: Tweak| bench.compare(&tree(&r, &tweak), &r);
    let high_nu = run(Tweak {
        nu: 1.05,
        ..Tweak::default()
    });
    assert!(!high_nu.passed);
    assert_eq!(failed(&high_nu), ["Nu"]);
    let low_w = run(Tweak {
        w_max: 0.9,
        ..Tweak::default()
    });
    assert!(!low_w.passed);
    assert_eq!(failed(&low_w), ["w_max"]);
    let off_u = run(Tweak {
        u_max_shift: 0.1,
        ..Tweak::default()
    });
    assert!(!off_u.passed);
    assert_eq!(failed(&off_u), ["u_max z"]);
    let off_w = run(Tweak {
        w_max_shift: 0.1,
        ..Tweak::default()
    });
    assert_eq!(failed(&off_w), ["w_max x"]);

    // 2.5 % high is inside the looser allowance at 1e6 and outside the shipped one at 1e5.
    let slightly = Tweak {
        nu: 1.025,
        ..Tweak::default()
    };
    let top = study(&case(1.0e6));
    assert!(bench.compare(&tree(&top, &slightly), &top).passed);
    let below = study(&case(1.0e5));
    let report = bench.compare(&tree(&below, &slightly), &below);
    assert_eq!(failed(&report), ["Nu"]);
}

#[test]
fn the_quantities_are_the_measured_numbers() {
    let bench = bench();
    let r = study(&Case::default());
    let good = tree(&r, &Tweak::default());
    let quantities = bench.quantities(&good, &r);
    let names: Vec<&str> = quantities.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "bench.dvd.nu_hot",
            "bench.dvd.u_max",
            "bench.dvd.w_max",
            "bench.dvd.u_max_y",
            "bench.dvd.w_max_x",
            "bench.dvd.energy_imbalance",
        ]
    );
    let published = row(1.0e4);
    assert!(agrees(quantities[0].1, published.nu_avg));
    assert!(agrees(quantities[1].1, published.u_max));
    assert!(agrees(quantities[2].1, published.w_max));
    assert!(agrees(quantities[3].1, published.u_max_z));
    assert!(agrees(quantities[4].1, published.w_max_x));
    assert!(quantities[5].1.abs() < 1e-9);

    // A run with no results records nothing and fails with the reason.
    let empty = Tree::default();
    assert!(bench.quantities(&empty, &r).is_empty());
    let report = bench.compare(&empty, &r);
    assert!(!report.passed);
    assert!(
        report
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CFD-BENCH-002")),
        "{:?}",
        report.reason
    );

    // A study that is not this cavity fails with its own code.
    let other = study(&case(5.0e4));
    let report = bench.compare(&good, &other);
    assert!(!report.passed);
    assert!(
        report
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CFD-BENCH-001")),
        "{:?}",
        report.reason
    );
}

#[test]
fn an_override_is_named_and_bounded() {
    let r = study(&Case::default());
    let slightly = tree(
        &r,
        &Tweak {
            nu: 1.04,
            ..Tweak::default()
        },
    );
    let shipped = bench().compare(&slightly, &r);
    assert_eq!(failed(&shipped), ["Nu"]);
    assert!(!shipped.notes.iter().any(|n| n.contains("override")));

    let text = "[tolerance]\nnu_rel = 0.05\njustification = \"coarse mesh accepted\"\n";
    let relaxed = bench().with_override(text).unwrap();
    let report = relaxed.compare(&slightly, &r);
    assert!(report.passed, "{}", report.render_text());
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("tolerance override used") && n.contains("coarse mesh accepted")),
        "{:?}",
        report.notes
    );

    // Past three times the shipped allowance, or with no reason, the file is refused.
    let wide = "[tolerance]\nnu_rel = 0.5\njustification = \"x\"\n";
    assert!(bench().with_override(wide).is_err());
    let silent = "[tolerance]\nnu_rel = 0.05\n";
    assert!(bench().with_override(silent).is_err());
}
