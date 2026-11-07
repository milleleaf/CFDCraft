//! Mesh independence of a document: a sweep over the mesh level, and the grid convergence index
//! read from the key numbers its runs recorded.
//!
//! The sweep is the ordinary sweep over `/mesh/level`, so the queue and the run machinery treat its
//! points like any other. The report asks for no new run: it reads the newest recorded result of
//! each point, takes the cell count and area from the point's own study, and hands both to the
//! pure convergence crate.
//!
//! Finding codes of this family: `CFD-GCI-001` the sweep is unknown, or some point has no recorded
//! result holding the quantity; `CFD-GCI-002` the meshes cannot be compared (they are not refined,
//! or a number is not finite). Both are refusals, exit class 2. Warnings about the series are data
//! in the report and sentences from [`gci_warning_lines`]; they never change the exit class.

use std::path::Path;

use cfdcraft_doc::{Document, open};
use cfdcraft_gci::{
    Convergence, GciError, GciReport, Level, Options, SAFETY_FACTOR, Warning, analyse_series,
};
use cfdcraft_model::{MAX_LEVEL, Study, TimeMode, TurbulenceModel, resolve};
use cfdcraft_sweep::{Sweep, points};
use cfdcraft_text::Line;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::documents::{
    DocOutcome, Finding, is_pack_name, pack_kept, problem, refused, study_hash,
};
use crate::jobs::open_refusal;
use crate::report::Report;
use crate::repro::Repro;
use crate::scenario_ops::effective_of;
use crate::sweep_ops::sweep_create;

const CREATE: &str = "gci-create";
const REPORT: &str = "gci-report";

/// The levels of a mesh independence sweep when none are given.
pub const DEFAULT_LEVELS: [u32; 3] = [0, 1, 2];

type Reply = Result<DocOutcome, Box<DocOutcome>>;

/// What a mesh independence sweep is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GciRequest {
    /// The id of the sweep. Its points are called `<sweep>-001`, `<sweep>-002`, and so on.
    pub sweep: String,
    /// The variant the points descend from; `base` is the study itself.
    pub scenario: String,
    /// The mesh levels, three to five, each above the one before and at most the largest level.
    pub levels: Vec<u32>,
}

impl GciRequest {
    /// A request for the default levels on the study itself.
    pub fn new(sweep: &str) -> Self {
        Self {
            sweep: sweep.to_owned(),
            scenario: "base".to_owned(),
            levels: DEFAULT_LEVELS.to_vec(),
        }
    }
}

/// One mesh of the series: the point that made it and the number its run recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GciLevelRow {
    /// The variant the point is stored as.
    pub scenario: String,
    /// The mesh level of the point's study.
    pub level: u32,
    /// The cells of the point's mesh.
    pub cells: u64,
    /// The recorded value of the quantity.
    pub value: f64,
    /// The run the value was read from.
    pub run: String,
}

/// What the report holds: the series and the analysis of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GciData {
    pub sweep: String,
    /// The key number that was read from each run.
    pub quantity: String,
    /// The points in sweep order.
    pub levels: Vec<GciLevelRow>,
    /// One analysis for every three neighbouring meshes, the coarsest three first and the finest
    /// three last.
    pub reports: Vec<GciReport>,
}

/// An id or name given by a person, shortened and with control characters replaced, for a message.
fn shown(text: &str) -> String {
    text.chars()
        .take(40)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn fail(operation: &str, found: Finding) -> Box<DocOutcome> {
    Box::new(refused(operation, problem(found)))
}

fn sweep_refused(operation: &str, sweep: &str, reason: String) -> Box<DocOutcome> {
    let sweep = shown(sweep);
    fail(
        operation,
        ("CFD-SWP-002", Line::SweepRefused { sweep, reason }),
    )
}

/// Why a list of levels cannot make a series, if it cannot.
fn level_problem(levels: &[u32]) -> Option<String> {
    if !(3..=5).contains(&levels.len()) {
        return Some(format!(
            "{} mesh levels were given, and three to five are needed",
            levels.len()
        ));
    }
    if let Some(level) = levels.iter().find(|level| **level > MAX_LEVEL) {
        return Some(format!(
            "the mesh level {level} is above the largest level, {MAX_LEVEL}"
        ));
    }
    if levels.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Some("the mesh levels must rise, each above the one before".to_owned());
    }
    None
}

/// The sweep over `/mesh/level` with the levels as its values.
fn level_sweep(request: &GciRequest) -> Result<Sweep, String> {
    serde_json::from_value(json!({
        "id": request.sweep,
        "base": request.scenario,
        "parameters": [{
            "name": "level",
            "target": { "kind": "count", "setting": "/mesh/level" },
            "values": { "mode": "list", "items": request.levels },
        }],
    }))
    .map_err(|e| e.to_string())
}

