//! The parity table of a benchmark over the runs a document has recorded, and the admission check
//! that reads it.
//!
//! The table is derived from the recorded runs every time it is asked, like `benchmark check`: the
//! runs of the benchmark are grouped by backend and image, the newest of each group is used, the
//! gate verdict of each is recomputed from its stored result (read through the backend's own
//! result view) and the convergence verdict is the one recorded with the run. The baseline is the
//! default backend. The tolerance of a quantity is the width the benchmark's own gate allows for
//! it, read from the gate report of the baseline run; nothing here widens or replaces one.
//!
//! Finding codes of this family: `CFD-PARITY-001` no recorded run under any other backend (also
//! for a backend asked about in an admission check), `002` no usable baseline, `004` a quantity
//! missing in a column (each an error), `003` a column whose gate failed or that is not
//! converged (a warning: the column is shown and never counted), `005` a quantity outside the
//! parity tolerance and `006` an admission refused because a level did not pass (both from the
//! admission check). A preview backend's column carries `CFD-BACKEND-009`, and `CFD-BACKEND-011`
//! while no claim covers the case.

use std::collections::BTreeMap;
use std::path::Path;

use cfdcraft_capture::BUNDLE_CAP;
use cfdcraft_compare::{KeyNumbers, Tolerance, Verdict};
use cfdcraft_doc::{open, store_dir};
use cfdcraft_gate::{Benchmark, GateReport, Mismatch};
use cfdcraft_model::{Diagnostic, ResolvedStudy, resolve};
use cfdcraft_parity::{
    GateResult, Level, ParityColumn, ParityInput, ParityIssue, ParityReport, RecordedColumn, build,
    render_table,
};
use cfdcraft_runs::{RunRecord, verify_runs};
use cfdcraft_settle::VerdictState;
use cfdcraft_store::Store;
use cfdcraft_text::Line;

use crate::backends::{GOLDEN_COMPILE_PASSED, Row, bounded, by_id, default_row, lbm};
use crate::benchmarks::{BundleFiles, build as build_benchmark, ids, not_a_benchmark};
use crate::cases::cli_problem;
use crate::documents::{problem, refused, say, study_hash};
use crate::import_ops::fact;
use crate::jobs::{comparer_files, inconsistent, open_refusal};
use crate::report::Report;
use crate::repro::{ParityData, Repro};

const TABLE: &str = "parity-table";

type Reply = Result<Report, Box<Report>>;

/// The numbers parity compares: the benchmark, the recorded quantity and the gate metric whose
/// tolerance the quantity takes. A recorded number that is not listed here (a count of cycles, an
/// energy imbalance) has no gate tolerance and is not compared.
const PARITY_QUANTITIES: &[(&str, &str, &str)] = &[
    ("ghia", "bench.ghia.u_centre", "u"),
    ("step", "bench.step.reattachment", "x_R/H"),
    ("armaly", "bench.armaly.reattachment", "x1/S"),
    ("cylinder", "bench.cylinder.st", "St"),
    ("cylinder", "bench.cylinder.cd_max", "cDmax"),
    ("cylinder", "bench.cylinder.cl_max", "cLmax"),
    ("cylinder", "bench.cylinder.delta_p", "DeltaP"),
    ("dvd", "bench.dvd.nu_hot", "Nu"),
    ("dvd", "bench.dvd.u_max", "u_max"),
    ("dvd", "bench.dvd.w_max", "w_max"),
    ("dvd", "bench.dvd.u_max_y", "u_max z"),
    ("dvd", "bench.dvd.w_max_x", "w_max x"),
];

/// A run of the benchmark chosen for the table, with the study it recorded.
struct Chosen<'a> {
    /// The place of the run in the log, so the newest of two runs is the later one.
    index: usize,
    record: &'a RunRecord,
    resolved: ResolvedStudy,
}

fn error(code: &str, message: String) -> Diagnostic {
    Diagnostic::error(code, "parity", "", message)
}

fn warning(code: &str, message: String) -> Diagnostic {
    Diagnostic::warning(code, "parity", "", message)
}

fn fail(found: Diagnostic) -> Box<Report> {
    Box::new(Report::from_diagnostics(TABLE, vec![found], None))
}

/// The files of the stored result of a run, or nothing when the result is gone from the store,
/// too large or was never recorded.
fn stored(path: &Path, record: &RunRecord) -> Option<BundleFiles> {
    let out = record.result.as_ref()?;
    if out.size > BUNDLE_CAP {
        return None;
    }
    let store = Store::open(store_dir(path)).ok()?;
    let bytes = store.read(out.blob).ok()?;
    Some(BundleFiles::new(bytes))
}

