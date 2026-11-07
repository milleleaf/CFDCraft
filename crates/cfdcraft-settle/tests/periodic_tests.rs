use cfdcraft_settle::{
    PeriodicOptions, PeriodicState, PeriodicStatus, Samples, assess_periodic, strouhal,
};

const DT: f64 = 1e-3;

fn grid(end: f64) -> Vec<f64> {
    let n = (end / DT).round() as usize;
    (0..=n).map(|i| (i as f64) * DT).collect()
}

fn sine(t: f64) -> f64 {
    (2.0 * std::f64::consts::PI * 3.0 * t).sin()
}

fn judge(times: &[f64], values: &[f64]) -> PeriodicState {
    assess_periodic(
        &Samples {
            name: "cylinder_forces.Cl",
            times,
            values,
        },
        &PeriodicOptions::default(),
    )
}

fn judge_fn(end: f64, f: impl Fn(f64) -> f64) -> PeriodicState {
    let t = grid(end);
    let v: Vec<f64> = t.iter().map(|&x| f(x)).collect();
    judge(&t, &v)
}

/// A 32-bit linear congruential generator, uniform in [0, 1).
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(self.0) / 4_294_967_296.0
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn a_sine_gives_the_strouhal_number() {
    // Shifted by 5 ms so that no sample sits on the series mean: a sine over exactly 24 periods
    // starts and ends on the zero level, and whether that edge crossing counts then depends on
    // the rounding of a mean of about 1e-16 (22 or 23 cycles).
    let state = judge_fn(8.0, |t| sine(t + 0.005));
    let f = state.frequency.expect("a frequency");
    assert!(close(f / 3.0, 1.0, 1e-3), "frequency {f}");
    assert!(close(strouhal(f, 0.1, 1.0), 0.3, 3e-4));
    assert!(state.cycles >= 23, "cycles {}", state.cycles);
    assert_eq!(state.status, PeriodicStatus::Settled);
}

#[test]
fn irregular_steps_give_the_same_period() {
    let mut rng = Lcg(12345);
    let times: Vec<f64> = grid(8.0)
        .iter()
        .map(|t| t + (rng.next() * 2.0 - 1.0) * 0.3 * DT)
        .collect();
    assert!(times.windows(2).all(|w| w[0] < w[1]));
    let values: Vec<f64> = times.iter().map(|&t| sine(t)).collect();
    let f = judge(&times, &values).frequency.expect("a frequency");
    assert!(close(f / 3.0, 1.0, 1e-3), "frequency {f}");
}

#[test]
fn the_mean_is_removed() {
    let plain = judge_fn(8.0, sine);
    let shifted = judge_fn(8.0, |t| 5.0 + sine(t));
    let (f0, f1) = (
        plain.frequency.unwrap_or(0.0),
        shifted.frequency.unwrap_or(0.0),
    );
    assert!(close(f0 / f1, 1.0, 1e-6), "{f0} {f1}");
    let (a0, a1) = (
        plain.amplitude.unwrap_or(0.0),
        shifted.amplitude.unwrap_or(0.0),
    );
    assert!(close(a0, a1, 1e-6), "{a0} {a1}");
    assert!(close(shifted.mean, 5.0, 1e-3), "mean {}", shifted.mean);
}