/// Store a sweep over the mesh level in the document: one variant for each level, holding the
/// single change `/mesh/level`. The sweep is refused for the reasons any sweep is refused, and
/// when the levels are fewer than three, more than five, not rising, or above the largest level.
/// Nothing is run; the points are queued and solved like any other sweep.
pub fn gci_create(path: &Path, request: &GciRequest) -> DocOutcome {
    if let Some(reason) = level_problem(&request.levels) {
        return *sweep_refused(CREATE, &request.sweep, reason);
    }
    let sweep = match level_sweep(request) {
        Ok(sweep) => sweep,
        Err(reason) => return *sweep_refused(CREATE, &request.sweep, reason),
    };
    let mut outcome = sweep_create(path, &sweep);
    let renamed = match &outcome.line {
        Some(Line::SweepCreated { sweep, points }) => Some(Line::GciCreated {
            sweep: sweep.clone(),
            points: points.clone(),
        }),
        _ => None,
    };
    if let Some(line) = renamed {
        outcome.line = Some(line);
        outcome.report.operation = CREATE.to_owned();
    }
    outcome
}

/// The document, opened for reading. A pack is refused, as it is for every sweep operation.
fn open_checked(path: &Path) -> Result<Document, Box<DocOutcome>> {
    let kept = || fail(REPORT, pack_kept(path));
    if is_pack_name(path) {
        return Err(kept());
    }
    let opened = open(path).map_err(|e| Box::new(open_refusal(REPORT, &e, path)))?;
    if opened.packed_entries > 0 {
        return Err(kept());
    }
    Ok(opened.document)
}

/// Whether the series is a time-dependent one, or uses wall functions, as the study says.
fn options_of(study: &Study) -> Options {
    let (transient, wall_function) = study.physics.as_ref().map_or((false, false), |physics| {
        (
            matches!(physics.time, TimeMode::Transient { .. }),
            !matches!(physics.turbulence.model, TurbulenceModel::Laminar),
        )
    });
    Options {
        safety: SAFETY_FACTOR,
        transient,
        wall_function,
        noise: None,
    }
}

/// The refusal for an analysis that cannot be made.
fn analysis_refusal(error: &GciError) -> Finding {
    let reason = match error {
        GciError::NotRefined { .. } => {
            "the meshes are not refined: two points have the same cell count".to_owned()
        }
        other => other.to_string(),
    };
    ("CFD-GCI-002", Line::ScenarioRefused { reason })
}

/// The word for how the values approach a limit, as a sentence uses it.
fn class_word(convergence: Convergence) -> &'static str {
    match convergence {
        Convergence::Monotone => "monotone",
        Convergence::Oscillatory => "oscillatory",
        Convergence::NoPositiveOrder => "not converging",
        Convergence::Divergent => "divergent",
        Convergence::Degenerate => "degenerate",
    }
}

fn summary_line(quantity: &str, report: &GciReport) -> Line {
    let quantity = shown(quantity);
    let class = class_word(report.convergence).to_owned();
    match (report.order, report.gci_fine) {
        (Some(order), Some(gci)) => Line::GciSummary {
            quantity,
            order: format!("{order:.2}"),
            gci_percent: format!("{:.2}", gci * 100.0),
            class,
        },
        _ => Line::GciOrderNone { quantity, class },
    }
}

/// Read the grid convergence index of the key number `quantity` over the points of the stored
/// sweep `sweep_id`.
///
/// Each point needs a run made for its variant that recorded a result holding the quantity; the
/// newest such run is used and a rerun is not. Points without one are all named in one refusal
/// (`CFD-GCI-001`). The cell count and the area of each mesh come from the point's own study, and
/// meshes that are not refined are refused (`CFD-GCI-002`). With three points the report holds one
/// analysis; with more it holds one for every three neighbouring meshes. A series with no
/// positive order of convergence still passes, with a sentence that says so.
pub fn gci_report(path: &Path, sweep_id: &str, quantity: &str) -> DocOutcome {
    report_inner(path, sweep_id, quantity).unwrap_or_else(|refusal| *refusal)
}

