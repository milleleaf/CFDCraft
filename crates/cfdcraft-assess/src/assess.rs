//! The one function that turns a log and the monitor files into the table and the verdict.
//!
//! A file that cannot be read makes its quantity an error, a function object with no file yet
//! makes its quantities too early, and a missing flux file fails the mass balance. None of these
//! can look settled, so none can make a run converged.
//!
//! A transient run has no residual limit (the caller passes `None`), and with `Spec::periodic` its
//! probe, surface and force series are judged on periodic settledness: "settled" then means the
//! cycles are steady, not that the value is flat. A series that is flat within the steady rules is
//! settled either way, so a constant drag stays settled beside a shedding lift, and a series with
//! no cycle at all keeps its steady verdict. Line monitors keep the write-time rules whatever the
//! flag says.

use std::collections::BTreeMap;

use cfdcraft_logscan::{Divergence, Limits, LogState, Scanner, divergence};
use cfdcraft_monitors::{
    FieldKind, LineFile, Series, coefficient_series, join_series, line_mean_series,
    parse_coefficients, parse_probes, parse_surface, parse_xy, probe_series, surface_series,
    surface_series_component,
};
use cfdcraft_settle::{
    Balance, BalanceStatus, PatchFlux, PeriodicOptions, PeriodicState, PeriodicStatus,
    QuantityState, SOLVER_FAILED, SampleKind, Samples, Settings, Status, Verdict, assess_periodic,
    assess_series, energy_balance, mass_balance, verdict,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::spec::{Files, Spec, SurfaceQuantityKind, SurfaceSpec};

/// The first and the latest initial residual of one solved field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResidualRow {
    pub field: String,
    pub first: f64,
    pub last: f64,
}

/// Everything shown about a run, and the verdict made from it.
///
/// `notes` are `kind:name` word pairs (`missing:probe_1`, `parse:probe_2`), data for the caller to
/// put into words. `labels` map a function-object name (the prefix of a quantity name) to the
/// study's label for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub iterations: u64,
    pub last_time: Option<f64>,
    pub ended: bool,
    pub residuals: Vec<ResidualRow>,
    pub quantities: Vec<QuantityState>,
    pub mass: Balance,
    pub energy: Balance,
    pub divergence: Option<Divergence>,
    pub verdict: Verdict,
    pub notes: Vec<String>,
    pub labels: BTreeMap<String, String>,
}

type Pieces<'a> = &'a [(String, String)];

fn pieces<'a>(files: &'a Files, fo: &str) -> Pieces<'a> {
    files.monitor_files.get(fo).map_or(&[][..], Vec::as_slice)
}

fn state(name: String, status: Status) -> QuantityState {
    QuantityState {
        name,
        last: 0.0,
        mean: 0.0,
        spread: 0.0,
        drift: 0.0,
        status,
        samples: 0,
    }
}

fn failed_balance() -> Balance {
    Balance {
        status: BalanceStatus::Failed,
        imbalance: None,
        reference: None,
    }
}

/// The two series of a force coefficient file that are judged.
const COEFFICIENTS: [&str; 2] = ["Cd", "Cl"];

#[derive(Default)]
struct Out {
    quantities: Vec<QuantityState>,
    notes: Vec<String>,
    periodic: bool,
}

/// Add a note unless the same one is already there.
fn note_once(notes: &mut Vec<String>, note: String) {
    if !notes.contains(&note) {
        notes.push(note);
    }
}

/// A periodic state as a quantity, status for status.
fn from_periodic(samples: &Samples<'_>, p: &PeriodicState) -> QuantityState {
    QuantityState {
        name: samples.name.to_owned(),
        last: samples.values.last().copied().unwrap_or(0.0),
        mean: p.mean,
        spread: p.amplitude.unwrap_or(0.0),
        drift: p.disagreement.unwrap_or(0.0),
        status: match p.status {
            PeriodicStatus::Settled => Status::Settled,
            PeriodicStatus::Drifting => Status::Drifting,
            PeriodicStatus::TooEarly => Status::TooEarly,
            PeriodicStatus::TooFewSamples => Status::TooFewSamples,
            PeriodicStatus::Error => Status::Error,
        },
        samples: samples.values.len() as u64,
    }
}

