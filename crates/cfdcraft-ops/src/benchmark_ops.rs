//! Benchmarks on documents: a recorded run checked against the published reference its study
//! selects, the list of benchmarks, and a complete study made from a template.
//!
//! The check reads the stored result of a recorded run and recomputes the comparison every time
//! it is asked, so changing a tolerance never needs a new run. It changes nothing: the document
//! and the store beside it are as they were afterwards.
//!
//! Finding codes of this family: `CFD-BENCH-001` the study is not the benchmark it is checked
//! against, or there is no such benchmark (a refusal, exit class 2); `CFD-BENCH-002` the stored
//! result lacks a file the comparison needs, `CFD-BENCH-003` the coefficient file is not for lift
//! along y and `CFD-BENCH-004` too few shedding cycles (each a failed gate report, exit class 1,
//! with the code at the start of its reason). The run is chosen as the other run verbs choose it,
//! with their `CFD-REPRO-004` and `CFD-REPRO-005`.

use std::path::Path;

use cfdcraft_capture::BUNDLE_CAP;
use cfdcraft_doc::{Document, open, store_dir};
use cfdcraft_gate::{Benchmark, GateConvergence, GateReport, Mismatch};
use cfdcraft_model::{InitMode, ResolveError, ResolvedStudy, TurbulenceModel, resolve};
use cfdcraft_runs::{RunRecord, find_run, is_plain_name, verify_runs};
use cfdcraft_settle::{Verdict, VerdictState};
use cfdcraft_store::Store;
use cfdcraft_templates::{MeshPreset, TemplateError, TemplateOptions, TimeChoice};
use cfdcraft_text::Line;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::backends::{by_id, default_row};
use crate::benchmarks::{
    BundleFiles, OVERRIDE_NOTE, Rejected, build, ids, not_a_benchmark, select_id,
};
use crate::cases::cli_problem;
use crate::documents::{DocOutcome, Finding, problem, refused, say, shown, study_hash};
use crate::jobs::{comparer_files, inconsistent, open_refusal};
use crate::report::{ExitClass, Report};
use crate::repro::{BenchmarkEntry, BenchmarksData, Repro};

const CHECK: &str = "benchmark-check";

type Reply = Result<DocOutcome, Box<DocOutcome>>;

/// A word given by a person, shortened and with control characters replaced, for a message.
fn bounded(text: &str) -> String {
    text.chars()
        .take(40)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn fail(found: Finding) -> Box<DocOutcome> {
    Box::new(refused(CHECK, problem(found)))
}

/// The run `wanted` names, or the newest one that recorded a result and is not a rerun. The
/// refusals are those of the other run verbs.
fn select_run<'a>(
    runs: &'a [RunRecord],
    wanted: Option<&str>,
) -> Result<&'a RunRecord, Box<Finding>> {
    let unknown = |id: &str| Box::new(("CFD-REPRO-004", Line::UnknownRun { run: id.to_owned() }));
    let no_runs = || Box::new(("CFD-REPRO-005", Line::NoRuns));
    let no_result = |run: &RunRecord| {
        Box::new((
            "CFD-REPRO-005",
            Line::RunHasNoResult {
                run: run.id.clone(),
            },
        ))
    };
    let Some(id) = wanted else {
        let Some(last) = runs.last() else {
            return Err(no_runs());
        };
        return runs
            .iter()
            .rev()
            .find(|run| run.rerun_of.is_none() && run.result.is_some())
            .ok_or_else(|| no_result(last));
    };
    if !is_plain_name(id) {
        return Err(unknown(id));
    }
    if runs.is_empty() {
        return Err(no_runs());
    }
    let run = find_run(runs, id).ok_or_else(|| unknown(id))?;
    if run.result.is_some() {
        Ok(run)
    } else {
        Err(no_result(run))
    }
}