/// The recorded numbers parity compares for this benchmark.
fn parity_numbers(benchmark: &str, recorded: &KeyNumbers) -> KeyNumbers {
    recorded
        .iter()
        .filter(|(name, _)| {
            PARITY_QUANTITIES
                .iter()
                .any(|(id, quantity, _)| *id == benchmark && *quantity == name.as_str())
        })
        .map(|(name, value)| (name.clone(), *value))
        .collect()
}

/// The tolerance of each parity quantity: the one the gate gave it. A quantity whose metric is not
/// in the gate report has none, and the table says so.
fn gate_tolerances(benchmark: &str, gate: &GateReport) -> BTreeMap<String, Tolerance> {
    PARITY_QUANTITIES
        .iter()
        .filter(|(id, _, _)| *id == benchmark)
        .filter_map(|(_, quantity, metric)| {
            let found = gate.metrics.iter().find(|m| m.name == *metric)?;
            let tolerance = Tolerance::new(found.tolerance, 0.0).ok()?;
            Some(((*quantity).to_owned(), tolerance))
        })
        .collect()
}

/// The column of one chosen run, and the gate report it was made with.
fn recorded_column(
    path: &Path,
    benchmark: &dyn Benchmark,
    chosen: &Chosen<'_>,
) -> (RecordedColumn, GateReport) {
    let record = chosen.record;
    let tool = &record.provenance.tool;
    let known = by_id(&tool.dialect);
    // A run of a backend that is not registered is read through the default row's view, as the
    // other readers of recorded runs do.
    let row: Row = known.unwrap_or_else(default_row);
    let gate = match stored(path, record) {
        Some(files) => {
            let view = comparer_files(&row, Box::new(files), &chosen.resolved);
            benchmark.compare(view.as_ref(), &chosen.resolved)
        }
        None => GateReport::failed_named(
            benchmark.id(),
            "the stored result is not available, so the gate cannot be judged",
        ),
    };
    let converged = match &record.verdict {
        Some(v) if v.state == VerdictState::Converged => Some(true),
        Some(v) if v.state == VerdictState::NotConverged => Some(false),
        _ => None,
    };
    let column = RecordedColumn {
        backend: tool.dialect.clone(),
        image: tool.image_digest.clone(),
        run: record.id.clone(),
        preview: known.is_some_and(|r| r.backend.preview()),
        gate_passed: gate.passed,
        converged,
        numbers: parity_numbers(benchmark.id(), &record.key_numbers),
    };
    (column, gate)
}

/// The newest run of each backend and image that is a run of the benchmark, in the order of
/// backend and image.
fn choose_runs<'a>(
    runs: &'a [RunRecord],
    benchmark: &dyn Benchmark,
) -> BTreeMap<(String, String), Chosen<'a>> {
    let mut groups = BTreeMap::new();
    for (index, record) in runs.iter().enumerate() {
        if record.rerun_of.is_some() || record.result.is_none() || !record.outcome.is_complete() {
            continue;
        }
        let Ok(resolved) = resolve(&record.provenance.settings) else {
            continue;
        };
        if benchmark.matches(&resolved).is_err() {
            continue;
        }
        let tool = &record.provenance.tool;
        groups.insert(
            (tool.dialect.clone(), tool.image_digest.clone()),
            Chosen {
                index,
                record,
                resolved,
            },
        );
    }
    groups
}

/// The sentences of the table, in order: the header, the fixed-format table, the levels and one
/// sentence for each column.
fn render(report: &ParityReport) -> String {
    let mut out = say(&Line::ParityTableHeader {
        benchmark: report.benchmark.clone(),
        case: report.case.clone(),
        baseline: report.baseline.backend.clone(),
    });
    out.push_str("\n\n");
    out.push_str(&render_table(report));
    out.push('\n');
    out.push_str(&say(&Line::ParityLevels {
        golden: report.levels.golden.as_str().to_owned(),
        oracle: report.levels.oracle.as_str().to_owned(),
        parity: report.levels.parity.as_str().to_owned(),
    }));
    out.push('\n');
    for column in &report.columns {
        out.push_str(&say(&column_line(column)));
        out.push('\n');
        let verdict = if column.converged {
            Line::ParityConverged {
                backend: column.backend.clone(),
            }
        } else {
            Line::ParityNotConverged {
                backend: column.backend.clone(),
            }
        };
        out.push_str(&say(&verdict));
        out.push('\n');
        if column.preview {
            out.push_str(&say(&Line::BackendPreview));
            out.push('\n');
        }
    }
    out
}

