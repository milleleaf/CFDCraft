//! The frequency and the cycle extremes of a series that oscillates by design, such as the lift of a shedding cylinder.
//!
//! The tail rules of `assess_series` call such a series oscillating. Here the oscillation is the
//! signal: the series is cut into cycles at its up-crossings of its own mean (linear interpolation
//! between the two samples around a sign change, so uneven time steps are fine), and the last
//! whole cycles give the period, the amplitude, the maxima and minima of each cycle and the time of
//! each maximum. No Fourier transform is used.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::series::Samples;

/// Fewer samples than this cannot show a cycle.
const MIN_SAMPLES: usize = 8;
/// The sine fit searches this fraction either side of the frequency the crossings give.
const FIT_SPAN: f64 = 0.05;
/// Golden-section steps of the sine fit.
const FIT_STEPS: usize = 40;

/// How many whole cycles are needed and how steady they must be.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeriodicOptions {
    /// Whole cycles needed before the series is judged.
    pub min_cycles: u64,
    /// The last this many whole cycles are measured.
    pub window_cycles: u64,
    /// Largest change of the amplitude across the window, relative to its mean.
    pub amplitude_rel: f64,
    /// Largest change of the cycle length across the window, relative to its mean.
    pub period_rel: f64,
}

impl Default for PeriodicOptions {
    fn default() -> Self {
        Self {
            min_cycles: 10,
            window_cycles: 5,
            amplitude_rel: 0.01,
            period_rel: 0.01,
        }
    }
}

/// Where a periodic series stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeriodicStatus {
    /// Enough cycles, and the amplitude and the cycle length are steady across the window.
    Settled,
    /// Enough cycles, but the amplitude or the cycle length still changes across the window.
    Drifting,
    /// Fewer whole cycles than needed, or none.
    TooEarly,
    /// Too few samples to show a cycle.
    TooFewSamples,
    /// A non-finite value, unequal lengths or times that do not increase.
    Error,
}

/// The numbers for a periodic series. They are given whenever a cycle was found.
///
/// The per-cycle vectors have one entry per cycle of the judged window, oldest first.
/// `disagreement` is the relative difference between the frequency from the crossings and the
/// frequency of a least-squares sine fitted to the window; above 0.01 the series is not a single
/// periodic mode and a report should say so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeriodicState {
    pub status: PeriodicStatus,
    /// Whole cycles in the series.
    pub cycles: u64,
    pub period: Option<f64>,
    pub frequency: Option<f64>,
    /// The mean over the window of half the maximum minus the minimum of a cycle.
    pub amplitude: Option<f64>,
    /// The mean value over the window (the mean of the whole series when no cycle was found).
    pub mean: f64,
    pub maxima: Vec<f64>,
    pub minima: Vec<f64>,
    /// The time of the maximum of each cycle.
    pub maximum_times: Vec<f64>,
    pub sine_fit_frequency: Option<f64>,
    pub disagreement: Option<f64>,
}

fn blank(status: PeriodicStatus, mean: f64) -> PeriodicState {
    PeriodicState {
        status,
        cycles: 0,
        period: None,
        frequency: None,
        amplitude: None,
        mean,
        maxima: Vec::new(),
        minima: Vec::new(),
        maximum_times: Vec::new(),
        sine_fit_frequency: None,
        disagreement: None,
    }
}

/// A rising crossing of the level: its time, and the index of the first sample at or above it.
struct Crossing {
    time: f64,
    after: usize,
}

fn up_crossings(ts: &[f64], xs: &[f64], level: f64) -> Vec<Crossing> {
    let mut found = Vec::new();
    for (i, (t, x)) in ts.windows(2).zip(xs.windows(2)).enumerate() {
        let (v0, v1) = (x[0] - level, x[1] - level);
        if v0 < 0.0 && v1 >= 0.0 {
            found.push(Crossing {
                time: t[0] + (t[1] - t[0]) * (-v0) / (v1 - v0),
                after: i + 1,
            });
        }
    }
    found
}

