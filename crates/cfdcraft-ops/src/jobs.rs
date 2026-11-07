//! Solving a document: run its case once, put the mesh and the result into the store beside the
//! file, and record the run in the file, so the next identical request runs nothing.
//!
//! The work is split in three. [`plan_job`] decides what is out of date and does no work beyond
//! reading the file. A [`CaseRunner`] runs the case and hands back the folder it left; the real
//! one lives in the host runner module and tests use a fixture. [`DocJob::finish`] captures that
//! folder, stores the blobs and appends the run. Nothing is recorded unless every check held and
//! every file could be read; a failure of any kind leaves the document as it was.
//!
//! Finding codes of this family: 002 a recorded blob is missing from the store (a warning; the
//! stage counts as out of date); 003 the run records of the file are inconsistent; 009 the output
//! of a finished run could not be recorded. The document codes of the document operations apply
//! to the file itself.

use std::path::{Path, PathBuf};

use cfdcraft_backend_esi::Dialect;
use cfdcraft_capture::{key_numbers, mesh_bundle, result_bundle, store_bundle};
use cfdcraft_compare::{Comparison, KeyNumbers, Tolerance, compare};
use cfdcraft_doc::{ARTIFACTS_MEMBER, DocError, Document, Opened, open, save, store_dir};
use cfdcraft_keys::{Stage, StageKeys, StageRecipes, ToolIdentity, diff_mesh};
use cfdcraft_model::{Diagnostic, MigrationNote, Severity};
use cfdcraft_runs::{
    Check, Item, Output, Provenance, RunRecord, StageState, Status, find_run, latest_mesh,
    latest_result, next_run_id, pending, status, verify_runs,
};
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_text::Line;

use crate::documents::{
    DocOutcome, Finding, finding, is_pack_name, not_written, pack_kept, problem, read_failure,
    refused, say, shown,
};
use crate::inspect::{is_plain_name, judgement};
use crate::keys::tool_identity;
use crate::prepare::{Prepared, prepare};
use crate::reasons::reason_text;
use crate::report::{ExitClass, Report};
use crate::repro::{ComparisonData, Repro, RunData};

const SOLVE: &str = "solve";

/// The code of an update that is refused.
const REFUSED: &str = "CFD-REPRO-008";

/// What a runner hands back for a case it ran: the folder the run left its output in, the checks
/// the run was judged by, and a label for the machine it ran on.
#[derive(Debug, Clone)]
pub struct CaseRun {
    /// The folder holding the mesh files, the final fields, the samples and the logs.
    pub results_dir: PathBuf,
    /// The checks the run was held to. A check that did not pass stops the run being recorded.
    pub checks: Vec<Check>,
    /// A label for the machine the case ran on, if it has one.
    pub host: Option<String>,
}

/// Something that can run the case of a study.
pub trait CaseRunner {
    /// Run the case of the study written as `study_json`. A failure is the finished, classified
    /// report of why, and nothing is recorded for it.
    fn run_case(&mut self, study_json: &str) -> Result<CaseRun, Box<Report>>;
}

/// What a job does with a document.
#[derive(Debug, Clone, PartialEq)]
pub enum JobAction {
    /// Make whatever mesh and result is out of date, and record the run.
    Solve,
    /// Make this one stage, which must be out of date, and record a run holding only it.
    Item(Stage),
    /// Run the settings a recorded run was made from, and record the new run with its comparison
    /// against that run.
    Rerun {
        /// The run to repeat.
        run: String,
        /// How far the key numbers may differ and still count as the same.
        tolerance: Tolerance,
        /// Whether to run with this build's tools when the run was made with others.
        allow_tool_change: bool,
    },
}

/// The name of the operation a job reports as.
pub(crate) fn operation_of(action: &JobAction) -> &'static str {
    match action {
        JobAction::Solve => SOLVE,
        JobAction::Item(_) => "update",
        JobAction::Rerun { .. } => "rerun",
    }
}

