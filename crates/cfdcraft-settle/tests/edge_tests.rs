use cfdcraft_settle::{QuantityState, SampleKind, Samples, Settings, Status, assess_series};

fn index_times(n: usize) -> Vec<f64> {
    (0..n).map(|i| i as f64).collect()
}

fn run(values: &[f64], kind: SampleKind, s: &Settings) -> QuantityState {
    let t = index_times(values.len());
    assess_series(
        &Samples {
            name: "q",
            times: &t,
            values,
        },
        kind,
        s,
    )
}

fn iterations(values: &[f64]) -> QuantityState {
    run(values, SampleKind::Iteration, &Settings::default())
}

/// An odd tail of alternating values is symmetric about its centre, so the fitted line is flat.
fn alternating(n: usize, centre: f64, spread: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            centre
                + if i % 2 == 0 {
                    -spread / 2.0
                } else {
                    spread / 2.0
                }
        })
        .collect()
}

#[test]
fn a_constant_series_is_settled_without_dividing_by_zero() {
    for value in [0.0, 3.5, -2.0] {
        let q = iterations(&[value; 300]);
        assert_eq!(q.status, Status::Settled, "constant {value}");
        assert_eq!(q.spread, 0.0, "constant {value}");
        assert_eq!(q.drift, 0.0, "constant {value}");
        assert_eq!(q.mean, value, "constant {value}");
    }
}

#[test]
fn a_mean_near_zero_uses_the_floor_on_the_scale() {
    let with_start = |amp: f64| {
        let mut v = alternating(301, 1e-9, amp);
        v[0] = 1.0;
        v
    };
    let floored = Settings::default();
    let unfloored = Settings {
        floor_fraction: 0.0,
        ..Settings::default()
    };
    let cases = [
        (
            "spread 1e-12 against the floor",
            1e-12,
            &floored,
            Status::Settled,
        ),
        (
            "spread 1e-4 against the floor",
            1e-4,
            &floored,
            Status::Oscillating,
        ),
        (
            "spread 1e-8 against the floor",
            1e-8,
            &floored,
            Status::Settled,
        ),
        (
            "spread 1e-8 without the floor",
            1e-8,
            &unfloored,
            Status::Oscillating,
        ),
    ];
    for (name, amp, settings, want) in cases {
        let q = run(&with_start(amp), SampleKind::Iteration, settings);
        assert_eq!(q.samples, 61, "{name}");
        assert_eq!(
            q.status, want,
            "{name}: spread {} mean {}",
            q.spread, q.mean
        );
    }
}

#[test]
fn a_non_finite_sample_is_an_error() {
    for (name, bad) in [
        ("infinity", f64::INFINITY),
        ("negative infinity", f64::NEG_INFINITY),
        ("not a number", f64::NAN),
    ] {
        let mut v = vec![1.0; 100];
        v[40] = bad;
        let q = iterations(&v);
        assert_eq!(q.status, Status::Error, "{name}");
        assert_eq!(q.samples, 100, "{name}");
        for number in [q.last, q.mean, q.spread, q.drift] {
            assert_eq!(number, 0.0, "{name}: an error carries no number");
        }
    }
    // Values large enough that their sum overflows are an error too, not a stored infinity.
    let q = iterations(&[f64::MAX; 100]);
    assert_eq!(q.status, Status::Error);
    assert_eq!(q.mean, 0.0);
}

#[test]
fn unequal_lengths_or_unordered_times_are_an_error() {
    let values = [1.0; 100];
    let ordered = index_times(100);
    let mut repeated = ordered.clone();
    repeated[60] = repeated[59];
    let mut backwards = ordered.clone();
    backwards.swap(10, 11);
    let mut broken = ordered;
    broken[5] = f64::NAN;
    let short = index_times(99);
    let cases = [
        ("one time short", short.as_slice()),
        ("a repeated time", repeated.as_slice()),
        ("times going backwards", backwards.as_slice()),
        ("a time that is not a number", broken.as_slice()),
    ];
    for (name, times) in cases {
        let q = assess_series(
            &Samples {
                name: "q",
                times,
                values: &values,
            },
            SampleKind::Iteration,
            &Settings::default(),
        );
        assert_eq!(q.status, Status::Error, "{name}");
        assert_eq!(q.samples, 100, "{name}");
    }
}

