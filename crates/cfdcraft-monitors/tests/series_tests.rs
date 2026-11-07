//! Joining the series of a continued run.

use cfdcraft_monitors::{Series, join_series};

fn part(name: &str, samples: &[(f64, f64)]) -> Series {
    let mut series = Series::new(name);
    for (time, value) in samples {
        series.push(*time, *value);
    }
    series
}

#[test]
fn join_series_concatenates_in_order_and_the_later_value_wins_at_the_seam() {
    let first = part("m.p", &[(0.0, 1.0), (1.0, 2.0), (2.0, 3.0), (3.0, 4.0)]);
    let second = part("m.p", &[(3.0, 40.0), (4.0, 50.0), (5.0, 60.0)]);
    let joined = join_series(vec![first, second]);
    assert_eq!(joined.name, "m.p");
    assert_eq!(joined.times, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!(joined.values, vec![1.0, 2.0, 3.0, 40.0, 50.0, 60.0]);

    // Parts handed over out of order are sorted by their first time.
    let first = part("m.p", &[(0.0, 1.0), (3.0, 4.0)]);
    let second = part("m.p", &[(3.0, 40.0), (6.0, 7.0)]);
    let joined = join_series(vec![second, first]);
    assert_eq!(joined.times, vec![0.0, 3.0, 6.0]);
    assert_eq!(joined.values, vec![1.0, 40.0, 7.0]);
}

#[test]
fn a_restart_that_overlaps_an_earlier_part_keeps_time_order() {
    let first = part("m.p", &[(0.0, 1.0), (2.0, 2.0), (4.0, 3.0)]);
    let second = part("m.p", &[(3.0, 30.0), (4.0, 40.0)]);
    let joined = join_series(vec![first, second]);
    assert_eq!(joined.times, vec![0.0, 2.0, 3.0, 4.0]);
    assert_eq!(joined.values, vec![1.0, 2.0, 30.0, 40.0]);
}

#[test]
fn an_empty_part_is_skipped() {
    let empty = Series::new("m.p");
    let full = part("m.p", &[(1.0, 2.0), (2.0, 3.0)]);
    let joined = join_series(vec![empty.clone(), full.clone(), empty]);
    assert_eq!(joined.name, "m.p");
    assert_eq!(joined.times, full.times);
    assert_eq!(joined.values, full.values);
    let nothing = join_series(Vec::new());
    assert!(nothing.times.is_empty() && nothing.name.is_empty());
}