/// What an item of the document at `path` is called: the file stem of its path.
pub(crate) fn doc_label(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Which stages a job has to make.
#[derive(Debug, Clone, Copy)]
struct Pending {
    mesh: bool,
    result: bool,
}

/// The run a rerun repeats and the tolerance it is compared under.
#[derive(Debug)]
struct Repeat {
    original: RunRecord,
    tolerance: Tolerance,
}

/// A document that has been looked at and has work to do.
#[derive(Debug)]
pub struct DocJob {
    path: PathBuf,
    action: JobAction,
    study_json: String,
    hash: String,
    end_time: f64,
    recipes: StageRecipes,
    /// The document as it was when the job was planned.
    document: Document,
    pending: Pending,
    warnings: Vec<Diagnostic>,
    repeat: Option<Repeat>,
}

fn stage_words(mesh: bool, result: bool) -> Vec<String> {
    [(Stage::Mesh, mesh), (Stage::Result, result)]
        .into_iter()
        .filter(|(_, wanted)| *wanted)
        .map(|(stage, _)| stage.as_str().to_owned())
        .collect()
}

fn run_data(run: Option<String>, pending: Pending, keys: &StageKeys) -> RunData {
    RunData {
        run,
        recorded: stage_words(pending.mesh, pending.result),
        reused: stage_words(!pending.mesh, !pending.result),
        mesh_key: keys.mesh.to_string(),
        result_key: keys.result.to_string(),
    }
}

/// The finished report of a run whose output was not recorded: class 1, one error finding.
pub(crate) fn not_recorded_report(
    mut diagnostics: Vec<Diagnostic>,
    hash: Option<String>,
    reason: &str,
) -> Report {
    diagnostics.push(problem((
        "CFD-REPRO-009",
        Line::OutputNotRecorded {
            reason: reason.to_owned(),
        },
    )));
    let mut report = Report::from_diagnostics(SOLVE, diagnostics, hash);
    report.set_class(ExitClass::GateFailed);
    report
}

pub(crate) fn inconsistent(reason: &str) -> Finding {
    // The messages of the run log checks start with the run they are about.
    let (run, why) = reason
        .strip_prefix("run ")
        .and_then(|rest| rest.split_once(": "))
        .unwrap_or(("the run list", reason));
    (
        "CFD-REPRO-003",
        Line::RecordsInconsistent {
            run: run.to_owned(),
            reason: why.to_owned(),
        },
    )
}

/// Whether a reason given for a bad artifact index is about the run history.
fn is_run_log_reason(reason: &str) -> bool {
    reason.starts_with("run ")
        || reason.starts_with("the run id")
        || reason.contains(" run records")
}

/// A file that cannot be opened, as the refusal a solve answers with. A run history that does not
/// hold together is told apart from a file that is not a study file.
pub(crate) fn open_refusal(operation: &str, error: &DocError, path: &Path) -> DocOutcome {
    match error {
        DocError::BadJson { member, reason }
            if member.as_str() == ARTIFACTS_MEMBER && is_run_log_reason(reason) =>
        {
            refused(operation, problem(inconsistent(reason)))
        }
        other => refused(operation, problem(read_failure(other, path))),
    }
}

/// Whether a stage has to be made. A fresh stage whose stored blob is gone counts as out of date,
/// and the warning that says so is added.
fn stage_pending(
    stage: Stage,
    state: &StageState,
    blob: Option<ContentHash>,
    present: &dyn Fn(&ContentHash) -> bool,
    warnings: &mut Vec<Diagnostic>,
) -> bool {
    if !matches!(state, StageState::Fresh { .. }) {
        return true;
    }
    match blob {
        Some(hash) if present(&hash) => false,
        Some(hash) => {
            warnings.push(finding(
                Severity::Warning,
                "stored result",
                (
                    "CFD-REPRO-002",
                    Line::StageBlobMissing {
                        stage: stage.as_str().to_owned(),
                        hash: hash.to_string(),
                    },
                ),
            ));
            true
        }
        None => true,
    }
}

/// One note for the steps an older file went through, worded for a file that is saved back.
fn saved_migration(notes: &[MigrationNote]) -> Option<MigrationNote> {
    let (first, last) = (notes.first()?, notes.last()?);
    let summary = notes
        .iter()
        .map(|note| note.summary.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let (from, to) = (first.from, last.to);
    Some(MigrationNote {
        from,
        to,
        summary: say(&Line::MigratedAndSaved { from, to, summary }),
    })
}

/// A document that was read, its study prepared, and its stages compared with the records.
struct Looked {
    document: Document,
    study_json: String,
    prepared: Prepared,
    recipes: StageRecipes,
    state: Status,
}

/// Open the document at `path`, prepare its study and say where each stage stands. The file is
/// not changed.
fn look(operation: &str, path: &Path, tool: &ToolIdentity) -> Result<Looked, Box<DocOutcome>> {
    if is_pack_name(path) {
        return Err(Box::new(refused(operation, problem(pack_kept(path)))));
    }
    let opened = open(path).map_err(|e| Box::new(open_refusal(operation, &e, path)))?;
    if opened.packed_entries > 0 {
        return Err(Box::new(refused(operation, problem(pack_kept(path)))));
    }
    let document = opened.document;
    let mut study_json =
        serde_json::to_string_pretty(&document.study).expect("a study is plain data");
    study_json.push('\n');

    let prepared = prepare(&study_json, Dialect::EsiV2512).map_err(|report| {
        Box::new(DocOutcome {
            report: report.for_operation(operation),
            line: None,
        })
    })?;
    let recipes = StageRecipes::from_resolved(&prepared.resolved, tool);
    let runs = &document.artifacts.runs;
    if let Err(e) = verify_runs(runs) {
        return Err(Box::new(refused(
            operation,
            problem(inconsistent(&e.to_string())),
        )));
    }
    let state = status(runs, &recipes);
    Ok(Looked {
        document,
        study_json,
        prepared,
        recipes,
        state,
    })
}

/// The stages of the document at `path` that are out of date, as items called `label`, the mesh
/// first. The file is not changed.
pub(crate) fn pending_items(
    operation: &str,
    path: &Path,
    label: &str,
) -> Result<Vec<Item>, Box<DocOutcome>> {
    let looked = look(operation, path, &tool_identity(Dialect::EsiV2512))?;
    Ok(pending(label, &looked.state))
}

/// Look at the document at `path` and decide what `action` has to make.
///
/// An answer that is already final comes back as the error: a refusal (a file that cannot be
/// read, a pack, a study that fails the checks, run records that disagree, an item that is not
/// waiting, a run that cannot be repeated), or the report that nothing is out of date. The
/// document is never changed here.
pub fn plan_job(path: &Path, action: JobAction) -> Result<DocJob, Box<DocOutcome>> {
    plan_job_with_tool(path, action, tool_identity(Dialect::EsiV2512))
}

/// [`plan_job`] for another set of tools than this build's. It exists so tests can make a
/// document with tools other than the ones in use; no front end calls it.
#[doc(hidden)]
pub fn plan_job_with_tool(
    path: &Path,
    action: JobAction,
    tool: ToolIdentity,
) -> Result<DocJob, Box<DocOutcome>> {
    if let JobAction::Rerun {
        run,
        tolerance,
        allow_tool_change,
    } = &action
    {
        return plan_rerun(path, run, *tolerance, *allow_tool_change, &tool);
    }
    plan_make(path, action, &tool)
}

/// Plan a solve, or the update of one stage.
fn plan_make(
    path: &Path,
    action: JobAction,
    tool: &ToolIdentity,
) -> Result<DocJob, Box<DocOutcome>> {
    let operation = operation_of(&action);
    let looked = look(operation, path, tool)?;
    let Looked {
        document,
        study_json,
        prepared,
        recipes,
        state,
    } = looked;
    let Prepared {
        resolved,
        hash,
        mut warnings,
        ..
    } = prepared;
    let runs = &document.artifacts.runs;

    let keys = recipes.keys();
    let store = Store::open(store_dir(path)).ok();
    let present = |hash: &ContentHash| store.as_ref().is_some_and(|s| s.contains(hash));
    let found = Pending {
        mesh: stage_pending(
            Stage::Mesh,
            &state.mesh,
            latest_mesh(runs, keys.mesh).map(|(_, out)| out.blob),
            &present,
            &mut warnings,
        ),
        result: stage_pending(
            Stage::Result,
            &state.result,
            latest_result(runs, keys.result).map(|(_, out)| out.blob),
            &present,
            &mut warnings,
        ),
    };

    let wanted = if let JobAction::Item(stage) = &action {
        let stage = *stage;
        let name = |stage: Stage| format!("{}:{}", doc_label(path), stage.as_str());
        let waiting = match stage {
            Stage::Mesh => found.mesh,
            Stage::Result => found.result,
        };
        let refusal = if !waiting {
            Some(Line::NotPending { item: name(stage) })
        } else if stage == Stage::Result && found.mesh {
            Some(Line::NeedsUpstream {
                item: name(Stage::Result),
                needs: name(Stage::Mesh),
            })
        } else {
            None
        };
        if let Some(line) = refusal {
            return Err(Box::new(refused(operation, problem((REFUSED, line)))));
        }
        Pending {
            mesh: stage == Stage::Mesh,
            result: stage == Stage::Result,
        }
    } else {
        if !found.mesh && !found.result {
            let report = Report::from_diagnostics(operation, warnings, Some(hash))
                .with_artifact("document", shown(path))
                .with_repro(Repro::Run(run_data(None, found, &keys)));
            return Err(Box::new(DocOutcome {
                report,
                line: Some(Line::NothingToRun),
            }));
        }
        found
    };

    Ok(DocJob {
        path: path.to_owned(),
        action,
        study_json,
        hash,
        end_time: resolved.solver.end_time,
        recipes,
        document,
        pending: wanted,
        warnings,
        repeat: None,
    })
}

/// Plan the rerun of the run called `wanted`. Only what the run recorded is used: the settings it
/// was made from, never the study as the document holds it now.
fn plan_rerun(
    path: &Path,
    wanted: &str,
    tolerance: Tolerance,
    allow_tool_change: bool,
    tool: &ToolIdentity,
) -> Result<DocJob, Box<DocOutcome>> {
    const OP: &str = "rerun";
    let fail = |found: Finding| Box::new(refused(OP, problem(found)));
    if is_pack_name(path) {
        return Err(fail(pack_kept(path)));
    }
    let opened = open(path).map_err(|e| Box::new(open_refusal(OP, &e, path)))?;
    if opened.packed_entries > 0 {
        return Err(fail(pack_kept(path)));
    }
    let document = opened.document;
    let runs = &document.artifacts.runs;
    if let Err(e) = verify_runs(runs) {
        return Err(fail(inconsistent(&e.to_string())));
    }
    let unknown = || {
        fail((
            "CFD-REPRO-004",
            Line::UnknownRun {
                run: wanted.to_owned(),
            },
        ))
    };
    if !is_plain_name(wanted) {
        return Err(unknown());
    }
    if runs.is_empty() {
        return Err(fail(("CFD-REPRO-005", Line::NoRuns)));
    }
    let Some(original) = find_run(runs, wanted) else {
        return Err(unknown());
    };
    let Some(recorded_result) = original.result.as_ref() else {
        return Err(fail((
            "CFD-REPRO-005",
            Line::RunHasNoResult {
                run: original.id.clone(),
            },
        )));
    };

    let study_json =
        serde_json::to_string_pretty(&original.provenance.settings).expect("a study is plain data");
    let prepared = prepare(&study_json, Dialect::EsiV2512).map_err(|report| {
        Box::new(DocOutcome {
            report: report.for_operation(OP),
            line: None,
        })
    })?;
    let recorded = StageRecipes::from_resolved(&prepared.resolved, &original.provenance.tool);
    let recipes = StageRecipes::from_resolved(&prepared.resolved, tool);
    // The same settings under the two sets of tools differ in the tools alone.
    let changes = diff_mesh(&recorded.mesh, &recipes.mesh);
    let mut warnings = prepared.warnings;
    if !changes.is_empty() {
        let line = Line::ToolMismatch {
            reasons: reason_text(&changes),
        };
        if !allow_tool_change {
            return Err(fail(("CFD-REPRO-006", line)));
        }
        warnings.push(finding(Severity::Warning, "tool", ("CFD-REPRO-006", line)));
    }
    let keys = recorded.keys();
    if keys.mesh != original.provenance.mesh_key || keys.result != recorded_result.key {
        let reason = format!(
            "run {}: the recorded settings do not make the recorded keys",
            original.id
        );
        return Err(fail(inconsistent(&reason)));
    }

    let repeat = Repeat {
        original: original.clone(),
        tolerance,
    };
    Ok(DocJob {
        path: path.to_owned(),
        action: JobAction::Rerun {
            run: wanted.to_owned(),
            tolerance,
            allow_tool_change,
        },
        study_json,
        hash: prepared.hash,
        end_time: prepared.resolved.solver.end_time,
        recipes,
        document,
        pending: Pending {
            mesh: false,
            result: true,
        },
        warnings,
        repeat: Some(repeat),
    })
}

impl DocJob {
    /// The name of the operation this job reports as: `solve`, `update` or `rerun`.
    pub(crate) fn operation(&self) -> &'static str {
        operation_of(&self.action)
    }

    /// The study the case is made from, as pretty JSON.
    pub fn study_json(&self) -> &str {
        &self.study_json
    }

    /// Record what the runner made. Nothing is recorded unless every check held and the whole
    /// output could be read and stored; the document on disk is read again first, so a link or a
    /// picture changed while the case ran is kept, and a study changed meanwhile is not overwritten.
    pub fn finish(mut self, run: CaseRun) -> DocOutcome {
        match self.repeat.take() {
            Some(repeat) => self.finish_rerun(run, &repeat),
            None => self.finish_make(run),
        }
    }

    /// Run the case with `runner`, then [`DocJob::finish`].
    pub fn run_with(self, runner: &mut dyn CaseRunner) -> DocOutcome {
        match runner.run_case(&self.study_json) {
            Ok(run) => self.finish(run),
            Err(report) => DocOutcome {
                report: report.for_operation(operation_of(&self.action)),
                line: None,
            },
        }
    }

    fn not_recorded(&self, reason: &str) -> DocOutcome {
        let report = not_recorded_report(self.warnings.clone(), Some(self.hash.clone()), reason);
        DocOutcome {
            report: report.for_operation(operation_of(&self.action)),
            line: None,
        }
    }

    /// Read the document again and check that it is what was planned and that every check of the
    /// run held.
    fn reopen(&self, run: &CaseRun) -> Result<Opened, Box<DocOutcome>> {
        let operation = operation_of(&self.action);
        let opened =
            open(&self.path).map_err(|e| Box::new(open_refusal(operation, &e, &self.path)))?;
        let moved = opened.packed_entries > 0
            || opened.document.study != self.document.study
            || opened.document.artifacts.runs.len() != self.document.artifacts.runs.len();
        if moved {
            let reason = format!("the document changed while the case ran; {operation} it again");
            return Err(Box::new(self.not_recorded(&reason)));
        }
        if let Some(check) = run.checks.iter().find(|check| !check.passed) {
            let reason = format!("the check \"{}\" did not pass", check.name);
            return Err(Box::new(self.not_recorded(&reason)));
        }
        Ok(opened)
    }

    /// Capture the output of the run, store it, and build the record of the run.
    fn record(&self, run: CaseRun, runs: &[RunRecord]) -> Result<RunRecord, String> {
        let keys = self.recipes.keys();
        let dir = run.results_dir.as_path();
        let mesh = self
            .pending
            .mesh
            .then(|| mesh_bundle(dir))
            .transpose()
            .map_err(|e| e.to_string())?;
        let result = self
            .pending
            .result
            .then(|| result_bundle(dir, self.end_time))
            .transpose()
            .map_err(|e| e.to_string())?;
        let numbers = if self.pending.result {
            key_numbers(dir, self.end_time).map_err(|e| e.to_string())?
        } else {
            KeyNumbers::new()
        };

        let store = Store::create(store_dir(&self.path)).map_err(|e| e.to_string())?;
        let mesh_blob = mesh
            .map(|bundle| store_bundle(&store, &bundle))
            .transpose()
            .map_err(|e| e.to_string())?;
        let result_blob = result
            .map(|bundle| store_bundle(&store, &bundle))
            .transpose()
            .map_err(|e| e.to_string())?;
        let mesh_hash = match mesh_blob {
            Some((hash, _)) => hash,
            None => latest_mesh(runs, keys.mesh)
                .map(|(_, out)| out.blob)
                .ok_or("the mesh this run stands on is not in the run records")?,
        };

        Ok(RunRecord {
            id: next_run_id(runs),
            rerun_of: None,
            mesh: mesh_blob.map(|(blob, size)| Output {
                key: keys.mesh,
                recipe: self.recipes.mesh.clone(),
                blob,
                size,
            }),
            result: result_blob.map(|(blob, size)| Output {
                key: keys.result,
                recipe: self.recipes.result.clone(),
                blob,
                size,
            }),
            provenance: Provenance {
                geometry_hash: self.recipes.mesh.geometry.hash(),
                mesh_key: keys.mesh,
                mesh_hash,
                result_key: result_blob.map(|_| keys.result),
                settings: self.document.study.clone(),
                tool: self.recipes.mesh.tool.clone(),
                host: run.host,
            },
            checks: run.checks,
            key_numbers: numbers,
            comparison: None,
        })
    }

    /// Capture the result of a rerun, store it, and build its record with the comparison against
    /// the run it repeats. The settings and the mesh are the original's.
    fn record_rerun(
        &self,
        run: CaseRun,
        runs: &[RunRecord],
        repeat: &Repeat,
    ) -> Result<(RunRecord, Comparison), String> {
        let keys = self.recipes.keys();
        let original = &repeat.original;
        let dir = run.results_dir.as_path();
        let bundle = result_bundle(dir, self.end_time).map_err(|e| e.to_string())?;
        let numbers = key_numbers(dir, self.end_time).map_err(|e| e.to_string())?;
        let store = Store::create(store_dir(&self.path)).map_err(|e| e.to_string())?;
        let (blob, size) = store_bundle(&store, &bundle).map_err(|e| e.to_string())?;
        let comparison = compare(&original.key_numbers, &numbers, repeat.tolerance);

        let record = RunRecord {
            id: next_run_id(runs),
            rerun_of: Some(original.id.clone()),
            mesh: None,
            result: Some(Output {
                key: keys.result,
                recipe: self.recipes.result.clone(),
                blob,
                size,
            }),
            provenance: Provenance {
                geometry_hash: self.recipes.mesh.geometry.hash(),
                mesh_key: keys.mesh,
                mesh_hash: original.provenance.mesh_hash,
                result_key: Some(keys.result),
                settings: original.provenance.settings.clone(),
                tool: self.recipes.mesh.tool.clone(),
                host: run.host,
            },
            checks: run.checks,
            key_numbers: numbers,
            comparison: Some(comparison.clone()),
        };
        Ok((record, comparison))
    }

    fn finish_make(self, run: CaseRun) -> DocOutcome {
        let operation = operation_of(&self.action);
        let opened = match self.reopen(&run) {
            Ok(opened) => opened,
            Err(outcome) => return *outcome,
        };
        let mut document = opened.document;
        let record = match self.record(run, &document.artifacts.runs) {
            Ok(record) => record,
            Err(reason) => return self.not_recorded(&reason),
        };
        let id = record.id.clone();
        if let Err(e) = document.add_run(record) {
            return self.not_recorded(&e.to_string());
        }
        if let Err(e) = save(&document, &self.path) {
            return refused(operation, problem(not_written(&self.path, e)));
        }

        let mut data = run_data(Some(id.clone()), self.pending, &self.recipes.keys());
        let line = match &self.action {
            JobAction::Item(stage) => {
                // The other stage was not asked for, so it was not reused either.
                data.reused.clear();
                Line::ItemUpdated {
                    item: format!("{}:{}", doc_label(&self.path), stage.as_str()),
                    run: id,
                }
            }
            _ => Line::RunRecorded {
                run: id,
                stages: data.recorded.join(", "),
            },
        };
        let report = Report::from_diagnostics(operation, self.warnings, Some(self.hash))
            .with_migration(saved_migration(&opened.notes))
            .with_artifact("document", shown(&self.path))
            .with_repro(Repro::Run(data));
        DocOutcome {
            report,
            line: Some(line),
        }
    }

    fn finish_rerun(self, run: CaseRun, repeat: &Repeat) -> DocOutcome {
        let operation = operation_of(&self.action);
        let opened = match self.reopen(&run) {
            Ok(opened) => opened,
            Err(outcome) => return *outcome,
        };
        let mut document = opened.document;
        let (record, comparison) = match self.record_rerun(run, &document.artifacts.runs, repeat) {
            Ok(done) => done,
            Err(reason) => return self.not_recorded(&reason),
        };
        let id = record.id.clone();
        if let Err(e) = document.add_run(record) {
            return self.not_recorded(&e.to_string());
        }
        if let Err(e) = save(&document, &self.path) {
            return refused(operation, problem(not_written(&self.path, e)));
        }

        let (found, line, failed) = judgement(&comparison, &id, &repeat.original.id);
        let mut diagnostics = self.warnings;
        diagnostics.extend(found);
        let data = ComparisonData {
            original: repeat.original.id.clone(),
            other: id,
            comparison,
        };
        let mut report = Report::from_diagnostics(operation, diagnostics, Some(self.hash))
            .with_migration(saved_migration(&opened.notes))
            .with_artifact("document", shown(&self.path))
            .with_repro(Repro::Comparison(Box::new(data)));
        if failed {
            report.set_class(ExitClass::GateFailed);
        }
        DocOutcome {
            report,
            line: Some(line),
        }
    }
}

/// Solve the document at `path` with `runner`: run the case if the mesh or the result is out of
/// date, store what it made and record the run. When nothing is out of date nothing runs.
pub fn solve_document(path: &Path, runner: &mut dyn CaseRunner) -> DocOutcome {
    match plan_job(path, JobAction::Solve) {
        Ok(job) => job.run_with(runner),
        Err(outcome) => *outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::save_document;
    use cfdcraft_capture::fixture::{FixtureSpec, write_results};

    const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

    struct Fixture {
        root: PathBuf,
        calls: usize,
    }

    impl CaseRunner for Fixture {
        fn run_case(&mut self, _study_json: &str) -> Result<CaseRun, Box<Report>> {
            self.calls += 1;
            let dir = self.root.join(format!("call-{}", self.calls));
            let spec = FixtureSpec {
                end_time: 5000,
                cells: 16384,
                samples: 64,
                shift: 0.0,
            };
            write_results(&dir, &spec).expect("the fixture folder is written");
            Ok(CaseRun {
                results_dir: dir,
                checks: vec![Check {
                    name: "evidence".to_owned(),
                    passed: true,
                    detail: "passed".to_owned(),
                }],
                host: Some("fixture-host".to_owned()),
            })
        }
    }

    #[test]
    fn solving_a_fresh_document_records_run_1() {
        // Beside the test binary, inside the build folder (a lib test has no CARGO_TARGET_TMPDIR).
        let root = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("solve-fresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("cavity.cfdcraft");
        let saved = save_document(CAVITY, &file, false);
        assert_eq!(saved.report.class(), ExitClass::Passed);

        let mut runner = Fixture {
            root: root.join("runs"),
            calls: 0,
        };
        let done = solve_document(&file, &mut runner);
        assert_eq!(done.report.class(), ExitClass::Passed);
        assert_eq!(runner.calls, 1);

        let document = open(&file).unwrap().document;
        assert_eq!(document.artifacts.runs.len(), 1);
        let run = &document.artifacts.runs[0];
        assert_eq!(run.id, "run-1");
        let mesh = run.mesh.as_ref().expect("the mesh is recorded");
        let result = run.result.as_ref().expect("the result is recorded");
        assert_eq!(document.artifacts.blobs.len(), 2);
        let store = Store::open(store_dir(&file)).unwrap();
        assert!(store.contains(&mesh.blob));
        assert!(store.contains(&result.blob));

        match done.report.repro {
            Some(Repro::Run(data)) => {
                assert_eq!(data.run.as_deref(), Some("run-1"));
                assert_eq!(data.recorded, ["mesh", "result"]);
                assert!(data.reused.is_empty());
            }
            other => panic!("expected a run record, found {other:?}"),
        }
    }
}
