//! The numbers of the benchmark, measured from the recorded series: the Strouhal number, the
//! largest drag and lift over the last whole shedding periods, and the pressure difference half a
//! period after the lift maximum.

use cfdcraft_gate::ResultFiles;
use cfdcraft_model::ResolvedStudy;
use cfdcraft_monitors::Series;
use cfdcraft_settle::{PeriodicOptions, PeriodicStatus, Samples, assess_periodic, strouhal};
use thiserror::Error;

use crate::data::Tolerances;
use crate::files::{load, names};

/// Why a run is not judged. Each text begins with the stable code of the refusal.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Refusal {
    #[error("CFD-BENCH-002: the results lack {file}, which the cylinder comparison needs")]
    Missing { file: String },
    #[error("CFD-BENCH-002: {file} cannot be read as a result file")]
    Unreadable { file: String },
    #[error(
        "CFD-BENCH-003: the force coefficient file does not state a lift direction along y, \
         so its lift is not the force across the flow"
    )]
    LiftDirection,
    #[error(
        "CFD-BENCH-004: the lift shows {found} whole shedding cycles, fewer than the {needed} \
         needed before the run is compared"
    )]
    TooFewCycles { found: u64, needed: u64 },
    #[error(
        "CFD-BENCH-004: the lift amplitude or cycle length still changes over the last shedding \
         cycles, so the run is not periodic yet"
    )]
    Drifting,
}

/// The measured numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    pub st: f64,
    pub cd_max: f64,
    pub cl_max: f64,
    pub delta_p: f64,
    /// Whole shedding cycles in the lift series.
    pub cycles: u64,
}

/// The value of a series at `t`, by linear interpolation; `None` outside the recorded times.
fn at(series: &Series, t: f64) -> Option<f64> {
    let after = series.times.partition_point(|x| *x <= t);
    let before = after.checked_sub(1)?;
    let (t0, v0) = (*series.times.get(before)?, *series.values.get(before)?);
    match (series.times.get(after), series.values.get(after)) {
        (Some(t1), Some(v1)) => Some(v0 + (v1 - v0) * (t - t0) / (t1 - t0)),
        // The last sample is the last time: `t` is on it, or beyond it.
        _ => (t <= t0).then_some(v0),
    }
}

/// The largest value at or after `from`.
fn largest_from(series: &Series, from: f64) -> f64 {
    series
        .times
        .iter()
        .zip(&series.values)
        .filter(|(t, _)| **t >= from)
        .map(|(_, v)| *v)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Measure a recorded run.
///
/// The lift must be along y. The run is refused unless the lift has at least `min_cycles` whole
/// cycles and its amplitude and cycle length are steady over the last `window_cycles`: a lift that
/// is still flat before shedding starts has no cycle at all, so it is refused here and never
/// judged on the strength of looking settled. The Strouhal number comes from the cycle length of
/// the same window.
pub fn measure(
    files: &dyn ResultFiles,
    study: &ResolvedStudy,
    tolerances: &Tolerances,
) -> Result<Measured, Refusal> {
    let fos = names(study)?;
    let input = load(files, &fos)?;
    let options = PeriodicOptions {
        min_cycles: tolerances.min_cycles,
        window_cycles: tolerances.window_cycles,
        ..PeriodicOptions::default()
    };
    let lift = Samples {
        name: "Cl",
        times: &input.cl.times,
        values: &input.cl.values,
    };
    let state = assess_periodic(&lift, &options);
    let few = || Refusal::TooFewCycles {
        found: state.cycles,
        needed: tolerances.min_cycles,
    };
    match state.status {
        PeriodicStatus::Settled => {}
        PeriodicStatus::Drifting => return Err(Refusal::Drifting),
        PeriodicStatus::TooEarly | PeriodicStatus::TooFewSamples => return Err(few()),
        PeriodicStatus::Error => {
            return Err(Refusal::Unreadable {
                file: format!("postProcessing/{}", fos.force),
            });
        }
    }
    let (Some(frequency), Some(period)) = (state.frequency, state.period) else {
        return Err(few());
    };
    let last = input.cl.times.last().copied().ok_or_else(few)?;
    let from = last - (tolerances.window_cycles as f64) * period;
    // Half a period after the lift maximum of the latest cycle that leaves that much recorded.
    let half = 0.5 / frequency;
    let moment = state
        .maximum_times
        .iter()
        .rev()
        .map(|t| t + half)
        .find(|t| *t <= last)
        .ok_or_else(few)?;
    let pressure_at = |series: &Series, fo: &str| {
        at(series, moment).ok_or_else(|| Refusal::Missing {
            file: format!("postProcessing/{fo}"),
        })
    };
    let delta_p = pressure_at(&input.front, &fos.front)? - pressure_at(&input.rear, &fos.rear)?;
    Ok(Measured {
        st: strouhal(frequency, fos.reference_length, fos.reference_velocity),
        cd_max: largest_from(&input.cd, from),
        cl_max: largest_from(&input.cl, from),
        delta_p,
        cycles: state.cycles,
    })
}
