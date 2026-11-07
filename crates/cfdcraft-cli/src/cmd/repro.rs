//! The verbs that read a study file's runs (status, runs, result, provenance and compare) and the
//! verbs that run its case on the compute host (solve, rerun and update). Each one calls the
//! matching operation and prints its outcome.
//!
//! `status` and `compare` print like the other document verbs. `runs`, `result` and `provenance`
//! answer with data: their text output is the JSON data of the report on standard output, and the
//! report's findings go to standard error. The verbs that run the case also print the executor's
//! full text to standard error only, because it can name the key, the host and remote folders.

use std::path::{Path, PathBuf};

use cfdcraft_compare::Tolerance;
use cfdcraft_ops::{
    DocOutcome, ExitClass, HostRunner, JobAction, Repro, RunMode, RunRequest, plan_job,
};
use cfdcraft_text::{Line, Text};

use super::run::label;
use super::{Error, emit_doc};

/// Whether the mesh and the result of the study file are up to date, and if not why.
pub fn status(doc: &Path, json: bool) -> Result<i32, Error> {
    Ok(emit_doc(&cfdcraft_ops::document_status(doc), json))
}

/// The runs of the study file.
pub fn runs(doc: &Path, json: bool) -> Result<i32, Error> {
    print_data(&cfdcraft_ops::list_runs(doc), json)
}

/// The checks and key numbers of a run.
pub fn result(doc: &Path, run: Option<&str>, json: bool) -> Result<i32, Error> {
    print_data(&cfdcraft_ops::run_result(doc, run), json)
}

/// What a run was made from.
pub fn provenance(doc: &Path, run: Option<&str>, json: bool) -> Result<i32, Error> {
    print_data(&cfdcraft_ops::run_provenance(doc, run), json)
}

/// Compare the key numbers of run `other` with those of run `original`.
pub fn compare(
    doc: &Path,
    original: &str,
    other: &str,
    abs_tol: Option<f64>,
    rel_tol: Option<f64>,
    json: bool,
) -> Result<i32, Error> {
    let tolerance = tolerance(abs_tol, rel_tol)?;
    let outcome = cfdcraft_ops::compare_runs(doc, original, other, tolerance);
    Ok(emit_doc(&outcome, json))
}

/// The tolerance a person asked for; a part left out is the tight default for a rerun in the
/// same image. A part that is negative or not a finite number is refused.
pub fn tolerance(abs_tol: Option<f64>, rel_tol: Option<f64>) -> Result<Tolerance, Error> {
    let tight = Tolerance::same_image();
    Ok(Tolerance::new(
        abs_tol.unwrap_or(tight.abs),
        rel_tol.unwrap_or(tight.rel),
    )?)
}

/// Prints an outcome that carries data and returns the process exit status for it.
///
/// With `json` the report is the only thing on stdout and nothing goes to stderr. Otherwise the
/// data of a passing report is the only thing on stdout, and every note and finding goes to
/// stderr.
pub fn print_data(outcome: &DocOutcome, json: bool) -> Result<i32, Error> {
    let report = &outcome.report;
    if json {
        println!("{}", report.to_json());
    } else {
        let data = report
            .repro
            .as_ref()
            .filter(|_| report.class() == ExitClass::Passed);
        if let Some(data) = data {
            println!("{}", serde_json::to_string_pretty(data)?);
        }
        eprint!("{}", report.render_text());
    }
    Ok(i32::from(report.exit_class))
}

/// Which compute host runs the case, and how.
pub struct Host {
    /// The ssh key file.
    pub key: PathBuf,
    /// The name of the host in the hosts file; it is the only host text a study file records.
    pub profile: String,
    /// The hosts file.
    pub hosts: PathBuf,
    /// Run the Re = 100 cavity and compare it with the reference, instead of only judging the
    /// evidence.
    pub validate: bool,
}

/// The runner for the case of the study file `doc`: the runs and sockets go under `target` in the
/// current folder, as for the `run` verb.
fn host_runner(host: &Host, doc: &Path) -> Result<HostRunner, Error> {
    let target = std::env::current_dir()?.join("target");
    let mode = if host.validate {
        RunMode::Validate
    } else {
        RunMode::Run
    };
    let mut request = RunRequest::new(
        mode,
        &host.key,
        &host.hosts,
        &host.profile,
        target.join("cfdcraft-runs"),
        target.join("cfdcraft-sockets"),
    );
    request.label = label(doc);
    Ok(HostRunner::new(request))
}

