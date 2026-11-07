use cfdcraft_settle::{QuantityState, SampleKind, Samples, Settings, Status, assess_series};

fn times(n: usize) -> Vec<f64> {
    (0..n).map(|i| i as f64).collect()
}

fn judge(values: &[f64], s: &Settings) -> QuantityState {
    let t = times(values.len());
    assess_series(
        &Samples {
            name: "probe_1.Ux",
            times: &t,
            values,
        },
        SampleKind::Iteration,
        s,
    )
}

/// The least-squares slope by the textbook sums, written differently from the crate.
fn textbook_drift(t: &[f64], x: &[f64]) -> f64 {
    let n = t.len() as f64;
    let (st, sx) = (t.iter().sum::<f64>(), x.iter().sum::<f64>());
    let stx: f64 = t.iter().zip(x).map(|(a, b)| a * b).sum();
    let stt: f64 = t.iter().map(|a| a * a).sum();
    let slope = (n * stx - st * sx) / (n * stt - st * st);
    slope * (t[t.len() - 1] - t[0])
}

#[test]
fn a_series_that_levels_off_is_settled_and_every_number_is_returned() {
    let values: Vec<f64> = (0..1000)
        .map(|t| 1.0 + 0.5 * (-(t as f64) / 50.0).exp())
        .collect();
    let q = judge(&values, &Settings::default());
    let tail = &values[800..];
    let t = times(1000);
    let mean = tail.iter().sum::<f64>() / 200.0;
    let spread = tail[0] - tail[199];
    let drift = textbook_drift(&t[800..], tail);
    assert_eq!(q.name, "probe_1.Ux");
    assert_eq!(q.status, Status::Settled);
    assert_eq!(q.samples, 200);
    assert_eq!(q.last, values[999]);
    assert!((q.mean - mean).abs() < 1e-12, "mean {} vs {mean}", q.mean);
    assert!(q.spread > 0.0 && (q.spread - spread).abs() < 1e-12);
    assert!(q.drift < 0.0 && (q.drift - drift).abs() < 1e-12);
}

#[test]
fn a_series_still_falling_is_drifting() {
    let values: Vec<f64> = (0..1000).map(|t| (-(t as f64) / 2000.0).exp()).collect();
    let q = judge(&values, &Settings::default());
    assert_eq!(q.status, Status::Drifting);
    assert!(q.drift < 0.0);
}

#[test]
fn a_noisy_flat_series_is_oscillating() {
    // An odd tail (51 of 251) keeps the alternation symmetric about its centre, so the fitted
    // line is flat and only the spread is left.
    let values: Vec<f64> = (0..251)
        .map(|i| if i % 2 == 0 { 1.0 } else { 1.2 })
        .collect();
    let q = judge(&values, &Settings::default());
    assert_eq!(q.samples, 51);
    assert!(q.drift.abs() < 1e-12, "drift {}", q.drift);
    assert_eq!(q.status, Status::Oscillating);
}

#[test]
fn the_tail_is_a_fifth_of_the_samples_but_never_under_fifty() {
    for (n, tail) in [(100usize, 50u64), (1000, 200), (250, 50), (251, 51)] {
        let values = vec![1.0; n];
        let q = judge(&values, &Settings::default());
        assert_eq!(q.samples, tail, "{n} samples");
        assert_eq!(q.status, Status::Settled, "{n} samples");
    }
}

#[test]
fn fewer_than_fifty_iterations_is_too_early() {
    let values = [1.0; 49];
    let q = judge(&values, &Settings::default());
    assert_eq!(q.status, Status::TooEarly);
    assert_eq!(q.samples, 49);
    assert_eq!(q.last, 1.0);
    assert_eq!(
        judge(&[1.0; 50], &Settings::default()).status,
        Status::Settled
    );
}

#[test]
fn the_default_settings_are_the_locked_numbers() {
    let s = Settings::default();
    assert_eq!(s.tail_fraction, 0.20);
    assert_eq!(s.tail_floor, 50);
    assert_eq!(s.spread_rel, 0.01);
    assert_eq!(s.drift_rel, 0.001);
    assert_eq!(s.line_min_samples, 5);
    assert_eq!(s.floor_fraction, 1e-3);
    assert_eq!(s.mass_tol, 1e-3);
    assert_eq!(s.energy_tol, 1e-2);
    assert!(s.check().is_ok());
}

#[test]
fn settings_outside_their_range_are_refused() {
    let cases: Vec<(&str, Settings)> = vec![
        (
            "tail fraction 0",
            Settings {
                tail_fraction: 0.0,
                ..Settings::default()
            },
        ),
        (
            "tail fraction 1.5",
            Settings {
                tail_fraction: 1.5,
                ..Settings::default()
            },
        ),
        (
            "negative spread",
            Settings {
                spread_rel: -0.01,
                ..Settings::default()
            },
        ),
        (
            "negative drift",
            Settings {
                drift_rel: -1.0,
                ..Settings::default()
            },
        ),
        (
            "floor of 4",
            Settings {
                tail_floor: 4,
                ..Settings::default()
            },
        ),
        (
            "line floor of 2",
            Settings {
                line_min_samples: 2,
                ..Settings::default()
            },
        ),
        (
            "mass tolerance 0",
            Settings {
                mass_tol: 0.0,
                ..Settings::default()
            },
        ),
        (
            "energy tolerance 2",
            Settings {
                energy_tol: 2.0,
                ..Settings::default()
            },
        ),
        (
            "scale floor above 1",
            Settings {
                floor_fraction: 1.5,
                ..Settings::default()
            },
        ),
    ];
    for (name, s) in cases {
        assert!(s.check().is_err(), "{name} must be refused");
    }
}

#[test]
fn a_state_serialises_with_snake_case_words() {
    for (status, word) in [
        (Status::Settled, "settled"),
        (Status::Drifting, "drifting"),
        (Status::Oscillating, "oscillating"),
        (Status::TooEarly, "too_early"),
        (Status::TooFewSamples, "too_few_samples"),
        (Status::Error, "error"),
    ] {
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            format!("\"{word}\"")
        );
    }
    let q = judge(&[1.0; 100], &Settings::default());
    let text = serde_json::to_string(&q).unwrap();
    assert!(text.contains("\"status\":\"settled\""), "{text}");
    let back: QuantityState = serde_json::from_str(&text).unwrap();
    assert_eq!(back, q);
}