impl Out {
    /// Nothing has been written for `fo` yet: its quantities are too early to judge.
    fn missing(&mut self, fo: &str, suffixes: &[&str]) {
        self.notes.push(format!("missing:{fo}"));
        for suffix in suffixes {
            self.quantities
                .push(state(format!("{fo}.{suffix}"), Status::TooEarly));
        }
    }

    /// A file of `fo` could not be read: one error quantity, never a settled one.
    fn unreadable(&mut self, fo: &str, name: String) {
        self.notes.push(format!("parse:{fo}"));
        self.quantities.push(state(name, Status::Error));
    }

    fn judge(&mut self, series: &Series, kind: SampleKind, s: &Settings) {
        let samples = Samples {
            name: &series.name,
            times: &series.times,
            values: &series.values,
        };
        let steady = assess_series(&samples, kind, s);
        let periodic =
            (self.periodic && kind == SampleKind::Iteration && steady.status != Status::Settled)
                .then(|| assess_periodic(&samples, &PeriodicOptions::default()))
                // A series with no cycle at all (a ramp, a short one) keeps its steady verdict.
                .filter(|p| p.cycles > 0);
        let state = match periodic {
            Some(p) => from_periodic(&samples, &p),
            None => steady,
        };
        self.quantities.push(state);
    }

    /// Join the pieces of each series across start-time folders and judge the results.
    fn judge_joined(&mut self, parts: &[Vec<Series>], width: usize, s: &Settings) {
        for i in 0..width {
            let joined = join_series(parts.iter().filter_map(|p| p.get(i).cloned()).collect());
            self.judge(&joined, SampleKind::Iteration, s);
        }
    }
}

fn probe(out: &mut Out, files: &Files, fo: &str, field: FieldKind, s: &Settings) {
    let (names, whole): (&[&str], &str) = match field {
        FieldKind::Velocity => (&["Ux", "Uy"], "U"),
        FieldKind::Pressure => (&["p"], "p"),
    };
    let found = pieces(files, fo);
    if found.is_empty() {
        out.missing(fo, names);
        return;
    }
    let parsed: Result<Vec<Vec<Series>>, _> = found
        .iter()
        .map(|(_, text)| parse_probes(text).map(|f| probe_series(&f, fo, 0, field)))
        .collect();
    match parsed {
        Ok(parts) => out.judge_joined(&parts, names.len(), s),
        Err(_) => out.unreadable(fo, format!("{fo}.{whole}")),
    }
}

fn line(out: &mut Out, files: &Files, fo: &str, field: FieldKind, s: &Settings) {
    let (names, whole): (&[&str], &str) = match field {
        FieldKind::Velocity => (&["mean_Ux", "mean_Uy"], "mean_U"),
        FieldKind::Pressure => (&["mean_p"], "mean_p"),
    };
    let found = pieces(files, fo);
    if found.is_empty() {
        out.missing(fo, names);
        return;
    }
    let parsed: Option<Vec<(f64, LineFile)>> = found
        .iter()
        .map(|(dir, text)| {
            let time = dir.parse::<f64>().ok().filter(|t| t.is_finite())?;
            Some((time, parse_xy(text).ok()?))
        })
        .collect();
    let Some(mut files_by_time) = parsed else {
        out.unreadable(fo, format!("{fo}.{whole}"));
        return;
    };
    files_by_time.sort_by(|a, b| a.0.total_cmp(&b.0));
    for series in line_mean_series(&files_by_time, fo, field) {
        out.judge(&series, SampleKind::WriteTime, s);
    }
}

/// One column of a surface function object, joined across its start-time folders.
fn surface_joined(fo: &str, found: Pieces<'_>, column: &str, suffix: &str) -> Option<Series> {
    let parts = found
        .iter()
        .map(|(_, text)| {
            let file = parse_surface(text).ok()?;
            surface_series(&file, fo, &[(column, suffix)])
                .into_iter()
                .next()
        })
        .collect::<Option<Vec<Series>>>()?;
    Some(join_series(parts))
}

