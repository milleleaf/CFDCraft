//! Bringing out-of-date stages up to date across several documents, in an order the user can
//! choose where the dependencies allow it.
//!
//! An item is one stage of one document, written `name:mesh` or `name:result`, where the name is
//! the file stem of the document. The planner of the runs crate makes every decision about which
//! items are chosen and in what order; this module reads the documents, hands the planner the
//! items, and carries out its answer one item at a time. [`plan_update_report`] and
//! [`update_documents`] share one function for everything up to the answer, so what the plan shows
//! is exactly what the update would do and refuse.
//!
//! The case of a document runs once however many of its items are updated. Each item records its
//! own run with only that stage, and the document is saved after each one, so a failure part of
//! the way keeps what was recorded.
//!
//! Finding code: 008 an update selection or order is refused (an item that is not waiting, that
//! does not exist, that is made from one that is also waiting, an order that does not list the
//! chosen items exactly once or puts a result before its mesh, two documents with one name).

use std::path::PathBuf;

use cfdcraft_model::Diagnostic;
use cfdcraft_runs::{Item, PlanError, plan_update};
use cfdcraft_text::Line;

use crate::documents::{DocOutcome, Finding, problem, refused, shown};
use crate::jobs::{CaseRun, CaseRunner, JobAction, doc_label, pending_items, plan_job};
use crate::report::{ExitClass, Report};
use crate::repro::{PlanData, Repro, UpdateData, UpdatedItem};

const REFUSED: &str = "CFD-REPRO-008";

/// What a request asks for, once the documents were read and the planner answered.
struct Planned {
    labels: Vec<String>,
    order: Vec<Item>,
    /// Every item that is out of date, document by document.
    pending: Vec<Item>,
}

fn refusal(operation: &str, found: Finding) -> Box<DocOutcome> {
    Box::new(refused(operation, problem(found)))
}

/// The finding for a planner refusal.
fn refused_plan(error: PlanError) -> Finding {
    let line = match error {
        PlanError::BadItem(item) => Line::UnknownItem { item },
        PlanError::NotPending(item) => Line::NotPending {
            item: item.to_string(),
        },
        PlanError::NeedsUpstream { item, needs } => Line::NeedsUpstream {
            item: item.to_string(),
            needs: needs.to_string(),
        },
        PlanError::OrderMismatch => Line::OrderMismatch,
        PlanError::OrderBreaksDependency { item, needs } => Line::OrderBreaksDependency {
            item: item.to_string(),
            needs: needs.to_string(),
        },
    };
    (REFUSED, line)
}

/// The items a list of texts names. Each must be written `name:mesh` or `name:result` with the
/// name of one of the documents.
fn parse_items(
    operation: &str,
    texts: Option<&[String]>,
    labels: &[String],
) -> Result<Option<Vec<Item>>, Box<DocOutcome>> {
    let Some(texts) = texts else {
        return Ok(None);
    };
    let mut items = Vec::with_capacity(texts.len());
    for text in texts {
        match text.parse::<Item>() {
            Ok(item) if labels.contains(&item.doc) => items.push(item),
            _ => {
                let line = Line::UnknownItem { item: text.clone() };
                return Err(refusal(operation, (REFUSED, line)));
            }
        }
    }
    Ok(Some(items))
}

/// Read the documents, find what is out of date and let the planner choose and order it. Nothing
/// is written and nothing runs.
fn plan(
    operation: &str,
    paths: &[PathBuf],
    select: Option<&[String]>,
    order: Option<&[String]>,
) -> Result<Planned, Box<DocOutcome>> {
    let mut labels: Vec<String> = Vec::with_capacity(paths.len());
    for path in paths {
        let label = doc_label(path);
        if labels.contains(&label) {
            let line = Line::DuplicateName { name: label };
            return Err(refusal(operation, (REFUSED, line)));
        }
        labels.push(label);
    }

    let mut pending = Vec::new();
    for (path, label) in paths.iter().zip(&labels) {
        pending.extend(pending_items(operation, path, label)?);
    }
    let select = parse_items(operation, select, &labels)?;
    let order = parse_items(operation, order, &labels)?;
    let order = plan_update(&pending, select.as_deref(), order.as_deref())
        .map_err(|e| refusal(operation, refused_plan(e)))?;
    Ok(Planned {
        labels,
        order,
        pending,
    })
}