/// The largest value, the smallest value and the time of the largest, over samples `a..b`.
fn extremes(ts: &[f64], xs: &[f64], a: usize, b: usize) -> (f64, f64, f64) {
    let (mut hi, mut lo, mut at) = (f64::NEG_INFINITY, f64::INFINITY, ts[a]);
    for (&t, &x) in ts[a..b].iter().zip(&xs[a..b]) {
        if x > hi {
            hi = x;
            at = t;
        }
        lo = lo.min(x);
    }
    (hi, lo, at)
}

/// The mean of the piecewise-linear signal between two crossings, where it equals `level`.
fn window_mean(ts: &[f64], xs: &[f64], from: &Crossing, to: &Crossing, level: f64) -> f64 {
    let mut points = vec![(from.time, level)];
    points.extend(
        ts[from.after..to.after]
            .iter()
            .copied()
            .zip(xs[from.after..to.after].iter().copied()),
    );
    points.push((to.time, level));
    let area: f64 = points
        .windows(2)
        .map(|p| 0.5 * (p[0].1 + p[1].1) * (p[1].0 - p[0].0))
        .sum();
    area / (to.time - from.time)
}

/// The largest minus the smallest of the values, relative to their mean.
fn relative_spread(values: &[f64]) -> f64 {
    let (lo, hi) = values
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let mean = values.iter().sum::<f64>() / (values.len() as f64);
    if mean.abs() > 0.0 {
        (hi - lo) / mean.abs()
    } else {
        0.0
    }
}

type Matrix3 = [[f64; 3]; 3];

