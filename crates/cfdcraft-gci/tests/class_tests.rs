//! The convergence classes, the warnings and the series form.

use cfdcraft_gci::{
    Convergence, GciError, GciReport, Level, Options, Warning, analyse, analyse_series,
};
use serde::Deserialize;
use serde::de::IntoDeserializer;
use serde::de::value::{Error, StrDeserializer};

/// Three meshes on one area with a refinement ratio of two, finest first.
fn two_ratio(values: [f64; 3]) -> [Level; 3] {
    [64, 16, 4]
        .into_iter()
        .zip(values)
        .map(|(cells, value)| Level {
            cells,
            area: 1.0,
            value,
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

fn run(values: [f64; 3]) -> GciReport {
    analyse(&two_ratio(values), &Options::default()).unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-5 * expected.abs(),
        "{actual} is not {expected}"
    );
}

fn no_numbers(report: &GciReport) {
    assert!(report.order.is_none());
    assert!(report.extrapolated.is_none());
    assert!(report.relative_error_fine.is_none());
    assert!(report.extrapolated_error_fine.is_none());
    assert!(report.gci_fine.is_none());
    assert!(report.gci_coarse.is_none());
    assert!(report.asymptotic_ratio.is_none());
}

#[test]
fn oscillatory_is_named_and_warned() {
    let report = run([1.0, 1.1, 0.9]);
    assert_eq!(report.convergence, Convergence::Oscillatory);
    close(report.order.unwrap(), 1.0);
    close(report.extrapolated.unwrap(), 0.9);
    close(report.gci_fine.unwrap(), 0.125);
    assert!(report.warnings.contains(&Warning::Oscillatory));
}

#[test]
fn no_positive_order_reports_no_gci() {
    let report = run([1.0, 1.1, 1.15]);
    assert_eq!(report.convergence, Convergence::NoPositiveOrder);
    no_numbers(&report);
}

#[test]
fn divergent_oscillation() {
    let report = run([1.0, 1.1, 1.05]);
    assert_eq!(report.convergence, Convergence::Divergent);
    no_numbers(&report);
}

#[test]
fn degenerate() {
    for values in [[1.0, 1.0, 1.2], [1.0, 1.2, 1.2], [1.0, 1.0, 1.0]] {
        let report = run(values);
        assert_eq!(report.convergence, Convergence::Degenerate, "{values:?}");
        no_numbers(&report);
    }
}

#[test]
fn order_and_ratio_windows_warn() {
    let report = run([1.0, 1.1, 1.9]);
    close(report.order.unwrap(), 3.0);
    assert!(report.warnings.contains(&Warning::OrderOutOfRange));
    assert!(!report.warnings.contains(&Warning::AsymptoticOutOfRange));
    let report = run([1.3, 1.0, 0.4]);
    close(report.order.unwrap(), 1.0);
    close(report.asymptotic_ratio.unwrap(), 1.3);
    assert!(report.warnings.contains(&Warning::AsymptoticOutOfRange));
    assert!(!report.warnings.contains(&Warning::OrderOutOfRange));
}

#[test]
fn option_flags_become_warnings() {
    let levels = two_ratio([10.0, 10.3, 11.5]);
    let plain = analyse(&levels, &Options::default()).unwrap();
    assert!(plain.warnings.is_empty());
    let options = Options {
        transient: true,
        wall_function: true,
        noise: Some(0.2),
        ..Options::default()
    };
    let report = analyse(&levels, &options).unwrap();
    assert_eq!(
        report.warnings,
        [
            Warning::Transient,
            Warning::WallFunction,
            Warning::NoiseLimited
        ]
    );
    let quiet = Options {
        noise: Some(0.1),
        ..Options::default()
    };
    assert!(analyse(&levels, &quiet).unwrap().warnings.is_empty());
}

#[test]
fn not_refined_and_uneven_ratios() {
    let same = [4, 4, 1].map(|cells| Level {
        cells,
        area: 1.0,
        value: 1.0,
    });
    assert!(matches!(
        analyse(&same, &Options::default()),
        Err(GciError::NotRefined { .. })
    ));
    let uneven = [3600, 1600, 400]
        .into_iter()
        .zip([1.0, 1.1, 1.4])
        .map(|(cells, value)| Level {
            cells,
            area: 1.0,
            value,
        })
        .collect::<Vec<_>>();
    let report = analyse(&[uneven[0], uneven[1], uneven[2]], &Options::default()).unwrap();
    close(report.ratios[0], 1.5);
    close(report.ratios[1], 2.0);
    assert!(report.warnings.contains(&Warning::NotRefined));
}

#[test]
fn bad_areas_and_values_are_refused() {
    let mut levels = two_ratio([1.0, 1.1, 1.4]);
    levels[1].area = 0.0;
    assert_eq!(
        analyse(&levels, &Options::default()),
        Err(GciError::BadArea)
    );
    let mut levels = two_ratio([1.0, 1.1, 1.4]);
    levels[2].value = f64::NAN;
    assert_eq!(
        analyse(&levels, &Options::default()),
        Err(GciError::NonFinite)
    );
}

#[test]
fn a_series_reports_every_triple() {
    let levels: Vec<Level> = [(1024, 1.0), (256, 1.1), (64, 1.4), (16, 2.6), (4, 7.4)]
        .into_iter()
        .map(|(cells, value)| Level {
            cells,
            area: 1.0,
            value,
        })
        .collect();
    let reports = analyse_series(&levels, &Options::default()).unwrap();
    assert_eq!(reports.len(), 3);
    assert_eq!(reports[2].values, [1.0, 1.1, 1.4]);
    assert_eq!(reports[0].values, [1.4, 2.6, 7.4]);
    assert_eq!(
        analyse_series(&levels[..2], &Options::default()),
        Err(GciError::TooFewLevels { found: 2 })
    );
}

/// The crate cannot depend on a text format, so the words are checked through serde itself.
#[test]
fn the_class_and_warning_words_are_snake_case() {
    fn word<T: for<'a> Deserialize<'a>>(text: &str) -> T {
        let deserializer: StrDeserializer<'_, Error> = text.into_deserializer();
        T::deserialize(deserializer).unwrap()
    }
    assert_eq!(
        word::<Convergence>("no_positive_order"),
        Convergence::NoPositiveOrder
    );
    assert_eq!(word::<Convergence>("monotone"), Convergence::Monotone);
    assert_eq!(
        word::<Warning>("asymptotic_out_of_range"),
        Warning::AsymptoticOutOfRange
    );
    assert_eq!(word::<Warning>("noise_limited"), Warning::NoiseLimited);
}