fn column_line(column: &ParityColumn) -> Line {
    Line::ParityColumn {
        backend: column.backend.clone(),
        image: column.image.clone(),
        gate: column.gate.as_str().to_owned(),
        converged: column.converged.to_string(),
    }
}

/// The findings of a built table: what makes it unusable, then what a person must not miss.
fn findings(
    report: &ParityReport,
    issues: &[ParityIssue],
    baseline_id: &str,
    case: &str,
) -> Vec<Diagnostic> {
    let mut found = Vec::new();
    if report.columns.is_empty() {
        let line = Line::ParityNoRuns {
            benchmark: report.benchmark.clone(),
            backend: format!("any backend besides {baseline_id}"),
        };
        found.push(error("CFD-PARITY-001", say(&line)));
    }
    for issue in issues {
        match issue {
            ParityIssue::BaselineMissing => found.push(error(
                "CFD-PARITY-002",
                fact("the baseline column is missing", baseline_id),
            )),
            ParityIssue::NoTolerance { quantity } => found.push(error(
                "CFD-PARITY-002",
                fact("a baseline quantity without a gate tolerance", quantity),
            )),
            ParityIssue::QuantityMissing { backend, quantity } => found.push(error(
                "CFD-PARITY-004",
                fact(
                    "a quantity is missing in a column",
                    &format!("{backend} {quantity}"),
                ),
            )),
            // Shown below, once for each column.
            ParityIssue::NotConverged { .. } | ParityIssue::GateFailed { .. } => {}
        }
    }
    for column in &report.columns {
        if !column.converged {
            let line = Line::ParityNotConverged {
                backend: column.backend.clone(),
            };
            found.push(warning("CFD-PARITY-003", say(&line)));
        } else if column.gate != GateResult::Pass {
            found.push(warning("CFD-PARITY-003", say(&column_line(column))));
        }
        if column.preview {
            found.push(warning("CFD-BACKEND-009", say(&Line::BackendPreview)));
            if !lbm::claimed(&report.benchmark, case) {
                let value = format!("{} {} {}", column.backend, report.benchmark, case);
                found.push(warning(
                    "CFD-BACKEND-011",
                    fact(
                        "the preview backend has no gate claim for this case",
                        &value,
                    ),
                ));
            }
        }
    }
    found
}

fn table(path: &Path, benchmark: &str, name: Option<&str>) -> Reply {
    let Some(id) = ids().iter().copied().find(|known| *known == benchmark) else {
        let unknown = Mismatch {
            what: "benchmark".to_owned(),
            found: bounded(benchmark),
            expected: format!("one of {}", ids().join(", ")),
        };
        return Err(fail(not_a_benchmark(&bounded(benchmark), &[unknown])));
    };
    let chosen_benchmark = build_benchmark(id, None).map_err(|_| {
        fail(cli_problem(fact(
            "the reference data of the benchmark cannot be read",
            id,
        )))
    })?;
    let document = open(path)
        .map_err(|e| Box::new(open_refusal(TABLE, &e, path).report))?
        .document;
    verify_runs(&document.artifacts.runs)
        .map_err(|e| Box::new(refused(TABLE, problem(inconsistent(&e.to_string()))).report))?;

    let baseline_id = default_row().backend.id();
    let mut groups = choose_runs(&document.artifacts.runs, chosen_benchmark.as_ref());
    // The baseline is the newest run under the default backend, whichever image it ran in.
    let baseline_key = groups
        .iter()
        .filter(|((dialect, _), _)| dialect == baseline_id)
        .max_by_key(|(_, chosen)| chosen.index)
        .map(|(key, _)| key.clone());
    let baseline_chosen = baseline_key.and_then(|key| groups.remove(&key));

    let (baseline, baseline_gate) = match &baseline_chosen {
        Some(chosen) => recorded_column(path, chosen_benchmark.as_ref(), chosen),
        None => (
            RecordedColumn {
                backend: baseline_id.to_owned(),
                image: String::new(),
                run: String::new(),
                preview: false,
                gate_passed: false,
                converged: None,
                numbers: KeyNumbers::new(),
            },
            GateReport::failed_named(id, "there is no baseline run"),
        ),
    };
    let columns: Vec<RecordedColumn> = groups
        .values()
        .map(|chosen| recorded_column(path, chosen_benchmark.as_ref(), chosen).0)
        .collect();

    let case = name.map_or_else(
        || {
            path.file_stem()
                .map_or_else(|| "document".to_owned(), |s| bounded(&s.to_string_lossy()))
        },
        bounded,
    );
    let golden = columns
        .iter()
        .map(|c| {
            let level = if GOLDEN_COMPILE_PASSED.contains(&c.backend.as_str()) {
                Level::Pass
            } else {
                Level::NotRun
            };
            (c.backend.clone(), level)
        })
        .collect();
    let input = ParityInput {
        benchmark: id.to_owned(),
        case: case.clone(),
        baseline,
        columns,
        tolerances: gate_tolerances(id, &baseline_gate),
        golden,
    };
    let (parity, issues) = build(&input);
    let found = findings(&parity, &issues, baseline_id, &case);
    let text = render(&parity);
    let mut report = Report::from_diagnostics(TABLE, found, study_hash(&document.study));
    if let Some(file) = path.file_name() {
        report = report.with_artifact("document", file.to_string_lossy().into_owned());
    }
    Ok(report.with_repro(Repro::Parity(Box::new(ParityData { parity, text }))))
}