fn report_inner(path: &Path, sweep_id: &str, quantity: &str) -> Reply {
    let document = open_checked(path)?;
    let Some(sweep) = document.artifacts.sweeps.iter().find(|s| s.id == sweep_id) else {
        let sweep = shown(sweep_id);
        return Err(fail(REPORT, ("CFD-GCI-001", Line::SweepUnknown { sweep })));
    };
    let made = points(sweep).map_err(|e| sweep_refused(REPORT, &sweep.id, e.to_string()))?;
    if made.len() < 3 {
        let reason = format!(
            "the sweep has {} points, and at least three meshes are needed",
            made.len()
        );
        return Err(fail(
            REPORT,
            (
                "CFD-GCI-001",
                Line::SweepRefused {
                    sweep: shown(&sweep.id),
                    reason,
                },
            ),
        ));
    }

    // The newest result of each point that is not a rerun.
    let mut found = Vec::with_capacity(made.len());
    let mut missing: Vec<String> = Vec::new();
    for point in &made {
        let run = document.artifacts.runs.iter().rev().find(|run| {
            run.scenario.as_deref() == Some(point.id.as_str())
                && run.result.is_some()
                && run.rerun_of.is_none()
        });
        match run {
            None => missing.push(point.id.clone()),
            Some(run) => match run.key_numbers.get(quantity) {
                Some(value) => found.push((point, run, *value)),
                None => missing.push(format!("{} (no {})", point.id, shown(quantity))),
            },
        }
    }
    if !missing.is_empty() {
        return Err(fail(
            REPORT,
            (
                "CFD-GCI-001",
                Line::GciWaiting {
                    sweep: shown(&sweep.id),
                    missing: missing.join(", "),
                },
            ),
        ));
    }

    let (base, _) = effective_of(REPORT, &document, Some(sweep.base.as_str()))?;
    let options = options_of(&base);
    let mut rows = Vec::with_capacity(found.len());
    let mut meshes = Vec::with_capacity(found.len());
    for (point, run, value) in found {
        let (study, _) = effective_of(REPORT, &document, Some(point.id.as_str()))?;
        let resolved = resolve(&study).map_err(|e| {
            fail(
                REPORT,
                (
                    "CFD-SWP-003",
                    Line::PreflightPointFailed {
                        point: point.id.clone(),
                        reason: e.to_string(),
                    },
                ),
            )
        })?;
        let Some(cells) = resolved.cell_count() else {
            let reason = format!("the mesh of {} has too many cells to count", point.id);
            return Err(fail(
                REPORT,
                ("CFD-GCI-002", Line::ScenarioRefused { reason }),
            ));
        };
        let area = resolved.domain_volume() / resolved.domain.length_z;
        meshes.push(Level { cells, area, value });
        rows.push(GciLevelRow {
            scenario: point.id.clone(),
            level: resolved.mesh.level,
            cells,
            value,
            run: run.id.clone(),
        });
    }

    let reports =
        analyse_series(&meshes, &options).map_err(|e| fail(REPORT, analysis_refusal(&e)))?;
    let Some(finest) = reports.last() else {
        let error = GciError::TooFewLevels {
            found: meshes.len(),
        };
        return Err(fail(REPORT, analysis_refusal(&error)));
    };
    let line = summary_line(quantity, finest);
    let data = GciData {
        sweep: sweep.id.clone(),
        quantity: quantity.to_owned(),
        levels: rows,
        reports,
    };
    let report = Report::from_diagnostics(REPORT, Vec::new(), study_hash(&document.study))
        .with_artifact("sweep", sweep.id.clone())
        .with_repro(Repro::Gci(Box::new(data)));
    Ok(DocOutcome {
        report,
        line: Some(line),
    })
}

fn three_places(number: Option<f64>) -> String {
    number.map(|n| format!("{n:.3}")).unwrap_or_default()
}

/// The sentences for the warnings of the finest analysis in `data`, in the order of the warnings.
/// A clean monotone series gives none.
pub fn gci_warning_lines(data: &GciData) -> Vec<Line> {
    let Some(report) = data.reports.last() else {
        return Vec::new();
    };
    let quantity = shown(&data.quantity);
    report
        .warnings
        .iter()
        .map(|warning| match warning {
            Warning::Oscillatory => Line::GciWarnOscillatory {
                quantity: quantity.clone(),
            },
            Warning::OrderOutOfRange => Line::GciWarnOrder {
                quantity: quantity.clone(),
                order: three_places(report.order),
            },
            Warning::AsymptoticOutOfRange => Line::GciWarnAsymptotic {
                quantity: quantity.clone(),
                ratio: three_places(report.asymptotic_ratio),
            },
            Warning::Transient => Line::GciWarnTransient {
                quantity: quantity.clone(),
            },
            Warning::WallFunction => Line::GciWarnWallFunction {
                quantity: quantity.clone(),
            },
            Warning::NoiseLimited => Line::GciWarnNoise {
                quantity: quantity.clone(),
            },
            Warning::NotRefined => Line::GciWarnNotRefined {
                quantity: quantity.clone(),
            },
        })
        .collect()
}
