use std::collections::BTreeMap;

use cfdcraft_compare::{KeyNumbers, Tolerance, Verdict};
use cfdcraft_parity::{Level, ParityInput, ParityIssue, RecordedColumn, build, render_table};

const NU_HOT: &str = "bench.dvd.nu_hot";
const U_MAX: &str = "bench.dvd.u_max";

fn nums(pairs: &[(&str, f64)]) -> KeyNumbers {
    pairs.iter().map(|(n, v)| ((*n).to_owned(), *v)).collect()
}

fn column(backend: &str, pairs: &[(&str, f64)]) -> RecordedColumn {
    RecordedColumn {
        backend: backend.to_owned(),
        image: format!("sha256:{backend}"),
        run: "run-1".to_owned(),
        preview: false,
        gate_passed: true,
        converged: Some(true),
        numbers: nums(pairs),
    }
}

fn input(baseline: RecordedColumn, columns: Vec<RecordedColumn>) -> ParityInput {
    let tolerance = |abs: f64| Tolerance { abs, rel: 0.0 };
    ParityInput {
        benchmark: "dvd".to_owned(),
        case: "ra1e5 medium".to_owned(),
        golden: columns
            .iter()
            .map(|c| (c.backend.clone(), Level::Pass))
            .collect(),
        baseline,
        columns,
        tolerances: BTreeMap::from([
            (NU_HOT.to_owned(), tolerance(0.5)),
            (U_MAX.to_owned(), tolerance(0.25)),
        ]),
    }
}

#[test]
fn identical_numbers_are_reproduced() {
    let numbers = [(NU_HOT, 4.0), (U_MAX, 0.5)];
    let (report, issues) = build(&input(
        column("esi-v2512", &numbers),
        vec![column("esi-v2606", &numbers)],
    ));
    assert!(issues.is_empty());
    let rows = &report.columns[0].rows;
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.abs_diff, 0.0);
        assert_eq!(row.verdict, Verdict::Reproduced);
        assert_eq!(row.value, row.baseline);
    }
}

#[test]
fn a_difference_inside_the_tolerance_is_within_tolerance() {
    // The tolerance of the first quantity is 0.5; 0.25 is half of it and 1.0 is twice it.
    let (report, issues) = build(&input(
        column("esi-v2512", &[(NU_HOT, 4.0)]),
        vec![
            column("near", &[(NU_HOT, 4.25)]),
            column("far", &[(NU_HOT, 5.0)]),
        ],
    ));
    assert!(issues.is_empty());
    let near = &report.columns[0].rows[0];
    assert_eq!(near.verdict, Verdict::WithinTolerance);
    assert_eq!(near.abs_diff, 0.25);
    assert_eq!(near.tolerance, Tolerance { abs: 0.5, rel: 0.0 });
    let far = &report.columns[1].rows[0];
    assert_eq!(far.verdict, Verdict::Differs);
    assert_eq!(far.abs_diff, 1.0);
    assert_eq!(far.tolerance, Tolerance { abs: 0.5, rel: 0.0 });
}

#[test]
fn a_missing_quantity_is_an_issue_not_a_row_of_zeros() {
    let (report, issues) = build(&input(
        column("esi-v2512", &[(NU_HOT, 4.0), (U_MAX, 0.5)]),
        vec![column("su2-8.6.0", &[(NU_HOT, 4.0)])],
    ));
    assert_eq!(
        issues,
        vec![ParityIssue::QuantityMissing {
            backend: "su2-8.6.0".to_owned(),
            quantity: U_MAX.to_owned(),
        }]
    );
    let rows = &report.columns[0].rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].quantity, NU_HOT);
}

#[test]
fn the_table_is_a_fixed_format() {
    let (report, issues) = build(&input(
        column("esi-v2512", &[(NU_HOT, 4.0), (U_MAX, 0.5)]),
        vec![
            column("esi-v2606", &[(NU_HOT, 4.25), (U_MAX, 0.5)]),
            column("su2-8.6.0", &[(NU_HOT, 5.0), (U_MAX, 0.5)]),
        ],
    ));
    assert!(issues.is_empty());
    let text = render_table(&report);
    let header = text.lines().find(|l| l.starts_with("baseline:"));
    assert!(header.is_some_and(|h| h.contains("esi-v2512")));
    for quantity in [NU_HOT, U_MAX] {
        let lines = text.lines().filter(|l| l.starts_with(quantity)).count();
        assert_eq!(lines, 1, "{quantity} must be one line of the table");
    }
    let golden = include_str!("golden/parity-table.txt");
    assert_eq!(text, golden, "the table text differs from the golden file");
}