/// Prints the outcome of a verb that ran the case, then the executor's full text, which is for
/// the operator's terminal only. Returns the process exit status.
fn emit_run(outcome: &DocOutcome, runner: &mut HostRunner, json: bool) -> i32 {
    let code = emit_doc(outcome, json);
    if let Some(detail) = runner.take_detail() {
        eprintln!("{detail}");
    }
    code
}

/// Make whatever mesh and result of the study file is out of date, and record the run.
pub fn solve(doc: &Path, host: &Host, json: bool) -> Result<i32, Error> {
    let mut runner = host_runner(host, doc)?;
    let outcome = cfdcraft_ops::solve_document(doc, &mut runner);
    Ok(emit_run(&outcome, &mut runner, json))
}

/// What the `rerun` verb was asked.
pub struct RerunArgs {
    pub doc: PathBuf,
    /// The run to repeat.
    pub run: String,
    pub host: Host,
    pub abs_tol: Option<f64>,
    pub rel_tol: Option<f64>,
    pub allow_tool_change: bool,
    pub json: bool,
}

/// Run the settings a recorded run was made from, and record the new run with its comparison.
pub fn rerun(args: &RerunArgs) -> Result<i32, Error> {
    let tolerance = tolerance(args.abs_tol, args.rel_tol)?;
    let mut runner = host_runner(&args.host, &args.doc)?;
    let action = JobAction::Rerun {
        run: args.run.clone(),
        tolerance,
        allow_tool_change: args.allow_tool_change,
    };
    let outcome = match plan_job(&args.doc, action) {
        Ok(job) => job.run_with(&mut runner),
        Err(refused) => *refused,
    };
    Ok(emit_run(&outcome, &mut runner, args.json))
}

/// What the `update` verb was asked.
pub struct UpdateArgs {
    pub docs: Vec<PathBuf>,
    /// The items to update, written `name:mesh` or `name:result`; all that are out of date when
    /// empty.
    pub items: Vec<String>,
    /// The order to update the chosen items in; the dependency order when empty.
    pub order: Vec<String>,
    /// Only show what would be done.
    pub plan: bool,
    pub key: Option<PathBuf>,
    pub profile: String,
    pub hosts: PathBuf,
    pub validate: bool,
    pub json: bool,
}

/// Show what an update would do, or bring the out-of-date items of the study files up to date.
pub fn update(args: &UpdateArgs) -> Result<i32, Error> {
    let select = (!args.items.is_empty()).then_some(args.items.as_slice());
    let order = (!args.order.is_empty()).then_some(args.order.as_slice());
    if args.plan {
        let shown = cfdcraft_ops::plan_update_report(&args.docs, select, order);
        return print_data(&shown, args.json);
    }
    let Some(key) = &args.key else {
        return Err(
            "update needs --key to run the case; use --plan to see what it would do".into(),
        );
    };
    let host = Host {
        key: key.clone(),
        profile: args.profile.clone(),
        hosts: args.hosts.clone(),
        validate: args.validate,
    };
    // The runs of one update share a label made from the first study file.
    let mut runner = host_runner(&host, &args.docs[0])?;
    let mut outcome = cfdcraft_ops::update_documents(&args.docs, select, order, &mut runner);
    say_updated(&mut outcome, args.json);
    Ok(emit_run(&outcome, &mut runner, args.json))
}

/// In text mode, one sentence for each item that was made, even when the update stopped part of
/// the way. They replace the single closing sentence the outcome carries.
fn say_updated(outcome: &mut DocOutcome, json: bool) {
    if json {
        return;
    }
    let Some(Repro::Update(data)) = &outcome.report.repro else {
        return;
    };
    if data.updated.is_empty() {
        return;
    }
    for made in &data.updated {
        let line = Line::ItemUpdated {
            item: made.item.clone(),
            run: made.run.clone(),
        };
        println!("{}", Text::english().render(&line));
    }
    outcome.line = None;
}
