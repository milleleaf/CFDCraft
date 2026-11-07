//! Reading what a document holds: which stages are up to date and why not, which runs there are,
//! what each produced and was made from, and how two runs compare.
//!
//! None of these operations writes. The file and the store beside it are exactly as they were
//! afterwards. Every answer is the usual [`Report`] with the data in its `repro` field, and every
//! sentence is rendered from a text crate line.
//!
//! Finding codes of this family: 001 a stage is out of date or never made (a warning); 002 a
//! recorded stage output is missing from the store (a warning); 003 the run records disagree with
//! each other; 004 there is no such run; 005 the file has no runs, or the run recorded no result;
//! 007 two runs differ beyond the tolerance (exit class 1); 010 two runs are within the tolerance
//! but not identical (a warning). Staleness never changes the exit class.

use std::path::Path;

use cfdcraft_backend_esi::Dialect;
use cfdcraft_compare::{Comparison, Tolerance, Verdict, compare};
use cfdcraft_doc::{Document, open, store_dir};
use cfdcraft_keys::{Stage, StageKey, StageRecipes};
use cfdcraft_model::{Diagnostic, Severity};
use cfdcraft_runs::{
    RunRecord, StageState, find_run, latest_mesh, latest_result, status, verify_runs,
};
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_text::Line;

use crate::documents::{DocOutcome, Finding, finding, problem, refused, say, shown, study_hash};
use crate::jobs::{inconsistent, open_refusal};
use crate::keys::tool_identity;
use crate::prepare::prepare;
use crate::reasons::{reason_parts, stale_line};
use crate::report::{ExitClass, Report};
use crate::repro::{
    ComparisonData, ProvenanceData, Repro, ResultData, RunSummary, RunsData, StageStatus,
    StatusData,
};

/// A document that was opened and whose run records hold together.
struct Loaded<'a> {
    operation: &'static str,
    path: &'a Path,
    document: Document,
}

impl<'a> Loaded<'a> {
    fn load(operation: &'static str, path: &'a Path) -> Result<Self, Box<DocOutcome>> {
        let opened = open(path).map_err(|e| Box::new(open_refusal(operation, &e, path)))?;
        if let Err(e) = verify_runs(&opened.document.artifacts.runs) {
            let found = inconsistent(&e.to_string());
            return Err(Box::new(refused(operation, problem(found))));
        }
        Ok(Self {
            operation,
            path,
            document: opened.document,
        })
    }

    fn runs(&self) -> &[RunRecord] {
        &self.document.artifacts.runs
    }

    /// A report for this document with these findings.
    fn report(&self, diagnostics: Vec<Diagnostic>) -> Report {
        Report::from_diagnostics(
            self.operation,
            diagnostics,
            study_hash(&self.document.study),
        )
        .with_artifact("document", shown(self.path))
    }

    fn failure(&self, found: Finding) -> Box<DocOutcome> {
        Box::new(DocOutcome {
            report: self.report(vec![problem(found)]),
            line: None,
        })
    }

    fn no_runs(&self) -> Box<DocOutcome> {
        self.failure(("CFD-REPRO-005", Line::NoRuns))
    }

    /// The run called `id`. A name that is not a plain name is never looked up.
    fn find(&self, id: &str) -> Result<&RunRecord, Box<DocOutcome>> {
        let unknown = || self.failure(("CFD-REPRO-004", Line::UnknownRun { run: id.to_owned() }));
        if !is_plain_name(id) {
            return Err(unknown());
        }
        if self.runs().is_empty() {
            return Err(self.no_runs());
        }
        find_run(self.runs(), id).ok_or_else(unknown)
    }

    /// `run` itself when it recorded a result.
    fn with_result<'r>(&self, run: &'r RunRecord) -> Result<&'r RunRecord, Box<DocOutcome>> {
        if run.result.is_some() {
            Ok(run)
        } else {
            Err(self.failure((
                "CFD-REPRO-005",
                Line::RunHasNoResult {
                    run: run.id.clone(),
                },
            )))
        }
    }

    /// The run `wanted` names, or the newest one: with a result and not a rerun when
    /// `want_result` is set, of any kind otherwise.
    fn select(
        &self,
        wanted: Option<&str>,
        want_result: bool,
    ) -> Result<&RunRecord, Box<DocOutcome>> {
        if let Some(id) = wanted {
            let run = self.find(id)?;
            return if want_result {
                self.with_result(run)
            } else {
                Ok(run)
            };
        }
        let Some(last) = self.runs().last() else {
            return Err(self.no_runs());
        };
        if !want_result {
            return Ok(last);
        }
        self.runs()
            .iter()
            .rev()
            .find(|run| run.rerun_of.is_none() && run.result.is_some())
            .ok_or_else(|| {
                self.failure((
                    "CFD-REPRO-005",
                    Line::RunHasNoResult {
                        run: last.id.clone(),
                    },
                ))
            })
    }
}