/// One component of a vector column of a surface function object, joined across its start-time
/// folders and named `<fo>.<suffix>`.
fn surface_vector(
    fo: &str,
    found: Pieces<'_>,
    column: &str,
    component: usize,
    suffix: &str,
) -> Option<Series> {
    let prefix = format!("{fo}.{column}.");
    let parts = found
        .iter()
        .map(|(_, text)| {
            let file = parse_surface(text).ok()?;
            surface_series_component(&file, fo, component)
                .into_iter()
                .find(|s| s.name.starts_with(&prefix))
        })
        .collect::<Option<Vec<Series>>>()?;
    let mut joined = join_series(parts);
    joined.name = format!("{fo}.{suffix}");
    Some(joined)
}

fn surface(out: &mut Out, files: &Files, sf: &SurfaceSpec, s: &Settings) {
    let fo = sf.fo.as_str();
    let (column, suffix) = sf.quantity.column();
    let found = pieces(files, fo);
    if found.is_empty() {
        out.missing(fo, &[suffix]);
        return;
    }
    let joined = match (sf.quantity, sf.component) {
        (SurfaceQuantityKind::HeatFlux, Some(c)) => surface_vector(fo, found, column, c, suffix),
        _ => surface_joined(fo, found, column, suffix),
    };
    match joined {
        Some(series) => out.judge(&series, SampleKind::Iteration, s),
        None => out.unreadable(fo, format!("{fo}.{suffix}")),
    }
}

/// The drag and lift series of a force coefficient function object, joined across its start-time
/// folders. A column the files do not hold is an error, never a settled quantity.
fn force(out: &mut Out, files: &Files, fo: &str, s: &Settings) {
    let found = pieces(files, fo);
    if found.is_empty() {
        out.missing(fo, &COEFFICIENTS);
        return;
    }
    let parsed: Result<Vec<Vec<Series>>, _> = found
        .iter()
        .map(|(_, text)| parse_coefficients(text).map(|f| coefficient_series(&f, fo)))
        .collect();
    let Ok(parts) = parsed else {
        out.unreadable(fo, format!("{fo}.coefficients"));
        return;
    };
    for suffix in COEFFICIENTS {
        let name = format!("{fo}.{suffix}");
        let wanted: Vec<Series> = parts
            .iter()
            .filter_map(|p| p.iter().find(|x| x.name == name).cloned())
            .collect();
        if wanted.is_empty() {
            note_once(&mut out.notes, format!("parse:{fo}"));
            out.quantities.push(state(name, Status::Error));
        } else {
            out.judge(&join_series(wanted), SampleKind::Iteration, s);
        }
    }
}

/// The mass balance. A domain with no inlet is not applicable whatever the files hold; with one,
/// every patch needs a readable flux file or the balance fails.
fn mass(spec: &Spec, files: &Files, s: &Settings, notes: &mut Vec<String>) -> Balance {
    if !spec.patches.iter().any(|p| p.inlet) {
        return mass_balance(&[], s);
    }
    let mut fluxes = Vec::new();
    let mut complete = true;
    for p in &spec.patches {
        let found = pieces(files, &p.fo);
        if found.is_empty() {
            notes.push(format!("missing:{}", p.fo));
            complete = false;
            continue;
        }
        let (column, suffix) = SurfaceQuantityKind::Flux.column();
        match surface_joined(&p.fo, found, column, suffix).and_then(|j| j.values.last().copied()) {
            Some(sum) => fluxes.push(PatchFlux {
                patch: p.patch.clone(),
                inlet: p.inlet,
                sum,
                sum_mag: sum.abs(),
            }),
            None => {
                notes.push(format!("parse:{}", p.fo));
                complete = false;
            }
        }
    }
    if complete {
        mass_balance(&fluxes, s)
    } else {
        failed_balance()
    }
}

/// The last heat flow of one wall surface, or `None` with the reason in the notes.
///
/// The component defaults to x, the first part of the vector.
fn wall_heat(spec: &Spec, files: &Files, fo: &str, notes: &mut Vec<String>) -> Option<f64> {
    let (column, suffix) = SurfaceQuantityKind::HeatFlux.column();
    let found = pieces(files, fo);
    let Some(sf) = spec.surfaces.iter().find(|sf| sf.fo == fo) else {
        note_once(notes, format!("missing:{fo}"));
        return None;
    };
    if found.is_empty() {
        note_once(notes, format!("missing:{fo}"));
        return None;
    }
    let last = surface_vector(fo, found, column, sf.component.unwrap_or(0), suffix)
        .and_then(|j| j.values.last().copied());
    if last.is_none() {
        note_once(notes, format!("parse:{fo}"));
    }
    last
}

