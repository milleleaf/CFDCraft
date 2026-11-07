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
use cfdcraft_doc::{
    DocError, Document, Opened, Stamp, open, open_stamped, save_if_unchanged, store_dir,
};
use cfdcraft_keys::{Stage, StageKeys, StageRecipes, ToolIdentity, diff_mesh};
use cfdcraft_model::{Diagnostic, Severity, Study};
use cfdcraft_runs::{
    Check, Item, Output, Provenance, RunRecord, StageState, Status, find_run, is_plain_name,
    latest_mesh, latest_result, next_run_id, status_for, verify_runs,
};
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_text::Line;

use crate::documents::{
    DocOutcome, Finding, finding, is_pack_name, migration_with, not_written, pack_kept, problem,
    read_failure, refused, shown,
};
use crate::inspect::judgement;
use crate::keys::tool_identity;
use crate::prepare::{Prepared, prepare};
use crate::reasons::reason_text;
use crate::report::{ExitClass, Report};
use crate::repro::{ComparisonData, Repro, RunData};
use crate::scenario_ops::effective_of;

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
    /// How the tools of this build differ from those the run was made with, when they do.
    tool_changed: Option<String>,
}

/// A document that has been looked at and has work to do.
#[derive(Debug)]
pub struct DocJob {
    path: PathBuf,
    action: JobAction,
    study_json: String,
    /// The study the case is made from: the document's own, or the effective study of a variant.
    effective: Study,
    /// The variant the job is for; `None` is the study itself.
    scenario: Option<String>,
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

/// `report`, with the variant it is about named as an artifact when there is one.
fn with_scenario(report: Report, scenario: Option<&str>) -> Report {
    match scenario {
        Some(id) => report.with_artifact("scenario", id),
        None => report,
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

/// A file that cannot be opened, as the refusal a solve answers with. A run history that does not
/// hold together is told apart from a file that is not a study file.
pub(crate) fn open_refusal(operation: &str, error: &DocError, path: &Path) -> DocOutcome {
    match error {
        DocError::RunLog(reason) => refused(operation, problem(inconsistent(reason))),
        DocError::Scenarios(reason) => refused(
            operation,
            problem((
                "CFD-SCN-002",
                Line::ScenarioRefused {
                    reason: reason.clone(),
                },
            )),
        ),
        other => refused(operation, problem(read_failure(other, path))),
    }
}

/// Whether the blob `hash` is in the store as a file of the size the record gave it. A blob that
/// was cut short, emptied or swapped for another length counts as gone; a full check of the content
/// is the store's `verify`, which packing runs.
pub(crate) fn blob_present(store: Option<&Store>, hash: &ContentHash, size: u64) -> bool {
    store.is_some_and(|store| {
        std::fs::metadata(store.blob_path(hash)).is_ok_and(|m| m.is_file() && m.len() == size)
    })
}

/// What a job does with the results that were removed on purpose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobOptions {
    /// Make a result that was pruned again. Left off, a pruned result is not out of date and
    /// nothing is run for it.
    pub remake_pruned: bool,
}

/// The results the document lists as pruned, and whether a job makes them again.
#[derive(Debug, Clone, Copy)]
struct Pruned<'a> {
    hashes: &'a [ContentHash],
    remake: bool,
}

/// Whether a stage has to be made. A fresh stage whose stored blob is gone (or is not the size the
/// record gave it) counts as out of date, and the warning that says so is added. `blob` is the hash
/// and size the newest record of the stage holds. A blob that was pruned on purpose is not gone by
/// accident: it has no warning, and the stage waits only when pruned results are to be remade.
fn stage_pending(
    stage: Stage,
    state: &StageState,
    blob: Option<(ContentHash, u64)>,
    present: &dyn Fn(&ContentHash, u64) -> bool,
    pruned: Pruned<'_>,
    warnings: &mut Vec<Diagnostic>,
) -> bool {
    if !matches!(state, StageState::Fresh { .. }) {
        return true;
    }
    match blob {
        Some((hash, size)) if present(&hash, size) => false,
        Some((hash, _)) if pruned.hashes.contains(&hash) => pruned.remake,
        Some((hash, _)) => {
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

/// A document that was read, its study prepared, and its stages compared with the records.
struct Looked {
    document: Document,
    effective: Study,
    scenario: Option<String>,
    study_json: String,
    prepared: Prepared,
    recipes: StageRecipes,
    state: Status,
}

/// Open the document at `path`, prepare the study of `scenario` (the document's own when it is
/// `None`) and say where each stage stands. The file is not changed.
fn look(
    operation: &str,
    path: &Path,
    tool: &ToolIdentity,
    scenario: Option<&str>,
) -> Result<Looked, Box<DocOutcome>> {
    if is_pack_name(path) {
        return Err(Box::new(refused(operation, problem(pack_kept(path)))));
    }
    let opened = open(path).map_err(|e| Box::new(open_refusal(operation, &e, path)))?;
    if opened.packed_entries > 0 {
        return Err(Box::new(refused(operation, problem(pack_kept(path)))));
    }
    let document = opened.document;
    let (effective, tag) = effective_of(operation, &document, scenario)?;
    let mut study_json = serde_json::to_string_pretty(&effective).expect("a study is plain data");
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
    let state = status_for(runs, &recipes, tag.as_deref());
    Ok(Looked {
        document,
        effective,
        scenario: tag,
        study_json,
        prepared,
        recipes,
        state,
    })
}

/// Refuse a job whose record would not fit in the file, before its case runs and not after.
fn room_for_run(operation: &str, path: &Path, document: &Document) -> Result<(), Box<DocOutcome>> {
    document
        .check_room_for_run()
        .map_err(|e| Box::new(refused(operation, problem(not_written(path, e)))))
}

/// The stages of the document at `path` that are out of date, as items called `label`, the mesh
/// first. The file is not changed.
pub(crate) fn pending_items(
    operation: &str,
    path: &Path,
    label: &str,
) -> Result<Vec<Item>, Box<DocOutcome>> {
    let looked = look(operation, path, &tool_identity(Dialect::EsiV2512), None)?;
    let runs = &looked.document.artifacts.runs;
    // The same question a solve asks, so the plan and the job cannot disagree: a fresh stage whose
    // stored blob is gone is waiting too.
    let pruned = Pruned {
        hashes: &looked.document.artifacts.pruned,
        remake: false,
    };
    let found = pending_stages(
        path,
        &looked.recipes,
        &looked.state,
        runs,
        pruned,
        &mut Vec::new(),
    );
    let items: Vec<Item> = [(Stage::Mesh, found.mesh), (Stage::Result, found.result)]
        .into_iter()
        .filter(|(_, waiting)| *waiting)
        .map(|(stage, _)| Item::new(label, stage))
        .collect();
    if !items.is_empty() {
        room_for_run(operation, path, &looked.document)?;
    }
    Ok(items)
}

/// Which stages of the document at `path` are out of date: those that are stale or never made, and
/// those that are fresh by their records but whose stored blob is gone. The warning for each blob
/// that is gone is added to `warnings`; a blob that was pruned on purpose has none, and its stage
/// waits only when `pruned` asks for pruned results to be made again.
fn pending_stages(
    path: &Path,
    recipes: &StageRecipes,
    state: &Status,
    runs: &[RunRecord],
    pruned: Pruned<'_>,
    warnings: &mut Vec<Diagnostic>,
) -> Pending {
    let keys = recipes.keys();
    let store = Store::open(store_dir(path)).ok();
    let present = |hash: &ContentHash, size: u64| blob_present(store.as_ref(), hash, size);
    Pending {
        mesh: stage_pending(
            Stage::Mesh,
            &state.mesh,
            latest_mesh(runs, keys.mesh).map(|(_, out)| (out.blob, out.size)),
            &present,
            pruned,
            warnings,
        ),
        result: stage_pending(
            Stage::Result,
            &state.result,
            latest_result(runs, keys.result).map(|(_, out)| (out.blob, out.size)),
            &present,
            pruned,
            warnings,
        ),
    }
}

/// Look at the document at `path` and decide what `action` has to make.
///
/// An answer that is already final comes back as the error: a refusal (a file that cannot be
/// read, a pack, a study that fails the checks, run records that disagree, an item that is not
/// waiting, a run that cannot be repeated), or the report that nothing is out of date. The
/// document is never changed here.
pub fn plan_job(path: &Path, action: JobAction) -> Result<DocJob, Box<DocOutcome>> {
    plan_scenario_job(path, None, action)
}

/// [`plan_job`] for the variant `scenario` of the document. `None` and `base` mean the study
/// itself and record no tag.
///
/// A variant that does not exist and a change of a variant that does not apply to the study are
/// refused here, before anything runs. A rerun takes its variant from the run it repeats and
/// ignores `scenario`.
pub fn plan_scenario_job(
    path: &Path,
    scenario: Option<&str>,
    action: JobAction,
) -> Result<DocJob, Box<DocOutcome>> {
    plan_scenario_job_with(path, scenario, action, &JobOptions::default())
}

/// [`plan_scenario_job`] with options. With `remake_pruned` set, a stage whose stored result was
/// pruned counts as out of date and is made again; otherwise it is left as it is.
pub fn plan_scenario_job_with(
    path: &Path,
    scenario: Option<&str>,
    action: JobAction,
    options: &JobOptions,
) -> Result<DocJob, Box<DocOutcome>> {
    plan_with_tool(
        path,
        scenario,
        action,
        tool_identity(Dialect::EsiV2512),
        options,
    )
}

/// [`plan_job`] for another set of tools than this build's. It exists so tests can make a
/// document with tools other than the ones in use; no front end calls it.
#[doc(hidden)]
pub fn plan_job_with_tool(
    path: &Path,
    action: JobAction,
    tool: ToolIdentity,
) -> Result<DocJob, Box<DocOutcome>> {
    plan_with_tool(path, None, action, tool, &JobOptions::default())
}

fn plan_with_tool(
    path: &Path,
    scenario: Option<&str>,
    action: JobAction,
    tool: ToolIdentity,
    options: &JobOptions,
) -> Result<DocJob, Box<DocOutcome>> {
    if let JobAction::Rerun {
        run,
        tolerance,
        allow_tool_change,
    } = &action
    {
        return plan_rerun(path, run, *tolerance, *allow_tool_change, &tool);
    }
    plan_make(path, scenario, action, &tool, options)
}

/// Plan a solve, or the update of one stage.
fn plan_make(
    path: &Path,
    requested: Option<&str>,
    action: JobAction,
    tool: &ToolIdentity,
    options: &JobOptions,
) -> Result<DocJob, Box<DocOutcome>> {
    let operation = operation_of(&action);
    let looked = look(operation, path, tool, requested)?;
    let Looked {
        document,
        effective,
        scenario,
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
    let pruned = Pruned {
        hashes: &document.artifacts.pruned,
        remake: options.remake_pruned,
    };
    let found = pending_stages(path, &recipes, &state, runs, pruned, &mut warnings);

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
                .with_artifact("document", shown(path));
            let report = with_scenario(report, scenario.as_deref())
                .with_repro(Repro::Run(run_data(None, found, &keys)));
            return Err(Box::new(DocOutcome {
                report,
                line: Some(Line::NothingToRun),
            }));
        }
        found
    };

    room_for_run(operation, path, &document)?;
    Ok(DocJob {
        path: path.to_owned(),
        action,
        study_json,
        effective,
        scenario,
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
    let tool_changed = (!changes.is_empty()).then(|| reason_text(&changes));
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

    room_for_run(OP, path, &document)?;
    let effective = original.provenance.settings.clone();
    let scenario = original.scenario.clone();
    let repeat = Repeat {
        original: original.clone(),
        tolerance,
        tool_changed,
    };
    Ok(DocJob {
        path: path.to_owned(),
        action: JobAction::Rerun {
            run: wanted.to_owned(),
            tolerance,
            allow_tool_change,
        },
        study_json,
        effective,
        scenario,
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
    /// run held. The stamp is that of the bytes read, for the save that follows.
    fn reopen(&self, run: &CaseRun) -> Result<(Opened, Stamp), Box<DocOutcome>> {
        let operation = operation_of(&self.action);
        let (opened, stamp) = open_stamped(&self.path)
            .map_err(|e| Box::new(open_refusal(operation, &e, &self.path)))?;
        let moved = opened.packed_entries > 0
            || opened.document.study != self.document.study
            || opened.document.artifacts.scenarios != self.document.artifacts.scenarios
            || opened.document.artifacts.runs.len() != self.document.artifacts.runs.len();
        if moved {
            return Err(Box::new(self.changed()));
        }
        if let Some(check) = run.checks.iter().find(|check| !check.passed) {
            let reason = format!("the check \"{}\" did not pass", check.name);
            return Err(Box::new(self.not_recorded(&reason)));
        }
        Ok((opened, stamp))
    }

    /// The answer when the document is not what the job was planned from.
    fn changed(&self) -> DocOutcome {
        let operation = operation_of(&self.action);
        let reason = format!("the document changed while the case ran; {operation} it again");
        self.not_recorded(&reason)
    }

    /// Capture the output of the run the job is waiting for and put it into the store. Blobs are
    /// named by their content, so storing them before the document is looked at again does no
    /// harm when the record is then refused.
    fn store_output(&self, run: &CaseRun) -> Result<Stored, String> {
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
        let mesh = mesh
            .map(|bundle| store_bundle(&store, &bundle))
            .transpose()
            .map_err(|e| e.to_string())?;
        let result = result
            .map(|bundle| store_bundle(&store, &bundle))
            .transpose()
            .map_err(|e| e.to_string())?;
        Ok(Stored {
            mesh,
            result,
            numbers,
            regenerated_mesh: None,
        })
    }

    /// Build the record of the run from what was stored, as the next entry of `runs`.
    fn record(
        &self,
        run: CaseRun,
        stored: Stored,
        runs: &[RunRecord],
    ) -> Result<RunRecord, String> {
        let keys = self.recipes.keys();
        let Stored {
            mesh: mesh_blob,
            result: result_blob,
            numbers,
            ..
        } = stored;
        let mesh_hash = match mesh_blob {
            Some((hash, _)) => hash,
            None => latest_mesh(runs, keys.mesh)
                .map(|(_, out)| out.blob)
                .ok_or("the mesh this run stands on is not in the run records")?,
        };

        Ok(RunRecord {
            id: next_run_id(runs),
            rerun_of: None,
            scenario: self.scenario.clone(),
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
                settings: self.effective.clone(),
                tool: self.recipes.mesh.tool.clone(),
                host: run.host,
            },
            checks: run.checks,
            key_numbers: numbers,
            comparison: None,
        })
    }

    /// Build the record of a rerun from what was stored, with the comparison against the run it
    /// repeats. The settings and the mesh are the original's.
    fn record_rerun(
        &self,
        run: CaseRun,
        stored: Stored,
        runs: &[RunRecord],
        repeat: &Repeat,
    ) -> Result<(RunRecord, Comparison), String> {
        let keys = self.recipes.keys();
        let original = &repeat.original;
        let (blob, size) = stored.result.ok_or("the rerun stored no result")?;
        let comparison = compare(&original.key_numbers, &stored.numbers, repeat.tolerance);
        // With other tools the mesh the run stood on is the one it regenerated, not the recorded
        // one, and the record says which tools made it.
        let mesh_hash = stored
            .regenerated_mesh
            .unwrap_or(original.provenance.mesh_hash);
        let mut checks = run.checks;
        if let Some(reasons) = &repeat.tool_changed {
            let same = Tolerance::same_image();
            let note = if repeat.tolerance == same {
                "; the numbers were compared under the tight tolerance for the same image, which another image may not meet"
            } else {
                ""
            };
            checks.push(Check {
                name: "tool_changed".to_owned(),
                passed: true,
                detail: format!(
                    "made with other tools than {}: {reasons}{note}",
                    original.id
                ),
            });
        }

        let record = RunRecord {
            id: next_run_id(runs),
            rerun_of: Some(original.id.clone()),
            scenario: original.scenario.clone(),
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
                mesh_hash,
                result_key: Some(keys.result),
                settings: original.provenance.settings.clone(),
                tool: self.recipes.mesh.tool.clone(),
                host: run.host,
            },
            checks,
            key_numbers: stored.numbers,
            comparison: Some(comparison.clone()),
        };
        Ok((record, comparison))
    }

    /// Save the document with the run added, unless another save came in since it was read.
    fn save_run(&self, document: &Document, stamp: Stamp) -> Result<(), Box<DocOutcome>> {
        match save_if_unchanged(document, &self.path, stamp) {
            Ok(()) => Ok(()),
            Err(DocError::Changed(_)) => Err(Box::new(self.changed())),
            Err(e) => Err(Box::new(refused(
                operation_of(&self.action),
                problem(not_written(&self.path, e)),
            ))),
        }
    }

    fn finish_make(self, run: CaseRun) -> DocOutcome {
        let operation = operation_of(&self.action);
        // Refuse what can be refused before the output is read and stored.
        if let Err(outcome) = self.reopen(&run) {
            return *outcome;
        }
        let stored = match self.store_output(&run) {
            Ok(stored) => stored,
            Err(reason) => return self.not_recorded(&reason),
        };
        // The capture may have taken a long time: read the file again, build the record on what is
        // in it now, and save only if no one saved it since.
        let (opened, stamp) = match self.reopen(&run) {
            Ok(read) => read,
            Err(outcome) => return *outcome,
        };
        let mut document = opened.document;
        let record = match self.record(run, stored, &document.artifacts.runs) {
            Ok(record) => record,
            Err(reason) => return self.not_recorded(&reason),
        };
        let id = record.id.clone();
        if let Err(e) = document.add_run(record) {
            return self.not_recorded(&e.to_string());
        }
        if let Err(outcome) = self.save_run(&document, stamp) {
            return *outcome;
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
            .with_migration(migration_with(&opened.notes, true))
            .with_artifact("document", shown(&self.path));
        let report = with_scenario(report, self.scenario.as_deref()).with_repro(Repro::Run(data));
        DocOutcome {
            report,
            line: Some(line),
        }
    }

    fn finish_rerun(self, run: CaseRun, repeat: &Repeat) -> DocOutcome {
        let operation = operation_of(&self.action);
        if let Err(outcome) = self.reopen(&run) {
            return *outcome;
        }
        let mut stored = match self.store_output(&run) {
            Ok(stored) => stored,
            Err(reason) => return self.not_recorded(&reason),
        };
        if repeat.tool_changed.is_some() {
            match mesh_bundle(run.results_dir.as_path()).and_then(|bundle| bundle.hash()) {
                Ok(hash) => stored.regenerated_mesh = Some(hash),
                Err(e) => return self.not_recorded(&e.to_string()),
            }
        }
        let (opened, stamp) = match self.reopen(&run) {
            Ok(read) => read,
            Err(outcome) => return *outcome,
        };
        let mut document = opened.document;
        let (record, comparison) =
            match self.record_rerun(run, stored, &document.artifacts.runs, repeat) {
                Ok(done) => done,
                Err(reason) => return self.not_recorded(&reason),
            };
        let id = record.id.clone();
        if let Err(e) = document.add_run(record) {
            return self.not_recorded(&e.to_string());
        }
        if let Err(outcome) = self.save_run(&document, stamp) {
            return *outcome;
        }

        let (found, line, failed) = judgement(&comparison, &id, &repeat.original.id);
        let mut diagnostics = self.warnings;
        diagnostics.extend(found);
        let data = ComparisonData {
            original: repeat.original.id.clone(),
            other: id,
            comparison,
        };
        let report = Report::from_diagnostics(operation, diagnostics, Some(self.hash))
            .with_migration(migration_with(&opened.notes, true))
            .with_artifact("document", shown(&self.path));
        let mut report = with_scenario(report, self.scenario.as_deref())
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

/// What was captured from a run and put into the store: the blob and size of each stage the job
/// was waiting for, and the key numbers.
struct Stored {
    mesh: Option<(ContentHash, u64)>,
    result: Option<(ContentHash, u64)>,
    numbers: KeyNumbers,
    /// For a rerun made with other tools: the hash of the mesh that run regenerated.
    regenerated_mesh: Option<ContentHash>,
}

/// Solve the document at `path` with `runner`: run the case if the mesh or the result is out of
/// date, store what it made and record the run. When nothing is out of date nothing runs.
pub fn solve_document(path: &Path, runner: &mut dyn CaseRunner) -> DocOutcome {
    solve_scenario(path, None, runner)
}

/// [`solve_document`] for the variant `scenario`: its effective study goes through the same
/// checks and keys as the study itself, a mesh any record already holds is reused, and the run is
/// recorded with the variant and the effective study it was made from. `None` and `base` mean the
/// study itself.
pub fn solve_scenario(
    path: &Path,
    scenario: Option<&str>,
    runner: &mut dyn CaseRunner,
) -> DocOutcome {
    match plan_scenario_job(path, scenario, JobAction::Solve) {
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