/// The parity table of `benchmark` over the runs the document at `document` has recorded.
///
/// The baseline is the newest recorded run of the benchmark under the default backend; every
/// other backend and image that has a recorded run of the benchmark is a column, the newest run of
/// each. `name` is the label of the case in the report; without it the document's file name
/// stands for it. The report holds the `ParityReport` and the text a person reads. A column
/// that failed its gate or is not converged is shown with `CFD-PARITY-003` and never counts, and a
/// preview backend's column carries its label. Nothing in the document or its store changes.
pub fn parity_table(document: &Path, benchmark: &str, name: Option<&str>) -> Report {
    match table(document, benchmark, name) {
        Ok(report) => report,
        Err(report) => *report,
    }
}

/// Whether the backend `backend_id` may be admitted on the evidence of `report`.
///
/// Admission needs all three levels for a column that counts (its gate passed and it is
/// converged): the backend's golden compile in [`GOLDEN_COMPILE_PASSED`], the reference oracle (the
/// gate) and parity. A level that did not pass is `CFD-PARITY-006` and names the level; a quantity
/// outside its tolerance is `CFD-PARITY-005`; a backend with no column in the report is
/// `CFD-PARITY-001`.
// The finding is the whole answer and is built once, as in the registry's own lookups.
#[allow(clippy::result_large_err)]
pub fn check_admission(report: &ParityReport, backend_id: &str) -> Result<(), Diagnostic> {
    check_admission_with(report, backend_id, GOLDEN_COMPILE_PASSED)
}

/// [`check_admission`] with the list of backends whose golden compile passed given by the caller.
#[doc(hidden)]
#[allow(clippy::result_large_err)] // the same finding as above
pub fn check_admission_with(
    report: &ParityReport,
    backend_id: &str,
    golden: &[&str],
) -> Result<(), Diagnostic> {
    let refuse = |level: &str| {
        error(
            "CFD-PARITY-006",
            fact("admission refused: a gate level did not pass", level),
        )
    };
    let columns: Vec<&ParityColumn> = report
        .columns
        .iter()
        .filter(|c| c.backend == backend_id)
        .collect();
    let Some(first) = columns.first() else {
        let line = Line::ParityNoRuns {
            benchmark: report.benchmark.clone(),
            backend: bounded(backend_id),
        };
        return Err(error("CFD-PARITY-001", say(&line)));
    };
    if !golden.contains(&backend_id) {
        return Err(refuse("golden compile"));
    }
    let counted: Vec<&ParityColumn> = columns
        .iter()
        .copied()
        .filter(|c| c.gate == GateResult::Pass && c.converged)
        .collect();
    if counted.is_empty() {
        return Err(refuse(if first.gate == GateResult::Pass {
            "convergence"
        } else {
            "reference oracle"
        }));
    }
    for column in counted {
        if let Some(row) = column.rows.iter().find(|r| r.verdict == Verdict::Differs) {
            return Err(error(
                "CFD-PARITY-005",
                fact(
                    "a quantity lies outside the parity tolerance",
                    &row.quantity,
                ),
            ));
        }
        if column.levels.parity != Level::Pass {
            return Err(refuse("parity"));
        }
    }
    Ok(())
}