/// The energy balance. Where energy is not solved it is not applicable. Where it is, the net heat
/// flow is the hot wall's minus the cold wall's, measured against the hot wall's; a missing or
/// unreadable flow, or no wall monitors at all, fails the balance.
fn energy(spec: &Spec, files: &Files, s: &Settings, notes: &mut Vec<String>) -> Balance {
    if !spec.solves_energy {
        return energy_balance(None, s);
    }
    let Some(heat) = &spec.heat else {
        notes.push("unavailable:energy".to_owned());
        return failed_balance();
    };
    let hot = wall_heat(spec, files, &heat.hot, notes);
    let cold = wall_heat(spec, files, &heat.cold, notes);
    match (hot, cold) {
        (Some(h), Some(c)) => energy_balance(Some((h - c, h.abs())), s),
        _ => failed_balance(),
    }
}

fn residuals(log: &LogState) -> Vec<ResidualRow> {
    log.last_initial
        .iter()
        .map(|(field, last)| ResidualRow {
            field: field.clone(),
            first: log.first_initial.get(field).copied().unwrap_or(*last),
            last: *last,
        })
        .collect()
}

/// Whether the last initial residuals are under the limits: the spec's pressure field under the
/// first, every velocity component (and the temperature, when it is solved) under the second.
/// Other fields are shown and not gated. No limits is no check; limits with no residual yet fail.
fn residuals_ok(spec: &Spec, rows: &[ResidualRow]) -> bool {
    let Some((p_limit, u_limit)) = spec.residual_limits else {
        return true;
    };
    !rows.is_empty()
        && rows.iter().all(|r| {
            if r.field == spec.pressure_field {
                r.last <= p_limit
            } else if r.field.starts_with('U') || (spec.temperature && r.field == "T") {
                r.last <= u_limit
            } else {
                true
            }
        })
}

/// The numbers a person sees for a run and the verdict, from the same bytes.
///
/// `files.log` may end mid-line (a run in progress): the unfinished line waits and is not read.
pub fn assess(spec: &Spec, files: &Files, settings: &Settings) -> Assessment {
    let mut scanner = Scanner::new();
    scanner.feed(&files.log);
    let log = scanner.state();

    let mut out = Out {
        periodic: spec.periodic,
        ..Out::default()
    };
    let mut labels = BTreeMap::new();
    for p in &spec.probes {
        labels.insert(p.fo.clone(), p.label.clone());
        probe(&mut out, files, &p.fo, p.field, settings);
    }
    for l in &spec.lines {
        labels.insert(l.fo.clone(), l.label.clone());
        line(&mut out, files, &l.fo, l.field, settings);
    }
    for f in &spec.forces {
        labels.insert(f.fo.clone(), f.label.clone());
        force(&mut out, files, &f.fo, settings);
    }
    for sf in &spec.surfaces {
        labels.insert(sf.fo.clone(), sf.label.clone());
        surface(&mut out, files, sf, settings);
    }

    let mass = mass(spec, files, settings, &mut out.notes);
    let energy = energy(spec, files, settings, &mut out.notes);

    let rows = residuals(log);
    let ok = residuals_ok(spec, &rows);
    let verdict = verdict(
        out.quantities.clone(),
        mass.clone(),
        energy.clone(),
        ok,
        settings,
    );
    // A log that shows the solver failed is never a converged run, whatever the probes say.
    let verdict = if log.fatal.is_some() || log.non_finite {
        verdict.blocked_by(SOLVER_FAILED)
    } else {
        verdict
    };
    Assessment {
        iterations: log.iterations,
        last_time: log.last_time,
        ended: log.ended,
        residuals: rows,
        quantities: out.quantities,
        mass,
        energy,
        divergence: divergence(log, &Limits::default(), spec.transient),
        verdict,
        notes: out.notes,
        labels,
    }
}