#[test]
fn maxima_and_minima_per_cycle() {
    let state = judge_fn(8.0, sine);
    let w = PeriodicOptions::default().window_cycles as usize;
    assert_eq!(state.maxima.len(), w);
    assert_eq!(state.minima.len(), w);
    assert_eq!(state.maximum_times.len(), w);
    assert!(state.maxima.iter().all(|&m| close(m, 1.0, 1e-3)));
    assert!(state.minima.iter().all(|&m| close(m, -1.0, 1e-3)));
    assert!(close(state.amplitude.unwrap_or(0.0), 1.0, 1e-3));
    // The maximum of a cycle comes a quarter period (1/12 s) after the crossing that starts it, and
    // consecutive maxima are one period apart.
    for pair in state.maximum_times.windows(2) {
        assert!(close(pair[1] - pair[0], 1.0 / 3.0, 2e-3));
    }
    let last = *state.maximum_times.last().expect("a maximum");
    let first = *state.maximum_times.first().expect("a maximum");
    let span = last - first;
    assert!(close(span, ((w - 1) as f64) / 3.0, 2e-3), "span {span}");
    // The rising zero crossings of the sine are at multiples of 1/3 s; the maxima sit 1/12 s later.
    let phase = (first - 1.0 / 12.0) * 3.0;
    assert!(close(phase, phase.round(), 1e-2), "phase {phase}");
}

#[test]
fn fewer_than_ten_cycles_is_too_early() {
    let short = judge_fn(2.5, sine);
    assert_eq!(short.status, PeriodicStatus::TooEarly);
    assert_eq!(short.cycles, 7);
    assert!(short.frequency.is_some(), "the numbers are still given");
    let enough = judge_fn(3.5, sine);
    assert_eq!(enough.status, PeriodicStatus::Settled);
    assert_eq!(enough.cycles, 10);
}

#[test]
fn a_growing_amplitude_is_drifting() {
    let state = judge_fn(8.0, |t| (1.0 + 0.05 * t) * sine(t));
    assert_eq!(state.status, PeriodicStatus::Drifting);
}

#[test]
fn a_changing_period_is_drifting() {
    // The frequency moves from 3.0 to 3.3 over the series.
    let state = judge_fn(8.0, |t| {
        (2.0 * std::f64::consts::PI * (3.0 * t + 0.01875 * t * t)).sin()
    });
    assert_eq!(state.status, PeriodicStatus::Drifting);
}

#[test]
fn noise_below_the_tolerance_still_settles() {
    let rng = std::cell::RefCell::new(Lcg(777));
    let state = judge_fn(8.0, |t| {
        sine(t) + 0.002 * (rng.borrow_mut().next() * 2.0 - 1.0)
    });
    assert_eq!(state.status, PeriodicStatus::Settled);
    let f = state.frequency.expect("a frequency");
    assert!(close(f / 3.0, 1.0, 1e-3), "frequency {f}");
}

#[test]
fn the_sine_fit_agrees_or_the_disagreement_is_reported() {
    let clean = judge_fn(8.0, sine);
    let d = clean.disagreement.expect("a disagreement");
    assert!(d < 1e-3, "clean sine {d}");
    let fit = clean.sine_fit_frequency.expect("a fitted frequency");
    assert!(close(fit / 3.0, 1.0, 1e-3), "fit {fit}");
    let two = judge_fn(8.0, |t| {
        sine(t) + (2.0 * std::f64::consts::PI * 4.5 * t).sin()
    });
    let d = two.disagreement.expect("a disagreement");
    assert!(d > 0.01, "two sines {d}");
}

#[test]
fn bad_input_is_an_error_status() {
    let t = grid(1.0);
    let v: Vec<f64> = t.iter().map(|&x| sine(x)).collect();
    let short = judge(&t[..t.len() - 1], &v);
    assert_eq!(short.status, PeriodicStatus::Error);
    let mut nan = v.clone();
    nan[10] = f64::NAN;
    assert_eq!(judge(&t, &nan).status, PeriodicStatus::Error);
    let mut back = t.clone();
    back[20] = back[19];
    assert_eq!(judge(&back, &v).status, PeriodicStatus::Error);
    assert_eq!(
        judge(&t[..7], &v[..7]).status,
        PeriodicStatus::TooFewSamples
    );
    let constant: Vec<f64> = std::iter::repeat_n(2.5, t.len()).collect();
    let flat = judge(&t, &constant);
    assert_eq!(flat.status, PeriodicStatus::TooEarly);
    assert_eq!(flat.frequency, None);
    assert_eq!(flat.cycles, 0);
}