/// The benchmark of an id and the id itself.
type Chosen = (&'static str, Box<dyn Benchmark>);

/// The benchmark with this id, with the caller's tolerance file when one is given.
fn make(
    id: &str,
    tolerance_override: Option<&Path>,
) -> Result<Box<dyn Benchmark>, Box<DocOutcome>> {
    build(id, tolerance_override).map_err(|e| {
        let sentence = format!("The tolerance file cannot be used: {e}.");
        Box::new(refused(CHECK, cli_problem(sentence)))
    })
}

/// The refusal for a study that is none of the benchmarks: the one it comes nearest to, with
/// everything that differs from it. A benchmark of another shape comes last, because it stops at
/// that one difference; among the rest the one with the fewest differences is nearest.
fn nearest(rejected: Rejected) -> Box<DocOutcome> {
    let (id, found) = rejected
        .into_iter()
        .min_by_key(|(_, found)| (found.iter().any(|m| m.what == "shape"), found.len()))
        .unwrap_or_default();
    Box::new(refused(CHECK, not_a_benchmark(&id, &found)))
}

/// The benchmark the check uses: the one named, which the study must be, or the first the study
/// is.
fn choose(
    r: &ResolvedStudy,
    wanted: Option<&str>,
    tolerance_override: Option<&Path>,
) -> Result<Chosen, Box<DocOutcome>> {
    let Some(word) = wanted else {
        let id = select_id(r).map_err(nearest)?;
        return Ok((id, make(id, tolerance_override)?));
    };
    let Some(id) = ids().iter().copied().find(|known| *known == word) else {
        let unknown = Mismatch {
            what: "benchmark".to_owned(),
            found: bounded(word),
            expected: format!("one of {}", ids().join(", ")),
        };
        return Err(Box::new(refused(
            CHECK,
            not_a_benchmark(&bounded(word), &[unknown]),
        )));
    };
    let benchmark = make(id, tolerance_override)?;
    benchmark
        .matches(r)
        .map_err(|found| Box::new(refused(CHECK, not_a_benchmark(id, &found))))?;
    Ok((id, benchmark))
}

/// The files of the stored result of `record`. A result that is gone from the store is the
/// stored-result finding of the status verb, an error here.
fn stored_result(
    document: &Document,
    path: &Path,
    record: &RunRecord,
) -> Result<BundleFiles, Box<DocOutcome>> {
    let Some(out) = record.result.as_ref() else {
        let run = record.id.clone();
        return Err(fail(("CFD-REPRO-005", Line::RunHasNoResult { run })));
    };
    if out.size > BUNDLE_CAP {
        let reason = format!(
            "run {}: the stored result is larger than a result can be",
            record.id
        );
        return Err(fail(inconsistent(&reason)));
    }
    let gone = || {
        let line = if document.artifacts.pruned.contains(&out.blob) {
            Line::StagePruned {
                stage: "result".to_owned(),
                run: record.id.clone(),
            }
        } else {
            Line::StageBlobMissing {
                stage: "result".to_owned(),
                hash: out.blob.to_string(),
            }
        };
        fail(("CFD-REPRO-002", line))
    };
    let store = Store::open(store_dir(path)).map_err(|_| gone())?;
    let bytes = store.read(out.blob).map_err(|_| gone())?;
    Ok(BundleFiles::new(bytes))
}

/// The file a reason that starts with `CFD-BENCH-002` says is missing or unusable.
fn missing_file(reason: &str) -> String {
    let rest = reason
        .trim_start_matches("CFD-BENCH-002")
        .trim_start_matches(':')
        .trim();
    let named = match rest.strip_prefix("the results lack ") {
        Some(file) => file.split(',').next().unwrap_or(file),
        None => rest.split_whitespace().next().unwrap_or(rest),
    };
    match named.trim() {
        "" => "a result file".to_owned(),
        file => file.to_owned(),
    }
}

/// The sentence for the verdict of a gate. A gate that failed for a reason other than a missing
/// file (a refusal to judge) has no sentence of its own: the report carries the reason.
fn verdict_line(id: &str, gate: &GateReport) -> Option<Line> {
    let benchmark = id.to_owned();
    let gating = || gate.metrics.iter().filter(|m| m.gating);
    if gate.passed {
        return Some(Line::BenchmarkPassed {
            benchmark,
            metrics: gating().count().to_string(),
        });
    }
    match gate.reason.as_deref() {
        Some(reason) if reason.starts_with("CFD-BENCH-002") => {
            Some(Line::BenchmarkResultsIncomplete {
                benchmark,
                file: missing_file(reason),
            })
        }
        Some(_) => None,
        None => Some(Line::BenchmarkFailed {
            benchmark,
            failed: gating()
                .filter(|m| !m.passed)
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

/// Name a tolerance file in the gate: its own note when the benchmark made one, and the sentence.
fn name_override(gate: &mut GateReport, id: &str) {
    if !gate
        .notes
        .iter()
        .any(|note| note.starts_with("tolerance override used:"))
    {
        gate.note_override(OVERRIDE_NOTE);
    }
    let line = Line::BenchmarkOverrideUsed {
        benchmark: id.to_owned(),
    };
    gate.notes.push(say(&line));
}

/// The flag for the run a verdict was made on, from the convergence assessment recorded with the
/// run. The benchmark verdict itself is never touched: this is information beside it. A run with no
/// assessment, or one the assessment could not judge, is not called converged and its reason says
/// `unknown`.
fn convergence_flag(id: &str, assessed: Option<&Verdict>) -> GateConvergence {
    let benchmark = id.to_owned();
    let (converged, reason, line) = match assessed {
        Some(v) if v.state == VerdictState::Converged => (true, "converged".to_owned(), None),
        Some(v) if v.state == VerdictState::NotConverged => {
            let reasons = if v.reasons.is_empty() {
                "not converged".to_owned()
            } else {
                v.reasons.join(", ")
            };
            let line = Line::BenchmarkNotConverged {
                benchmark,
                reasons: reasons.clone(),
            };
            (false, reasons, Some(line))
        }
        other => {
            let cause = other.map_or_else(
                || "the run recorded no assessment".to_owned(),
                |v| format!("not judged ({})", v.reasons.join(", ")),
            );
            let reasons = format!("unknown: {cause}");
            let line = Line::BenchmarkConvergenceUnknown {
                benchmark,
                reasons: reasons.clone(),
            };
            (false, reasons, Some(line))
        }
    };
    GateConvergence {
        converged,
        reason,
        message: line.as_ref().map(say).unwrap_or_default(),
    }
}

fn check(
    path: &Path,
    run: Option<&str>,
    benchmark: Option<&str>,
    tolerance_override: Option<&Path>,
) -> Reply {
    let document = open(path)
        .map_err(|e| Box::new(open_refusal(CHECK, &e, path)))?
        .document;
    verify_runs(&document.artifacts.runs).map_err(|e| fail(inconsistent(&e.to_string())))?;
    let record = select_run(&document.artifacts.runs, run).map_err(|found| fail(*found))?;
    let resolved = resolve(&record.provenance.settings).map_err(|e| {
        let reason = format!("run {}: the recorded study cannot be used: {e}", record.id);
        fail(inconsistent(&reason))
    })?;
    let (id, chosen) = choose(&resolved, benchmark, tolerance_override)?;
    let files = stored_result(&document, path, record)?;
    // The stored files are read through the view of the backend that made the run, as the run's
    // own tool identity names it; the default row when none matches, as the live view does.
    let row = by_id(&record.provenance.tool.dialect).unwrap_or_else(default_row);
    let files = comparer_files(&row, Box::new(files), &resolved);
    let mut gate = chosen.compare(files.as_ref(), &resolved);
    if tolerance_override.is_some() {
        name_override(&mut gate, id);
    }
    gate.convergence = Some(convergence_flag(id, record.verdict.as_ref()));
    let line = verdict_line(id, &gate);
    let mut report = Report::from_diagnostics(CHECK, Vec::new(), study_hash(&document.study))
        .with_artifact("document", shown(path))
        .with_artifact("run", record.id.clone());
    if !gate.passed {
        report.set_class(ExitClass::GateFailed);
    }
    report.gate = Some(gate);
    Ok(DocOutcome { report, line })
}

/// Check the stored result of a recorded run against the benchmark its study is.
///
/// `run` names the run; without it the newest run that recorded a result and is not a rerun is
/// used. `benchmark` names the benchmark the study must be; without it the study selects its own.
/// `tolerance_override` is a tolerance file: bounded to three times the shipped values, it needs a
/// justification, and the report names it. The report holds the gate with the computed value, the
/// published range, the tolerance used and the verdict of every metric, and the exit class is
/// `GateFailed` when the gate failed. The gate also carries `convergence`: whether the recorded
/// run had converged by its own assessment (`converged`, the reason, and a sentence for a run that
/// is not converged or whose state is unknown). It is a flag and never changes the verdict or the
/// exit class. A study that is not the benchmark is refused with the
/// differences, and nothing in the document or the store changes.
pub fn benchmark_check(
    path: &Path,
    run: Option<&str>,
    benchmark: Option<&str>,
    tolerance_override: Option<&Path>,
) -> DocOutcome {
    match check(path, run, benchmark, tolerance_override) {
        Ok(outcome) => outcome,
        Err(outcome) => *outcome,
    }
}

/// What a person may choose on top of a benchmark template, as the words they typed. Everything
/// left out keeps the template's own default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkOptions {
    /// The mesh preset: `coarse`, `medium` (the default) or `fine`.
    #[serde(default)]
    pub mesh: Option<String>,
    /// The turbulence model: `laminar`, `k-omega-sst`, `realizable-ke` or `rng-k-epsilon`.
    #[serde(default)]
    pub turbulence: Option<String>,
    /// `steady` or `transient`.
    #[serde(default)]
    pub time: Option<String>,
    /// The time step at the coarse mesh, in seconds; for a transient run only.
    #[serde(default)]
    pub delta_t: Option<f64>,
    /// How the fields start: `uniform` or `potential`.
    #[serde(default)]
    pub init: Option<String>,
    /// The Rayleigh number; the heated cavity only.
    #[serde(default)]
    pub rayleigh: Option<f64>,
    /// The Reynolds number; the laminar step only.
    #[serde(default)]
    pub reynolds: Option<f64>,
}

/// `a, b or c`.
fn either(names: &[&str]) -> String {
    match names.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The value a word stands for, in any case. `Ok(None)` when no word was given.
fn word<T: Copy>(
    option: &str,
    given: Option<&str>,
    table: &[(&str, T)],
) -> Result<Option<T>, String> {
    let Some(given) = given else {
        return Ok(None);
    };
    let lower = given.to_ascii_lowercase();
    match table.iter().find(|(name, _)| *name == lower) {
        Some((_, value)) => Ok(Some(*value)),
        None => {
            let names: Vec<&str> = table.iter().map(|(name, _)| *name).collect();
            Err(format!(
                "The option {option} takes {}; {} is not one of them.",
                either(&names),
                bounded(given)
            ))
        }
    }
}

/// A number above zero, or nothing when none was given.
fn number(option: &str, given: Option<f64>) -> Result<Option<f64>, String> {
    match given {
        Some(x) if !(x.is_finite() && x > 0.0) => Err(format!(
            "The option {option} takes a number above zero; {x} is not one."
        )),
        other => Ok(other),
    }
}

/// The options of a template from the words a person typed. This is the one place the words are
/// read: the command line and the agent tool both go through the operations that use it. A
/// refusal is one sentence that names the option.
pub fn parse_options(options: &BenchmarkOptions) -> Result<TemplateOptions, String> {
    let mesh = word(
        "mesh",
        options.mesh.as_deref(),
        &[
            ("coarse", MeshPreset::Coarse),
            ("medium", MeshPreset::Medium),
            ("fine", MeshPreset::Fine),
        ],
    )?;
    let turbulence = word(
        "turbulence",
        options.turbulence.as_deref(),
        &[
            ("laminar", TurbulenceModel::Laminar),
            ("k-omega-sst", TurbulenceModel::KOmegaSst),
            ("realizable-ke", TurbulenceModel::RealizableKe),
            ("rng-k-epsilon", TurbulenceModel::RngKEpsilon),
        ],
    )?;
    let time = word(
        "time",
        options.time.as_deref(),
        &[
            ("steady", TimeChoice::Steady),
            ("transient", TimeChoice::Transient),
        ],
    )?;
    let init = word(
        "init",
        options.init.as_deref(),
        &[
            ("uniform", InitMode::Uniform),
            ("potential", InitMode::Potential),
        ],
    )?;
    Ok(TemplateOptions {
        mesh: mesh.unwrap_or_default(),
        turbulence,
        time,
        delta_t: number("delta_t", options.delta_t)?,
        init,
        rayleigh: number("rayleigh", options.rayleigh)?,
        reynolds: number("reynolds", options.reynolds)?,
    })
}

/// The sentence for a template that could not be made.
fn template_refusal(id: &str, error: &TemplateError) -> String {
    let id = bounded(id);
    match error {
        TemplateError::UnknownTemplate { .. } => {
            let known: Vec<String> = cfdcraft_templates::list()
                .into_iter()
                .map(|t| t.id)
                .collect();
            format!(
                "There is no benchmark template called {id}. The templates are {}.",
                known.join(", ")
            )
        }
        TemplateError::BadOption { option } => {
            format!("The option {option} is out of range or does not apply to the {id} template.")
        }
        TemplateError::Internal { detail } => {
            format!("The {id} template did not make a valid study: {detail}.")
        }
    }
}

/// The benchmark templates, each with the benchmark that judges a run of it. The sentences for a
/// person are `BenchmarksData::lines`.
pub fn benchmark_list() -> Report {
    let benchmarks = cfdcraft_templates::list()
        .into_iter()
        .map(|t| BenchmarkEntry {
            id: t.id,
            title: t.title,
            benchmark: t.benchmark,
            reference: t.reference,
            defaults: t.defaults,
            headline_quantities: t.headline_quantities,
        })
        .collect();
    Report::new("benchmark-list")
        .with_repro(Repro::Benchmarks(Box::new(BenchmarksData { benchmarks })))
}

/// A complete study for the template `id` with the options chosen, as the text of a study file
/// (the second member), after the model's own checks accepted it. Nothing is touched: a caller
/// that wants a file makes it. A refusal is exit class 2 with a sentence that names what is wrong
/// and no study.
pub fn benchmark_create(id: &str, options: &BenchmarkOptions) -> (Report, Option<String>) {
    const CREATE: &str = "benchmark-create";
    let refuse = |sentence: String| {
        let report = Report::from_diagnostics(CREATE, vec![cli_problem(sentence)], None);
        (report, None)
    };
    let chosen = match parse_options(options) {
        Ok(chosen) => chosen,
        Err(sentence) => return refuse(sentence),
    };
    let study = match cfdcraft_templates::build(id, &chosen) {
        Ok(study) => study,
        Err(error) => return refuse(template_refusal(id, &error)),
    };
    if let Err(ResolveError::Diagnostics(found)) = resolve(&study) {
        return (Report::from_diagnostics(CREATE, found, None), None);
    }
    let mut text = match serde_json::to_string_pretty(&study) {
        Ok(text) => text,
        Err(e) => return refuse(format!("The study could not be turned into text: {e}.")),
    };
    text.push('\n');
    let report = Report::from_diagnostics(CREATE, Vec::new(), study_hash(&study));
    (report, Some(text))
}

/// The sentence that says a study was made, with `path` as the place it went (`-` for a study
/// that was returned as text).
pub fn created_line(id: &str, options: &BenchmarkOptions, path: &str) -> Line {
    let mesh = options
        .mesh
        .as_deref()
        .map_or_else(|| "medium".to_owned(), str::to_ascii_lowercase);
    Line::BenchmarkCreated {
        benchmark: bounded(id),
        mesh,
        path: path.to_owned(),
    }
}