#[test]
fn write_time_series_need_five_samples() {
    let s = Settings::default();
    let few = run(&[1.0; 4], SampleKind::WriteTime, &s);
    assert_eq!(few.status, Status::TooFewSamples);
    assert_eq!(few.samples, 4);
    assert_eq!(few.last, 1.0);
    let five = run(&[1.0; 5], SampleKind::WriteTime, &s);
    assert_eq!(five.status, Status::Settled);
    assert_eq!(five.samples, 5);
    // Twenty percent of 20 is 4, raised to the floor of 5: the 15 early values stay out.
    let mut twenty: Vec<f64> = (0..15).map(|i| 100.0 + f64::from(i)).collect();
    twenty.extend([1.0; 5]);
    let q = run(&twenty, SampleKind::WriteTime, &s);
    assert_eq!(q.samples, 5);
    assert_eq!(q.status, Status::Settled);
    assert_eq!(q.mean, 1.0);
    // The same 20 samples read as iterations are too early: a different floor.
    assert_eq!(
        run(&twenty, SampleKind::Iteration, &s).status,
        Status::TooEarly
    );
}

#[test]
fn the_tolerances_are_settings() {
    let v = alternating(251, 1.0, 0.02);
    let default = run(&v, SampleKind::Iteration, &Settings::default());
    assert_eq!(
        default.status,
        Status::Oscillating,
        "spread {}",
        default.spread
    );
    let loose = Settings {
        spread_rel: 0.05,
        ..Settings::default()
    };
    assert_eq!(
        run(&v, SampleKind::Iteration, &loose).status,
        Status::Settled
    );
    // A series that rises 1 percent over its length drifts under the default and settles when
    // the drift tolerance is raised.
    let slow: Vec<f64> = (0..300)
        .map(|i| 1.0 + 0.01 * f64::from(i) / 299.0)
        .collect();
    assert_eq!(iterations(&slow).status, Status::Drifting);
    let patient = Settings {
        spread_rel: 0.05,
        drift_rel: 0.05,
        ..Settings::default()
    };
    assert_eq!(
        run(&slow, SampleKind::Iteration, &patient).status,
        Status::Settled
    );
}

#[test]
fn drift_is_the_change_of_the_fitted_line_across_the_tail() {
    // 1000 samples, a tail of 200 spanning 199 steps of 0.001 each.
    let ramp: Vec<f64> = (0..1000).map(|i| 0.001 * f64::from(i)).collect();
    let q = iterations(&ramp);
    assert_eq!(q.samples, 200);
    assert!((q.drift - 0.199).abs() < 1e-9, "drift {}", q.drift);
    assert_eq!(q.status, Status::Drifting);
    let fall: Vec<f64> = ramp.iter().map(|v| 5.0 - v).collect();
    assert!((iterations(&fall).drift + 0.199).abs() < 1e-9);
}

fn level(t: f64) -> f64 {
    1.0 + 0.5 * (-t / 50.0).exp()
}

fn falling(t: f64) -> f64 {
    (-t / 2000.0).exp()
}

#[test]
fn time_spacing_does_not_change_the_state() {
    let shapes = [
        ("levelling off", level as fn(f64) -> f64, Status::Settled),
        ("still falling", falling as fn(f64) -> f64, Status::Drifting),
    ];
    for (name, shape, want) in shapes {
        for step in [1.0, 10.0] {
            let t: Vec<f64> = (0..)
                .map(|i| f64::from(i) * step)
                .take_while(|t| *t <= 1000.0)
                .collect();
            let x: Vec<f64> = t.iter().map(|t| shape(*t)).collect();
            let q = assess_series(
                &Samples {
                    name: "q",
                    times: &t,
                    values: &x,
                },
                SampleKind::Iteration,
                &Settings::default(),
            );
            assert_eq!(q.status, want, "{name} every {step}");
        }
    }
}

#[test]
fn empty_series() {
    let s = Settings::default();
    let q = run(&[], SampleKind::Iteration, &s);
    assert_eq!(q.status, Status::TooEarly);
    assert_eq!(q.samples, 0);
    assert_eq!(q.last, 0.0);
    let q = run(&[], SampleKind::WriteTime, &s);
    assert_eq!(q.status, Status::TooFewSamples);
    assert_eq!(q.samples, 0);
}