fn det3(m: &Matrix3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// The solution of `m x = b` by Cramer's rule, or `None` when `m` is singular.
fn solve3(m: &Matrix3, b: &[f64; 3]) -> Option<[f64; 3]> {
    let det = det3(m);
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let mut x = [0.0; 3];
    for (column, slot) in x.iter_mut().enumerate() {
        let mut swapped = *m;
        for (row, value) in swapped.iter_mut().zip(b) {
            row[column] = *value;
        }
        *slot = det3(&swapped) / det;
    }
    Some(x)
}

/// The summed squared residual of the best `a sin + b cos + c` at frequency `f`.
fn fit_error(ts: &[f64], xs: &[f64], f: f64) -> f64 {
    let t0 = ts[0];
    let basis = |t: f64| {
        let w = 2.0 * std::f64::consts::PI * f * (t - t0);
        [w.sin(), w.cos(), 1.0]
    };
    let (mut normal, mut rhs) = ([[0.0; 3]; 3], [0.0; 3]);
    for (&t, &x) in ts.iter().zip(xs) {
        let r = basis(t);
        for (row, ri) in normal.iter_mut().zip(r) {
            for (cell, rj) in row.iter_mut().zip(r) {
                *cell += ri * rj;
            }
        }
        for (slot, ri) in rhs.iter_mut().zip(r) {
            *slot += ri * x;
        }
    }
    let Some(c) = solve3(&normal, &rhs) else {
        return f64::INFINITY;
    };
    ts.iter()
        .zip(xs)
        .map(|(&t, &x)| {
            let r = basis(t);
            let e = x - (c[0] * r[0] + c[1] * r[1] + c[2] * r[2]);
            e * e
        })
        .sum()
}

/// The frequency near `f0` (within +-5 %) at which a fitted sine leaves the least residual.
fn fitted_frequency(ts: &[f64], xs: &[f64], f0: f64) -> f64 {
    let golden = (5.0_f64.sqrt() - 1.0) / 2.0;
    let (mut lo, mut hi) = (f0 * (1.0 - FIT_SPAN), f0 * (1.0 + FIT_SPAN));
    let (mut a, mut b) = (hi - golden * (hi - lo), lo + golden * (hi - lo));
    let (mut fa, mut fb) = (fit_error(ts, xs, a), fit_error(ts, xs, b));
    for _ in 0..FIT_STEPS {
        if fa < fb {
            hi = b;
            b = a;
            fb = fa;
            a = hi - golden * (hi - lo);
            fa = fit_error(ts, xs, a);
        } else {
            lo = a;
            a = b;
            fa = fb;
            b = lo + golden * (hi - lo);
            fb = fit_error(ts, xs, b);
        }
    }
    0.5 * (lo + hi)
}

/// The frequency of the flow's shedding in units of the length and speed: `f L / U`.
pub fn strouhal(frequency: f64, length: f64, velocity: f64) -> f64 {
    frequency * length / velocity
}

/// Judge a periodic series.
///
/// Refused as `Error`: unequal lengths, a value that is not finite, times that do not strictly
/// increase. Fewer than eight samples is `TooFewSamples`. The series is cut into cycles at its
/// up-crossings of its own mean. With fewer whole cycles than `min_cycles` (or none) the status is
/// `TooEarly`; with enough, it is `Drifting` when the cycle amplitudes, or the cycle lengths, of the
/// last `window_cycles` cycles differ (largest minus smallest, relative to their mean) by more than
/// `amplitude_rel` or `period_rel`, and `Settled` otherwise.
///
/// A sine is also fitted to the window and its frequency is reported beside the crossing one. They
/// agree for a single periodic mode; a `disagreement` above 0.01 says the series is not one, for
/// example two shedding frequencies at once.
pub fn assess_periodic(series: &Samples<'_>, o: &PeriodicOptions) -> PeriodicState {
    let (ts, xs) = (series.times, series.values);
    let n = xs.len();
    let ordered = ts.windows(2).all(|w| w[0] < w[1]);
    if ts.len() != n || !ordered || !ts.iter().chain(xs).all(|v| v.is_finite()) {
        return blank(PeriodicStatus::Error, 0.0);
    }
    if n < MIN_SAMPLES {
        return blank(PeriodicStatus::TooFewSamples, 0.0);
    }
    let level = xs.iter().sum::<f64>() / (n as f64);
    let crossings = up_crossings(ts, xs, level);
    let cycles = crossings.len().saturating_sub(1);
    if cycles == 0 {
        return blank(PeriodicStatus::TooEarly, level);
    }
    let w = usize::try_from(o.window_cycles)
        .unwrap_or(usize::MAX)
        .clamp(1, cycles);
    let window = &crossings[crossings.len() - w - 1..];
    let (first, last) = (&window[0], &window[w]);
    let (mut maxima, mut minima, mut maximum_times) = (Vec::new(), Vec::new(), Vec::new());
    for pair in window.windows(2) {
        let (hi, lo, at) = extremes(ts, xs, pair[0].after, pair[1].after);
        maxima.push(hi);
        minima.push(lo);
        maximum_times.push(at);
    }
    let period = (last.time - first.time) / (w as f64);
    let lengths: Vec<f64> = window.windows(2).map(|p| p[1].time - p[0].time).collect();
    let amplitudes: Vec<f64> = maxima
        .iter()
        .zip(&minima)
        .map(|(hi, lo)| 0.5 * (hi - lo))
        .collect();
    let status = if (cycles as u64) < o.min_cycles {
        PeriodicStatus::TooEarly
    } else if relative_spread(&amplitudes) > o.amplitude_rel
        || relative_spread(&lengths) > o.period_rel
    {
        PeriodicStatus::Drifting
    } else {
        PeriodicStatus::Settled
    };
    let frequency = 1.0 / period;
    let (from, to) = (first.after, last.after);
    let fitted = fitted_frequency(&ts[from..to], &xs[from..to], frequency);
    PeriodicState {
        status,
        cycles: cycles as u64,
        period: Some(period),
        frequency: Some(frequency),
        amplitude: Some(amplitudes.iter().sum::<f64>() / (w as f64)),
        mean: window_mean(ts, xs, first, last, level),
        maxima,
        minima,
        maximum_times,
        sine_fit_frequency: Some(fitted),
        disagreement: Some((fitted - frequency).abs() / frequency),
    }
}