/// The same rule run ids follow: 1 to 40 lower-case letters, digits or hyphens, not starting
/// with a hyphen.
pub(crate) fn is_plain_name(id: &str) -> bool {
    let bytes = id.as_bytes();
    (1..=40).contains(&bytes.len())
        && bytes
            .iter()
            .enumerate()
            .all(|(at, &c)| c.is_ascii_lowercase() || c.is_ascii_digit() || (at > 0 && c == b'-'))
}

fn blank_stage(key: StageKey, state: &str) -> StageStatus {
    StageStatus {
        key: key.to_string(),
        state: state.to_owned(),
        run: None,
        since: None,
        sentence: String::new(),
        reasons: Vec::new(),
        changes: Vec::new(),
    }
}

/// Where one stage stands, and the warning that goes with it when it is not up to date.
fn stage_status(
    stage: Stage,
    key: StageKey,
    state: &StageState,
    blob: Option<ContentHash>,
    present: &dyn Fn(&ContentHash) -> bool,
    found: &mut Vec<Diagnostic>,
) -> StageStatus {
    let word = stage.as_str();
    match state {
        StageState::Fresh { run } => {
            let gone = blob.filter(|hash| !present(hash));
            let mut out = blank_stage(
                key,
                if gone.is_some() {
                    "blob_missing"
                } else {
                    "fresh"
                },
            );
            out.run = Some(run.clone());
            match gone {
                Some(hash) => {
                    let line = Line::StageBlobMissing {
                        stage: word.to_owned(),
                        hash: hash.to_string(),
                    };
                    out.sentence = say(&line);
                    found.push(finding(
                        Severity::Warning,
                        "stored result",
                        ("CFD-REPRO-002", line),
                    ));
                }
                None => {
                    out.sentence = say(&Line::StageUpToDate {
                        stage: word.to_owned(),
                        run: run.clone(),
                    });
                }
            }
            out
        }
        StageState::Stale { since, changes } => {
            let line = stale_line(stage, changes);
            let mut out = blank_stage(key, "stale");
            out.since = Some(since.clone());
            out.sentence = say(&line);
            out.reasons = reason_parts(changes);
            out.changes = changes.clone();
            found.push(finding(Severity::Warning, word, ("CFD-REPRO-001", line)));
            out
        }
        StageState::NeverMade => {
            let line = Line::StageNeverMade {
                stage: word.to_owned(),
            };
            let mut out = blank_stage(key, "never_made");
            out.sentence = say(&line);
            found.push(finding(Severity::Warning, word, ("CFD-REPRO-001", line)));
            out
        }
    }
}

/// Whether the mesh and the result stored in the document at `path` are up to date for the study
/// it holds now, and for each one that is not, why.
///
/// The answer is a passing report whatever the state. A stage that was never made, or whose
/// recorded inputs differ from the study, or whose stored data is gone, is a warning.
pub fn document_status(path: &Path) -> DocOutcome {
    const OP: &str = "status";
    let loaded = match Loaded::load(OP, path) {
        Ok(loaded) => loaded,
        Err(outcome) => return *outcome,
    };
    let document = &loaded.document;
    let mut study_json =
        serde_json::to_string_pretty(&document.study).expect("a study is plain data");
    study_json.push('\n');
    let dialect = Dialect::EsiV2512;
    let prepared = match prepare(&study_json, dialect) {
        Ok(prepared) => prepared,
        Err(report) => {
            return DocOutcome {
                report: report.for_operation(OP),
                line: None,
            };
        }
    };
    let recipes = StageRecipes::from_resolved(&prepared.resolved, &tool_identity(dialect));
    let keys = recipes.keys();
    let runs = loaded.runs();
    let state = status(runs, &recipes);
    let store = Store::open(store_dir(path)).ok();
    let present = |hash: &ContentHash| store.as_ref().is_some_and(|s| s.contains(hash));

    let mut found = prepared.warnings;
    let mesh = stage_status(
        Stage::Mesh,
        keys.mesh,
        &state.mesh,
        latest_mesh(runs, keys.mesh).map(|(_, out)| out.blob),
        &present,
        &mut found,
    );
    let result = stage_status(
        Stage::Result,
        keys.result,
        &state.result,
        latest_result(runs, keys.result).map(|(_, out)| out.blob),
        &present,
        &mut found,
    );
    let all_fresh = mesh.state == "fresh" && result.state == "fresh";
    let report = Report::from_diagnostics(OP, found, Some(prepared.hash))
        .with_artifact("document", shown(path))
        .with_repro(Repro::Status(Box::new(StatusData { mesh, result })));
    DocOutcome {
        report,
        line: all_fresh.then_some(Line::NothingToUpdate),
    }
}

fn summary(run: &RunRecord) -> RunSummary {
    let recorded = [
        (Stage::Mesh, run.mesh.is_some()),
        (Stage::Result, run.result.is_some()),
    ]
    .into_iter()
    .filter(|(_, held)| *held)
    .map(|(stage, _)| stage.as_str().to_owned())
    .collect();
    RunSummary {
        id: run.id.clone(),
        rerun_of: run.rerun_of.clone(),
        recorded,
        mesh_key: run.mesh.as_ref().map(|out| out.key.to_string()),
        result_key: run.result.as_ref().map(|out| out.key.to_string()),
        host: run.provenance.host.clone(),
        checks: run.checks.clone(),
        verdict: run
            .comparison
            .as_ref()
            .map(|comparison| comparison.verdict.as_str().to_owned()),
    }
}

