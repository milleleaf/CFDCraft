//! Published worked examples, reproduced.

use cfdcraft_gci::{Convergence, Level, Options, analyse};

/// Three meshes on one area, finest first.
fn levels(cells: [u64; 3], values: [f64; 3]) -> [Level; 3] {
    [0, 1, 2].map(|i| Level {
        cells: cells[i],
        area: 1.0,
        value: values[i],
    })
}

fn close(actual: f64, expected: f64, rel: f64) {
    assert!(
        ((actual - expected) / expected).abs() <= rel,
        "{actual} is not {expected} within {rel}"
    );
}

#[test]
fn celik_table_one() {
    let report = analyse(
        &levels([18000, 8000, 4500], [6.063, 5.972, 5.863]),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(report.convergence, Convergence::Monotone);
    close(report.ratios[0], 1.5, 1e-9);
    close(report.ratios[1], 1.333333, 1e-6);
    close(report.order.unwrap(), 1.53397, 1e-5);
    close(report.extrapolated.unwrap(), 6.168496, 1e-5);
    close(report.relative_error_fine.unwrap(), 0.0150091, 1e-5);
    close(report.extrapolated_error_fine.unwrap(), 0.0171023, 1e-5);
    close(report.gci_fine.unwrap(), 0.0217499, 1e-5);
}

#[test]
fn nasa_example() {
    let report = analyse(
        &levels([64, 16, 4], [0.97050, 0.96854, 0.96178]),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(report.convergence, Convergence::Monotone);
    close(report.order.unwrap(), 1.78617, 1e-5);
    close(report.extrapolated.unwrap(), 0.971300, 1e-5);
    close(report.relative_error_fine.unwrap(), 0.0020196, 1e-4);
    close(report.gci_fine.unwrap(), 0.00103083, 1e-5);
    close(report.gci_coarse.unwrap(), 0.00356249, 1e-4);
    close(report.asymptotic_ratio.unwrap(), 1.002019, 1e-4);
}

#[test]
fn the_input_order_does_not_matter() {
    let fine_first = levels([64, 16, 4], [0.97050, 0.96854, 0.96178]);
    let coarse_first = [fine_first[2], fine_first[1], fine_first[0]];
    let options = Options::default();
    assert_eq!(
        analyse(&fine_first, &options),
        analyse(&coarse_first, &options)
    );
}

#[test]
fn the_input_is_not_changed() {
    let input = levels([4, 64, 16], [0.96178, 0.97050, 0.96854]);
    let before = input;
    let _ = analyse(&input, &Options::default());
    assert_eq!(input, before);
}

#[test]
fn exactly_quadratic() {
    let report = analyse(&levels([64, 16, 4], [1.1, 1.4, 2.6]), &Options::default()).unwrap();
    assert_eq!(report.convergence, Convergence::Monotone);
    close(report.order.unwrap(), 2.0, 1e-5);
    close(report.extrapolated.unwrap(), 1.0, 1e-5);
    close(report.gci_fine.unwrap(), 0.113636, 1e-5);
    close(report.asymptotic_ratio.unwrap(), 0.785714, 1e-5);
}
