//! The real case runner: it runs the case on the compute host through the structured run, and
//! hands the folder the run left behind to the job that records it.
//!
//! A real run needs a host. Every test of the job operations uses a fixture runner instead.

use std::path::Path;

use cfdcraft_runs::Check;

use crate::jobs::{CaseRun, CaseRunner, DocJob, not_recorded_report};
use crate::report::{ExitClass, Report};
use crate::run::{Finished, RunRequest, run_study};

/// Runs a case with [`run_study`] and answers with the folder of the run it made.
#[derive(Debug)]
pub struct HostRunner {
    request: RunRequest,
    detail: Option<String>,
}

impl HostRunner {
    pub fn new(request: RunRequest) -> Self {
        Self {
            request,
            detail: None,
        }
    }

    /// The executor's full text about the last run, for the operator's terminal only. It can name
    /// the key, the host and remote folders, so it never goes into a report or a record.
    pub fn take_detail(&mut self) -> Option<String> {
        self.detail.take()
    }
}

impl CaseRunner for HostRunner {
    fn run_case(&mut self, study_json: &str) -> Result<CaseRun, Box<Report>> {
        let done = run_study(study_json, &self.request);
        self.detail = done.detail;
        case_run_from(&done.report, &self.request.runs_dir, &self.request.profile)
    }
}

/// The folder a passing run left, with the checks it was judged by and the label of the machine.
/// A report that did not pass, or that names no run, comes back as the error.
pub(crate) fn case_run_from(
    report: &Report,
    runs_dir: &Path,
    profile: &str,
) -> Result<CaseRun, Box<Report>> {
    if report.class() != ExitClass::Passed {
        return Err(Box::new(report.clone()));
    }
    let Some(run_id) = report.run.as_ref().map(|run| run.run_id.clone()) else {
        let reason = "the run reported no results folder";
        return Err(Box::new(not_recorded_report(Vec::new(), None, reason)));
    };

    let evidence = match &report.evidence {
        Some(evidence) => Check {
            name: "evidence".to_owned(),
            passed: evidence.verdict != "failed",
            detail: evidence.verdict.clone(),
        },
        None => Check {
            name: "evidence".to_owned(),
            passed: false,
            detail: "no verdict was reported".to_owned(),
        },
    };
    let mut checks = vec![evidence];
    if let Some(gate) = &report.gate {
        let deviations = gate
            .metrics
            .iter()
            .map(|metric| format!("{} {:.6}", metric.name, metric.max_abs_dev))
            .collect::<Vec<_>>()
            .join(", ");
        checks.push(Check {
            name: "ghia".to_owned(),
            passed: gate.passed,
            detail: format!("largest deviations: {deviations}"),
        });
    }

    Ok(CaseRun {
        results_dir: runs_dir.join(&run_id).join("results"),
        checks,
        host: Some(profile.to_owned()),
        np: None,
        stopped_at: None,
    })
}

/// Record a finished background run in its document. A run that did not pass is the answer as it
/// is, with the job's operation; a passing one is handed to the job, which records it only if every
/// check held.
pub(crate) fn finish_job(job: DocJob, runs_dir: &Path, profile: &str, done: Finished) -> Report {
    let operation = job.operation();
    match case_run_from(&done.report, runs_dir, profile) {
        Ok(run) => job.finish(run).report,
        Err(report) => report.for_operation(operation),
    }
}
