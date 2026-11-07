//! The one function that turns a log and the monitor files into the table and the verdict.
//!
//! A file that cannot be read makes its quantity an error, a function object with no file yet
//! makes its quantities too early, and a missing flux file fails the mass balance. None of these
//! can look settled, so none can make a run converged.

use std::collections::BTreeMap;

use cfdcraft_logscan::{Divergence, Limits, LogState, Scanner, divergence};
use cfdcraft_monitors::{
    FieldKind, LineFile, Series, join_series, line_mean_series, parse_probes, parse_surface,
    parse_xy, probe_series, surface_series,
};
use cfdcraft_settle::{
    Balance, BalanceStatus, PatchFlux, QuantityState, SOLVER_FAILED, SampleKind, Samples, Settings,
    Status, Verdict, assess_series, energy_balance, mass_balance, verdict,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::spec::{Files, Spec, SurfaceQuantityKind};

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

#[derive(Default)]
struct Out {
    quantities: Vec<QuantityState>,
    notes: Vec<String>,
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
        self.quantities.push(assess_series(&samples, kind, s));
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

fn surface(out: &mut Out, files: &Files, fo: &str, quantity: SurfaceQuantityKind, s: &Settings) {
    let (column, suffix) = quantity.column();
    let found = pieces(files, fo);
    if found.is_empty() {
        out.missing(fo, &[suffix]);
        return;
    }
    match surface_joined(fo, found, column, suffix) {
        Some(series) => out.judge(&series, SampleKind::Iteration, s),
        None => out.unreadable(fo, format!("{fo}.{suffix}")),
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

/// Whether the last initial residuals are under the limits: pressure under the first, every
/// velocity component under the second. No limits is no check; limits with no residual yet fail.
fn residuals_ok(limits: Option<(f64, f64)>, rows: &[ResidualRow]) -> bool {
    let Some((p_limit, u_limit)) = limits else {
        return true;
    };
    !rows.is_empty()
        && rows.iter().all(|r| match r.field.as_str() {
            "p" => r.last <= p_limit,
            f if f.starts_with('U') => r.last <= u_limit,
            _ => true,
        })
}

/// The numbers a person sees for a run and the verdict, from the same bytes.
///
/// `files.log` may end mid-line (a run in progress): the unfinished line waits and is not read.
pub fn assess(spec: &Spec, files: &Files, settings: &Settings) -> Assessment {
    let mut scanner = Scanner::new();
    scanner.feed(&files.log);
    let log = scanner.state();

    let mut out = Out::default();
    let mut labels = BTreeMap::new();
    for p in &spec.probes {
        labels.insert(p.fo.clone(), p.label.clone());
        probe(&mut out, files, &p.fo, p.field, settings);
    }
    for l in &spec.lines {
        labels.insert(l.fo.clone(), l.label.clone());
        line(&mut out, files, &l.fo, l.field, settings);
    }
    for sf in &spec.surfaces {
        labels.insert(sf.fo.clone(), sf.label.clone());
        surface(&mut out, files, &sf.fo, sf.quantity, settings);
    }

    let mass = mass(spec, files, settings, &mut out.notes);
    let energy = if spec.solves_energy {
        out.notes.push("unavailable:energy".to_owned());
        failed_balance()
    } else {
        energy_balance(None, settings)
    };

    let rows = residuals(log);
    let ok = residuals_ok(spec.residual_limits, &rows);
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
