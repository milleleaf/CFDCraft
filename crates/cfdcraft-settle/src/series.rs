use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// One named series: values at strictly increasing times.
#[derive(Debug, Clone, Copy)]
pub struct Samples<'a> {
    pub name: &'a str,
    pub times: &'a [f64],
    pub values: &'a [f64],
}

/// What a sample is, which decides how many are needed before a series is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SampleKind {
    /// One sample per solver iteration.
    Iteration,
    /// One sample per written time, as a line monitor gives.
    WriteTime,
}

/// Where a quantity stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Flat within both tolerances over the tail.
    Settled,
    /// The fitted line still moves across the tail.
    Drifting,
    /// Flat on average but the values spread more than the tolerance.
    Oscillating,
    /// Fewer iteration samples than the tail floor.
    TooEarly,
    /// Fewer write-time samples than a line monitor needs.
    TooFewSamples,
    /// A non-finite value, unequal lengths or times that do not increase.
    Error,
}

/// The numbers for one monitored quantity. They are always given, settled or not.
///
/// `samples` is the number of samples the statistics were taken over (the tail); where no tail
/// could be taken (an error) it is the length of the series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuantityState {
    pub name: String,
    pub last: f64,
    pub mean: f64,
    pub spread: f64,
    pub drift: f64,
    pub status: Status,
    pub samples: u64,
}

/// Below this a quantity that is exactly zero all run still has a scale to divide by.
const TINY_SCALE: f64 = 1e-30;

fn bad(name: &str, n: usize) -> QuantityState {
    QuantityState {
        name: name.to_string(),
        last: 0.0,
        mean: 0.0,
        spread: 0.0,
        drift: 0.0,
        status: Status::Error,
        samples: n as u64,
    }
}

fn as_count(v: u64) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

fn range(xs: &[f64]) -> (f64, f64) {
    xs.iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        })
}

/// The tail statistics of a series and whether it has settled.
///
/// The tail is the last `tail_fraction` of the samples, never fewer than the floor for the kind
/// (`tail_floor` for iterations, `line_min_samples` for write times). `spread` is the largest
/// minus the smallest value of the tail; `drift` is the change of the least-squares line across
/// the tail. Both are judged against `scale`, the larger of the tail's absolute mean and
/// `floor_fraction` of the range over the whole run, so a quantity that settles near zero is not
/// judged against its own noise.
pub fn assess_series(series: &Samples<'_>, kind: SampleKind, s: &Settings) -> QuantityState {
    let (ts, xs) = (series.times, series.values);
    let n = xs.len();
    let ordered = ts.windows(2).all(|w| w[0] < w[1]);
    if ts.len() != n || !ordered || !ts.iter().chain(xs).all(|v| v.is_finite()) {
        return bad(series.name, n);
    }
    let (floor, short) = match kind {
        SampleKind::Iteration => (as_count(s.tail_floor), Status::TooEarly),
        SampleKind::WriteTime => (as_count(s.line_min_samples), Status::TooFewSamples),
    };
    let early = n < floor;
    let wanted = ((s.tail_fraction * (n as f64)) - 1e-9).ceil().max(0.0) as usize;
    let k = if early { n } else { wanted.max(floor).min(n) };
    if k == 0 {
        return QuantityState {
            status: short,
            ..bad(series.name, 0)
        };
    }
    let (tx, tt) = (&xs[n - k..], &ts[n - k..]);
    let kf = k as f64;
    let mean = tx.iter().sum::<f64>() / kf;
    let (lo, hi) = range(tx);
    let spread = hi - lo;
    let t_mean = tt.iter().sum::<f64>() / kf;
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for (t, x) in tt.iter().zip(tx) {
        let dt = t - t_mean;
        sxx += dt * dt;
        sxy += dt * (x - mean);
    }
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    let drift = slope * (tt[k - 1] - tt[0]);
    let (all_lo, all_hi) = range(xs);
    let mut scale = mean.abs().max(s.floor_fraction * (all_hi - all_lo));
    if scale == 0.0 {
        scale = TINY_SCALE;
    }
    if ![mean, spread, drift, scale].iter().all(|v| v.is_finite()) {
        return bad(series.name, n);
    }
    let drift_ok = drift.abs() / scale < s.drift_rel;
    let spread_ok = spread / scale < s.spread_rel;
    let status = if early {
        short
    } else if !drift_ok {
        Status::Drifting
    } else if !spread_ok {
        Status::Oscillating
    } else {
        Status::Settled
    };
    QuantityState {
        name: series.name.to_string(),
        last: xs[n - 1],
        mean,
        spread,
        drift,
        status,
        samples: k as u64,
    }
}