fn texts(items: &[Item]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

/// What an update that went wrong answers with: the report of what stopped it, named for the
/// update, and the items that were made before it stopped.
fn stopped(outcome: DocOutcome, updated: Vec<UpdatedItem>) -> DocOutcome {
    DocOutcome {
        report: outcome
            .report
            .for_operation("update")
            .with_repro(Repro::Update(UpdateData { updated })),
        line: outcome.line,
    }
}

/// What an update of the documents at `paths` would do: the items that are out of date, and the
/// order it would make the ones chosen in. It refuses what an update would refuse, and it never
/// writes or runs anything.
///
/// Without `select` every item that is out of date is chosen. Without `order` the items go
/// document by document, in the order of `paths`, the mesh before the result. With `order` it
/// must list each chosen item exactly once and keep each mesh before the result made from it.
pub fn plan_update_report(
    paths: &[PathBuf],
    select: Option<&[String]>,
    order: Option<&[String]>,
) -> DocOutcome {
    const OP: &str = "plan-update";
    let planned = match plan(OP, paths, select, order) {
        Ok(planned) => planned,
        Err(outcome) => return *outcome,
    };
    let mut report = Report::from_diagnostics(OP, Vec::new(), None);
    for path in paths {
        report = report.with_artifact("document", shown(path));
    }
    let line = planned.order.is_empty().then_some(Line::NothingToUpdate);
    let data = PlanData {
        pending: texts(&planned.pending),
        order: texts(&planned.order),
    };
    DocOutcome {
        report: report.with_repro(Repro::Plan(data)),
        line,
    }
}

/// Bring the items of the documents at `paths` up to date, in the order [`plan_update_report`]
/// shows for the same `select` and `order`.
///
/// Each item runs through [`plan_job`] on the file as it is then, so a mesh recorded a moment
/// ago is seen by the result made from it. The case runs once per document, the first time one of
/// its items is made. A failure of the runner, or of recording an item, stops the update and
/// keeps what was recorded before it.
pub fn update_documents(
    paths: &[PathBuf],
    select: Option<&[String]>,
    order: Option<&[String]>,
    runner: &mut dyn CaseRunner,
) -> DocOutcome {
    const OP: &str = "update";
    let planned = match plan(OP, paths, select, order) {
        Ok(planned) => planned,
        Err(outcome) => return *outcome,
    };

    let mut ran: Vec<Option<CaseRun>> = vec![None; paths.len()];
    let mut updated: Vec<UpdatedItem> = Vec::new();
    let mut warnings: Vec<Diagnostic> = Vec::new();
    let mut last = None;
    for item in &planned.order {
        let Some(at) = planned.labels.iter().position(|label| *label == item.doc) else {
            continue;
        };
        let job = match plan_job(&paths[at], JobAction::Item(item.stage)) {
            Ok(job) => job,
            Err(outcome) => return stopped(*outcome, updated),
        };
        let run = match ran[at].clone() {
            Some(run) => run,
            None => match runner.run_case(job.study_json()) {
                Ok(run) => {
                    ran[at] = Some(run.clone());
                    run
                }
                Err(report) => {
                    let outcome = DocOutcome {
                        report: *report,
                        line: None,
                    };
                    return stopped(outcome, updated);
                }
            },
        };

        let done = job.finish(run);
        if done.report.class() != ExitClass::Passed {
            return stopped(done, updated);
        }
        if let Some(Line::ItemUpdated { item: text, run }) = &done.line {
            updated.push(UpdatedItem {
                item: text.clone(),
                run: run.clone(),
            });
        }
        for found in done.report.diagnostics {
            if !warnings.contains(&found) {
                warnings.push(found);
            }
        }
        last = done.line;
    }

    let mut report = Report::from_diagnostics(OP, warnings, None);
    for path in paths {
        report = report.with_artifact("document", shown(path));
    }
    let line = if updated.is_empty() {
        Some(Line::NothingToUpdate)
    } else {
        last
    };
    DocOutcome {
        report: report.with_repro(Repro::Update(UpdateData { updated })),
        line,
    }
}