/// The runs recorded in the document at `path`, oldest first, with what each recorded.
pub fn list_runs(path: &Path) -> DocOutcome {
    let loaded = match Loaded::load("runs", path) {
        Ok(loaded) => loaded,
        Err(outcome) => return *outcome,
    };
    let runs: Vec<RunSummary> = loaded.runs().iter().map(summary).collect();
    let line = runs.is_empty().then_some(Line::NoRuns);
    DocOutcome {
        report: loaded
            .report(Vec::new())
            .with_repro(Repro::Runs(RunsData { runs })),
        line,
    }
}

/// The checks and key numbers of a run: the one named, or the newest that recorded a result and
/// is not a rerun.
pub fn run_result(path: &Path, run: Option<&str>) -> DocOutcome {
    let loaded = match Loaded::load("result", path) {
        Ok(loaded) => loaded,
        Err(outcome) => return *outcome,
    };
    let record = match loaded.select(run, true) {
        Ok(record) => record,
        Err(outcome) => return *outcome,
    };
    let data = ResultData {
        run: record.id.clone(),
        result_key: record.result.as_ref().map(|out| out.key.to_string()),
        checks: record.checks.clone(),
        key_numbers: record.key_numbers.clone(),
        comparison: record.comparison.clone(),
    };
    DocOutcome {
        report: loaded
            .report(Vec::new())
            .with_repro(Repro::RunResult(Box::new(data))),
        line: None,
    }
}

/// What a run was made from: the one named, or the newest of any kind.
pub fn run_provenance(path: &Path, run: Option<&str>) -> DocOutcome {
    let loaded = match Loaded::load("provenance", path) {
        Ok(loaded) => loaded,
        Err(outcome) => return *outcome,
    };
    let record = match loaded.select(run, false) {
        Ok(record) => record,
        Err(outcome) => return *outcome,
    };
    let data = ProvenanceData {
        run: record.id.clone(),
        provenance: record.provenance.clone(),
    };
    DocOutcome {
        report: loaded
            .report(Vec::new())
            .with_repro(Repro::Provenance(Box::new(data))),
        line: None,
    }
}

/// The findings, the sentence and whether the gate failed for the comparison of run `run` with
/// run `of`. Identical numbers are `reproduced`; numbers that all lie within the tolerance pass
/// with a warning; anything outside it is exit class 1.
pub(crate) fn judgement(
    comparison: &Comparison,
    run: &str,
    of: &str,
) -> (Vec<Diagnostic>, Line, bool) {
    let (run, of) = (run.to_owned(), of.to_owned());
    match comparison.verdict {
        Verdict::Reproduced => (Vec::new(), Line::RerunReproduced { run, of }, false),
        Verdict::WithinTolerance => {
            let line = Line::RerunWithin {
                run,
                of,
                max_diff: comparison.max_abs_diff.to_string(),
            };
            let found = finding(
                Severity::Warning,
                "comparison",
                ("CFD-REPRO-010", line.clone()),
            );
            (vec![found], line, false)
        }
        Verdict::Differs => {
            let count = comparison.differing.iter().filter(|d| !d.within).count();
            let line = Line::RerunDiffers {
                run,
                of,
                count: count as u64,
            };
            let found = finding(
                Severity::Error,
                "comparison",
                ("CFD-REPRO-007", line.clone()),
            );
            (vec![found], line, true)
        }
    }
}

/// Compare the key numbers of run `other` with those of run `original` under `tolerance`.
///
/// Identical numbers are `reproduced`. Numbers that all lie within the tolerance pass with a
/// warning. Any quantity outside it, or present on one side only, is exit class 1. Both runs must
/// exist and have recorded a result.
pub fn compare_runs(path: &Path, original: &str, other: &str, tolerance: Tolerance) -> DocOutcome {
    let loaded = match Loaded::load("compare", path) {
        Ok(loaded) => loaded,
        Err(outcome) => return *outcome,
    };
    let both = loaded
        .find(original)
        .and_then(|run| loaded.with_result(run))
        .and_then(|first| {
            loaded
                .find(other)
                .and_then(|run| loaded.with_result(run))
                .map(|second| (first, second))
        });
    let (first, second) = match both {
        Ok(pair) => pair,
        Err(outcome) => return *outcome,
    };
    let comparison = compare(&first.key_numbers, &second.key_numbers, tolerance);
    let (found, line, failed) = judgement(&comparison, other, original);
    let line = (!failed).then_some(line);
    let data = ComparisonData {
        original: original.to_owned(),
        other: other.to_owned(),
        comparison,
    };
    let mut report = loaded
        .report(found)
        .with_repro(Repro::Comparison(Box::new(data)));
    if failed {
        report.set_class(ExitClass::GateFailed);
    }
    DocOutcome { report, line }
}
